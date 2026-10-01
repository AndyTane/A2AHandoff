//! V1 owns scheduling and durable handoff state. Platform I/O is delegated, never V0.
mod staged_delivery;
use handoff_core::message_template::MessageTemplates;
use handoff_core::{
    fingerprint, plan_one, AutomationOwnership, ConversationEvidence, DeliveryLedger, Direction,
    HandoffAnchor, PlannedAction, PollInput,
};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::atomic::{AtomicU64, Ordering},
    thread,
    time::{Duration, SystemTime, UNIX_EPOCH},
};
static SEQUENCE: AtomicU64 = AtomicU64::new(0);
fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}
fn text<'a>(v: &'a Value, k: &str) -> &'a str {
    v[k].as_str().unwrap_or("")
}
fn number(v: &Value, k: &str) -> u64 {
    v[k].as_u64().unwrap_or(0)
}
fn yes(v: &Value, k: &str) -> bool {
    v[k].as_bool().unwrap_or(false)
}
fn read(p: &Path) -> Option<Value> {
    let b = fs::read(p).ok()?;
    if b.len() > 8 * 1024 * 1024 {
        return None;
    }
    serde_json::from_slice(b.strip_prefix(&[239, 187, 191]).unwrap_or(&b)).ok()
}
fn write(p: &Path, v: &Value) -> Result<(), String> {
    let t = p.with_extension(format!("{}.tmp", std::process::id()));
    let mut f = fs::File::create(&t).map_err(|e| e.to_string())?;
    f.write_all(&serde_json::to_vec_pretty(v).map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())?;
    f.sync_all().map_err(|e| e.to_string())?;
    drop(f);
    fs::rename(t, p).map_err(|e| e.to_string())
}
/// Writes the UI-facing status cache, best effort.
///
/// `state.json` is derived: the durable workflow lives in `workflow.json`. A failed write
/// must therefore never stop the runtime. The atomic replace fails with "access denied"
/// whenever anything holds the file open without FILE_SHARE_DELETE - a virus scanner, a
/// user reading it, or a monitoring script polling it - and propagating that error used to
/// exit the process, leaving `runtime/state.<pid>.tmp` behind while the window stayed up
/// and reported 心跳过期 / 已停止监听: monitoring silently off, with nothing in the window
/// to say why. Retry briefly in case the holder is transient, then carry on; the next
/// publish is half a second away, so the heartbeat is only ever as fresh as the last
/// success and the UI's staleness check stays honest.
fn publish_state(root: &Path, doc: &Value) {
    let target = root.join("runtime/state.json");
    let mut last = String::new();
    for attempt in 0..3u32 {
        match write(&target, doc) {
            Ok(()) => return,
            Err(e) => {
                last = e;
                thread::sleep(Duration::from_millis(30 * u64::from(attempt + 1)));
            }
        }
    }
    diagnostic(
        &root.join("runtime"),
        "state_write_failed",
        &json!({"error": last}),
    );
}
fn hash(s: &str) -> String {
    hex::encode(Sha256::digest(
        s.replace("\r\n", "\n")
            .replace('\r', "\n")
            .trim()
            .as_bytes(),
    ))
}
fn binding_key(b: &Value) -> String {
    hash(&b.to_string())
}
fn hidden(c: &mut Command) {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        c.creation_flags(0x08000000);
    }
    #[cfg(not(windows))]
    {
        let _ = c;
    }
}
#[cfg(windows)]
fn alive(pid: u32) -> bool {
    #[link(name = "kernel32")]
    extern "system" {
        fn OpenProcess(access: u32, inherit: i32, pid: u32) -> isize;
        fn GetExitCodeProcess(h: isize, code: *mut u32) -> i32;
        fn CloseHandle(h: isize) -> i32;
    }
    unsafe {
        let h = OpenProcess(0x1000, 0, pid);
        if h == 0 {
            return false;
        }
        let mut code = 0;
        let ok = GetExitCodeProcess(h, &mut code) != 0 && code == 259;
        CloseHandle(h);
        ok
    }
}
#[cfg(not(windows))]
fn alive(pid: u32) -> bool {
    Path::new(&format!("/proc/{pid}")).exists()
}
struct Lock(PathBuf);
impl Drop for Lock {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}
fn acquire(home: &Path) -> Result<Lock, String> {
    let p = home.join("runtime.lock");
    if p.exists() {
        let previous =
            read(&p).ok_or("runtime lock is unreadable; refusing duplicate ownership")?;
        if alive(number(&previous, "pid") as u32) {
            return Err("another V1 runtime already owns this product".into());
        }
        fs::remove_file(&p).map_err(|e| e.to_string())?;
    }
    let mut f = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&p)
        .map_err(|e| e.to_string())?;
    f.write_all(json!({"pid":std::process::id()}).to_string().as_bytes())
        .map_err(|e| e.to_string())?;
    Ok(Lock(p))
}
fn diagnostic(home: &Path, phase: &str, data: &Value) {
    let p = home.join("events.jsonl");
    if fs::metadata(&p)
        .map(|m| m.len() > 2_000_000)
        .unwrap_or(false)
    {
        let _ = fs::remove_file(home.join("events.previous.jsonl"));
        let _ = fs::rename(&p, home.join("events.previous.jsonl"));
    }
    if let Ok(mut f) = fs::OpenOptions::new().create(true).append(true).open(p) {
        let _ = writeln!(f, "{}", json!({"at_ms":now(),"phase":phase,"data":data}));
    }
}
fn run(root: &Path, owner: u32, c: &mut Command) -> Result<Value, String> {
    let home = root.join("runtime");
    let n = SEQUENCE.fetch_add(1, Ordering::Relaxed);
    let out = home.join(format!("ipc/output-{}-{n}.json", std::process::id()));
    let err = out.with_extension("err");
    hidden(c);
    c.stdout(Stdio::from(
        fs::File::create(&out).map_err(|e| e.to_string())?,
    ))
    .stderr(Stdio::from(
        fs::File::create(&err).map_err(|e| e.to_string())?,
    ));
    let mut child = c.spawn().map_err(|e| e.to_string())?;
    let start = now();
    loop {
        if child.try_wait().map_err(|e| e.to_string())?.is_some() {
            break;
        }
        if now() - start > 30_000 || (owner != 0 && !alive(owner)) {
            let _ = child.kill();
            let _ = child.wait();
            return Err("adapter_timeout_or_ui_closed".into());
        }
        thread::sleep(Duration::from_millis(80));
    }
    let raw = fs::read_to_string(&out).unwrap_or_default();
    let v = raw
        .lines()
        .rev()
        .find_map(|l| serde_json::from_str::<Value>(l.trim_start_matches('\u{feff}')).ok());
    let e = fs::read_to_string(&err).unwrap_or_default();
    let _ = fs::remove_file(out);
    let _ = fs::remove_file(err);
    v.ok_or_else(|| {
        format!(
            "adapter_output_invalid: {}",
            e.chars().take(200).collect::<String>()
        )
    })
}
fn observe_dsh(root: &Path, owner: u32) -> Value {
    run(
        root,
        owner,
        Command::new("node.exe")
            .arg(root.join("adapters/dsh/observe.mjs"))
            .arg(root),
    )
    .unwrap_or_else(|e| json!({"ok":false,"error":e}))
}
fn observe_claude(
    root: &Path,
    owner: u32,
    b: &Value,
    cfg: &handoff_core::config::RuntimeConfig,
) -> Value {
    run(
        root,
        owner,
        Command::new("powershell.exe")
            .args(["-NoProfile", "-STA", "-ExecutionPolicy", "Bypass", "-File"])
            .arg(root.join("adapters/claude/observe.ps1"))
            .args([
                "-SessionId",
                text(b, "claude_session"),
                "-Title",
                text(b, "claude_window"),
                // Not `-Host`: PowerShell reserves $Host and binding would fail.
                "-ClaudeHost",
                &cfg.claude_host,
                "-IncludeReply",
            ]),
    )
    .unwrap_or_else(|e| json!({"ok":false,"error":e}))
}
fn blank(b: &Value) -> Value {
    json!({"schema":1,"binding_key":binding_key(b),"phase":"unclaimed","dsh_after_seq":0,"dsh_user_seq":0,"dsh_answer_seq":0,"claude_anchor_index":0,"claude_anchor_hash":"","claude_floor":0,"pending":null,"ledger":[],"notice":"请选择从当前哪一端回复接管"})
}
fn adopt(w: &mut Value, d: &Value, c: &Value, direction: &str) {
    w["pending"] = Value::Null;
    w["dsh_user_seq"] = d["user_seq"].clone();
    w["dsh_answer_seq"] = d["last_question_answer_seq"].clone();
    w["claude_anchor_index"] = c["latest_user_index"].clone();
    w["claude_anchor_hash"] = c["latest_user_hash"].clone();
    if direction == "DSH_TO_CLAUDE" {
        w["phase"] = json!("waiting_dsh");
        w["dsh_after_seq"] = json!(if yes(d, "busy") {
            number(d, "open_seq")
        } else {
            number(d, "user_seq")
        });
        w["claude_floor"] = c["ui_message_index"].clone();
    } else {
        w["phase"] = json!("waiting_claude");
        w["claude_floor"] = json!(0);
        // The anchor must point at the message the pending reply ANSWERS, not at the
        // reply itself. `correlation_confirmed()` requires the assistant message to be
        // strictly after `target_user_message_index`, so anchoring on the current last
        // message makes the check unsatisfiable and the planner refuses with
        // "claude_reply_not_correlated_to_tool_input" - which is what made 发送给 DSH
        // silently do nothing. (The live observer's `latest_user_index` is stable at the
        // reply's index, so lowering the anchor by one keeps it consistent.)
        w["claude_anchor_index"] = number(c, "ui_message_index").saturating_sub(1).into();
    }
    w["notice"] = json!("已从当前会话接管；历史交接不会补发");
}
/// A manual send button click means "send this now".
///
/// The automatic path arms a pending handoff and waits out `dispatch_delay_seconds`
/// so the user can cancel it. Clicking send while that countdown is running must
/// not be a no-op: it converts the armed handoff into an immediate one by clearing
/// the delay and putting the deadline in the past, keeping the same draft, id and
/// delivery identity. The countdown stays intact for purely automatic handoffs.
fn override_with_manual_send(p: &mut Value) {
    p["manual"] = json!(true);
    p["dispatch_delay_seconds"] = json!(0);
    p["deadline_ms"] = json!(now());
}
fn restore_listener(w: &mut Value, d: &Value, c: &Value) {
    w["pending"] = Value::Null;
    w["dsh_user_seq"] = d["user_seq"].clone();
    w["dsh_answer_seq"] = d["last_question_answer_seq"].clone();
    w["claude_anchor_index"] = c["latest_user_index"].clone();
    w["claude_anchor_hash"] = c["latest_user_hash"].clone();
    w["claude_floor"] = c["ui_message_index"].clone();
    w["phase"] = json!("waiting_dsh");
    w["dsh_after_seq"] = json!(if yes(d, "busy") {
        number(d, "open_seq").saturating_sub(1)
    } else {
        number(d, "last_seq")
    });
    w["notice"] = json!(if yes(d, "busy") {
        format!("已恢复监听：接续第 {} 轮，不重发任务", number(d, "turn"))
    } else {
        format!(
            "已恢复监听：等待第 {} 轮开始，不重发任务",
            number(d, "turn").saturating_add(1)
        )
    });
}
fn manual_changed(w: &Value, d: &Value, c: &Value) -> bool {
    (yes(d, "ok")
        && (number(d, "user_seq") != number(w, "dsh_user_seq")
            || number(d, "last_question_answer_seq") > number(w, "dsh_answer_seq")))
        || (yes(c, "ok")
            && (number(c, "latest_user_index") != number(w, "claude_anchor_index")
                || text(c, "latest_user_hash") != text(w, "claude_anchor_hash")))
}
fn direction(s: &str) -> Direction {
    if s == "DSH_TO_CLAUDE" {
        Direction::DshToClaude
    } else {
        Direction::ClaudeToDsh
    }
}
// Why the planner refused the last attempt.
//
// Every distinct cause used to surface as the same "没有新的完整回复，或该回复已经投递过",
// which made a stuck click impossible to diagnose from the outside. The planner
// already knew the reason; it just threw it away.
thread_local! {
    static LAST_PLAN_REASON: std::cell::RefCell<String> = const { std::cell::RefCell::new(String::new()) };
    static LAST_PLAN_VERDICT: std::cell::RefCell<String> = const { std::cell::RefCell::new(String::new()) };
}
fn last_plan_verdict() -> String {
    LAST_PLAN_VERDICT.with(|v| v.borrow().clone())
}
fn plan_reason(action: &PlannedAction) -> &'static str {
    match action {
        PlannedAction::None { reason } | PlannedAction::Hold { reason } => reason,
        PlannedAction::PausedByUser { reason } => reason,
        PlannedAction::Send { .. } => "send",
    }
}
fn last_plan_reason() -> String {
    LAST_PLAN_REASON.with(|r| r.borrow().clone())
}
/// Does this ledger entry prove the message reached the peer?
///
/// Only a verified `sent` receipt does. A `cancelled` entry means the draft was
/// written and then abandoned WITHOUT being submitted, so treating it as delivered
/// permanently blocks that same reply - which is how a cancelled handoff made every
/// later 发送给 Claude click report "已经投递过".
fn counts_as_delivered(state: &str) -> bool {
    state == "sent"
}
/// A receipt state that must end this delivery for good.
///
/// `sent` means it went out and `submit_uncertain` means a Send was fired without
/// confirmation; both must block any second attempt - that is the whole point of the
/// durable receipt. Every other state records a draft that was written and then
/// abandoned, so the delivery has NOT left the machine and must stay retryable.
/// Treating `cancelled_before_send` as terminal made a cancelled handoff impossible
/// to send even by an explicit click, which reads as the tool silently refusing.
fn receipt_is_terminal(state: &str) -> bool {
    matches!(state, "sent" | "submit_uncertain")
}
/// Has this reply been explicitly abandoned by the user?
///
/// This is a policy block, not a delivery claim: 「取消」 means "do not AUTO-send this
/// again". It must still be sendable when the user clicks the button on purpose, so
/// only the automatic path honours it.
fn abandoned(w: &Value, dir: &str, fp: u64) -> bool {
    w["ledger"].as_array().is_some_and(|a| {
        a.iter()
            .any(|e| text(e, "direction") == dir && number(e, "fingerprint") == fp)
            && !a.iter().any(|e| {
                text(e, "direction") == dir
                    && number(e, "fingerprint") == fp
                    && counts_as_delivered(text(e, "state"))
            })
    })
}
fn consumed(w: &Value, dir: &str, h: &str) -> bool {
    w["ledger"].as_array().is_some_and(|a| {
        a.iter().any(|e| {
            text(e, "direction") == dir
                && text(e, "source_hash") == h
                && counts_as_delivered(text(e, "state"))
        })
    })
}
fn record(w: &mut Value, p: &Value, state: &str) {
    let mut e = json!({"id":p["id"],"direction":p["direction"],"source_hash":p["source_hash"],"fingerprint":p["fingerprint"],"source_seq":p["source_seq"],"state":state,"at_ms":now()});
    e["binding_key"] = w["binding_key"].clone();
    let a = w["ledger"].as_array_mut().unwrap();
    a.push(e);
    if a.len() > 2000 {
        a.drain(..a.len() - 2000);
    }
}
fn load_message_templates(home: &Path) -> Result<MessageTemplates, String> {
    let path = home.join("message-templates.json");
    if !path.exists() {
        return Ok(MessageTemplates::default());
    }
    let value = read(&path).ok_or("交接文案配置无法读取")?;
    let templates: MessageTemplates = serde_json::from_value(value).map_err(|e| e.to_string())?;
    templates.validate()?;
    Ok(templates)
}
fn candidate(
    templates: &MessageTemplates,
    w: &Value,
    b: &Value,
    d: &Value,
    c: &Value,
    delay: u64,
    manual: bool,
) -> Option<Value> {
    candidate_for(templates, w, b, d, c, delay, manual, None)
}
/// Build the pending handoff.
///
/// `requested` is set only for a deliberate click, and names the side the user asked
/// for. The automatic path leaves it `None` and lets `plan_one` choose. This matters
/// because the planner is a PRIORITY function, not a filter: with a DSH result pending
/// it always proposes DSH -> Claude, so a click meant for the other direction would be
/// refused even though the reply it asked for is sitting there. The click is the user's
/// decision, so it picks the side and the planner is only consulted for the automatic
/// path.
#[allow(clippy::too_many_arguments)]
fn candidate_for(
    templates: &MessageTemplates,
    w: &Value,
    b: &Value,
    d: &Value,
    c: &Value,
    delay: u64,
    manual: bool,
    requested: Option<&str>,
) -> Option<Value> {
    // Cleared on entry: this is read after the call returns, and a stale value from an
    // earlier tick would otherwise be reported as this call's reason.
    LAST_PLAN_REASON.with(|r| r.borrow_mut().clear());
    if !yes(d, "ok") || !yes(c, "ok") {
        LAST_PLAN_REASON.with(|r| *r.borrow_mut() = "observation_unavailable".into());
        return None;
    }
    if !matches!(text(w, "phase"), "waiting_dsh" | "waiting_claude") {
        LAST_PLAN_REASON.with(|r| {
            *r.borrow_mut() = format!("phase_not_waiting({})", text(w, "phase"));
        });
        return None;
    }
    let ds = d["result"]["reply"].as_str().filter(|s| {
        !s.trim().is_empty() && number(&d["result"], "end_seq") > number(w, "dsh_after_seq")
    });
    let cs = c["reply_text"].as_str().filter(|_| {
        text(w, "phase") == "waiting_claude"
            && yes(c, "reply_available")
            && number(c, "ui_message_index") > number(w, "claude_floor")
    });
    if ds.is_none() && cs.is_none() {
        // Neither side offers anything. Report which condition failed on each side, so
        // a stuck click is diagnosable instead of collapsing into one sentence.
        let dsh = if d["result"]["reply"].as_str().is_none() {
            "no_result"
        } else if number(&d["result"], "end_seq") <= number(w, "dsh_after_seq") {
            "result_consumed"
        } else {
            "reply_empty"
        };
        let claude = if !yes(c, "reply_available") {
            "reply_unavailable"
        } else if text(w, "phase") != "waiting_claude" {
            "phase_not_waiting_claude"
        } else if number(c, "ui_message_index") <= number(w, "claude_floor") {
            "floor_not_passed"
        } else {
            "no_reply_text"
        };
        LAST_PLAN_REASON.with(|r| *r.borrow_mut() = format!("dsh:{dsh} claude:{claude}"));
    }
    let mut ledger = DeliveryLedger::default();
    for e in w["ledger"].as_array().unwrap() {
        // `plan_one`'s ledger answers "has this already gone out?". A `cancelled`
        // entry never reached the peer, so it must not answer yes - otherwise the
        // reply the user is trying to send reads as "已经投递过" forever. The
        // "don't AUTO-resend a cancelled reply" rule is enforced separately below.
        if counts_as_delivered(text(e, "state")) {
            ledger.record(direction(text(e, "direction")), number(e, "fingerprint"));
        }
    }
    let conv = ConversationEvidence {
        ownership: AutomationOwnership::Tool,
        latest_user_fingerprint: Some(fingerprint(text(c, "latest_user_hash"))),
        latest_user_message_index: Some(number(c, "latest_user_index")),
        latest_assistant_message_index: Some(number(c, "ui_message_index")),
        anchor: Some(HandoffAnchor {
            target_session: text(b, "claude_session").into(),
            tool_input_fingerprint: fingerprint(text(w, "claude_anchor_hash")),
            target_user_message_index: number(w, "claude_anchor_index"),
        }),
    };
    let result = plan_one(
        PollInput {
            dsh_busy: yes(d, "busy"),
            dsh_result: ds,
            dsh_result_complete: true,
            claude_reply: cs,
            claude_reply_complete: yes(c, "reply_available"),
            claude_conversation: Some(&conv),
        },
        &ledger,
    );
    // Recorded for every call so a refusal can report the planner's own verdict.
    LAST_PLAN_VERDICT.with(|v| {
        *v.borrow_mut() = json!({
            "action": plan_reason(&result),
            "ds_present": ds.is_some(),
            "cs_present": cs.is_some(),
            "dsh_after_seq": number(w, "dsh_after_seq"),
            "dsh_end_seq": number(&d["result"], "end_seq"),
            "claude_floor": number(w, "claude_floor"),
            "claude_ui_index": number(c, "ui_message_index"),
            "phase": text(w, "phase"),
            "correlation": conv.correlation_confirmed(),
        })
        .to_string();
    });
    // A deliberate click picks the side itself. Everything that safety depends on is
    // still enforced below: the observation must be readable, the reply must correlate
    // to the tool's own input, and `consumed`/`abandoned` still apply to the automatic
    // path. Only the planner's automatic priority ordering is bypassed.
    let (dir, body, seq, fp) = if let Some(want) = requested {
        if want == "DSH_TO_CLAUDE" {
            let body = ds?;
            (
                "DSH_TO_CLAUDE",
                body,
                number(&d["result"], "end_seq"),
                fingerprint(body),
            )
        } else {
            let body = cs?;
            // Correlation is what proves the reply answers the tool's own input rather
            // than something the user typed. It stays mandatory.
            if !conv.correlation_confirmed() {
                LAST_PLAN_REASON.with(|r| *r.borrow_mut() = "not_correlated".into());
                return None;
            }
            (
                "CLAUDE_TO_DSH",
                body,
                number(c, "ui_message_index"),
                fingerprint(body),
            )
        }
    } else {
        match result {
            PlannedAction::Send {
                direction: Direction::DshToClaude,
                fingerprint,
            } => (
                "DSH_TO_CLAUDE",
                ds?,
                number(&d["result"], "end_seq"),
                fingerprint,
            ),
            PlannedAction::Send {
                direction: Direction::ClaudeToDsh,
                fingerprint,
            } => (
                "CLAUDE_TO_DSH",
                cs?,
                number(c, "ui_message_index"),
                fingerprint,
            ),
            _ => {
                // The reason was previously discarded, which left "没有新的完整回复" as
                // the only clue for every distinct cause. Keep it so a stuck click is
                // diagnosable.
                LAST_PLAN_REASON.with(|r| *r.borrow_mut() = plan_reason(&result).to_string());
                return None;
            }
        }
    };
    let source_hash = hash(body);
    // `consumed` and `abandoned` are both automatic-handoff policies: "this exact
    // content already went out, don't send it twice" and "the user already cancelled
    // this". A manual click is the user looking at the reply and telling the tool to
    // hand it over now, so it must override both - otherwise a reply that was ever
    // delivered (or cancelled) becomes permanently impossible to re-send, and the
    // click silently does nothing. The automatic path still honours them, which is
    // what keeps polling from re-delivering the same content.
    if !manual && (consumed(w, dir, &source_hash) || abandoned(w, dir, fp)) {
        LAST_PLAN_REASON.with(|r| {
            *r.borrow_mut() = if consumed(w, dir, &source_hash) {
                "already_delivered".to_string()
            } else {
                "cancelled_earlier".to_string()
            };
        });
        return None;
    }
    let id = hash(&format!("{}|{dir}|{source_hash}", binding_key(b)))[..24].to_string();
    let message = templates.render(direction(dir), &id, body, number(&d["result"], "turn"));
    if message.trim().is_empty() {
        return None;
    }
    // A manual click IS the confirmation the countdown exists to obtain, so it
    // dispatches immediately: deadline already in the past, no cancel window. The
    // automatic path keeps the configured countdown as its cancellable delay.
    let dispatch_delay = if manual { 0 } else { delay.clamp(1, 120) };
    let deadline = if manual { now() } else { 0 };
    Some(
        json!({"message_contract":"configured-plain-v2","id":id,"direction":dir,"fingerprint":fp,"source_hash":source_hash,"source_seq":seq,"source_turn":d["result"]["turn"],"created_ms":now(),"stage":"queued","deadline_ms":deadline,"dispatch_delay_seconds":dispatch_delay,"retry_after_ms":0,"text":message,"message_templates":templates,"manual":manual,"dsh_session":b["dsh_session"],"claude_session":b["claude_session"],"claude_window":b["claude_window"],"dsh_user_seq":d["user_seq"],"dsh_turn":d["turn"],"claude_user_index":c["latest_user_index"],"claude_user_hash":c["latest_user_hash"],"claude_reply_index":c["ui_message_index"],"claude_reply_hash":hash(text(c,"reply_text"))}),
    )
}
/// Next Claude poll deadline in epoch ms. This is derived runtime state: it lives in
/// memory and `runtime/state.json` only, never in `runtime/config.json`.
static NEXT_POLL_MS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

fn next_poll_ms() -> u64 {
    NEXT_POLL_MS.load(std::sync::atomic::Ordering::Relaxed)
}

fn set_next_poll_ms(value: u64) {
    NEXT_POLL_MS.store(value, std::sync::atomic::Ordering::Relaxed);
}

/// Who is the handoff actually waiting on right now?
///
/// `phase` alone cannot answer this. `waiting_dsh` covers two opposite situations -
/// "DSH is still running the round we are waiting for" and "DSH already produced the
/// result and we are waiting for CLAUDE to answer it" - and reporting the first for
/// the second is what made the window claim 「等待 DSH 执行结果」 while Claude was the
/// one actually working. The deciding evidence is the direction of the last verified
/// delivery plus what both sides currently report.
///
/// Returns `None` when nothing is being awaited (the other side's output is already
/// sitting there undelivered), which is a different message again.
fn waiting_on_whom(w: &Value, d: &Value, c: &Value) -> Option<&'static str> {
    let has_delivery = !w["last_delivery"].is_null();
    let delivered_to_claude = text(&w["last_delivery"], "direction") == "DSH_TO_CLAUDE";
    let claude_new_reply =
        number(c, "ui_message_index") > number(&w["last_delivery"], "claude_reply_index");

    // Claude is visibly working, so that is who the handoff is waiting on.
    if yes(c, "ok")
        && matches!(
            text(c, "state"),
            "running" | "awaiting_reply" | "needs_input"
        )
    {
        return Some("Claude");
    }
    if yes(d, "busy") {
        return Some("DSH");
    }
    // The last handover went to Claude, so Claude owes the next move.
    if has_delivery && delivered_to_claude && claude_new_reply {
        return Some("Claude");
    }
    // A completed DSH result that has not been handed over: nothing is awaited.
    if number(&d["result"], "end_seq") > number(w, "dsh_after_seq") {
        return None;
    }
    if has_delivery && delivered_to_claude {
        return Some("Claude");
    }
    Some("DSH")
}

fn publish(
    root: &Path,
    enabled: bool,
    b: &Value,
    w: &Value,
    d: &Value,
    c: &Value,
    error: &str,
    sending: bool,
) -> Result<(), String> {
    let phase = text(w, "phase");
    let p = &w["pending"];
    let (title, detail): (String, String) = if sending {
        (
            if text(p, "stage") == "draft_ready" {
                "正在发送"
            } else {
                "正在填入输入框"
            }
            .into(),
            if yes(p, "manual") {
                // A manual click dispatches as soon as the draft is verified, so
                // promising a countdown here would be false.
                "点击已确认；草稿核验通过后立即发送。".into()
            } else {
                "填入成功后倒计时；最终只提交一次。".into()
            },
        )
    } else if !error.is_empty() {
        (
            "交接暂缓，等待核验".into(),
            error.chars().take(100).collect(),
        )
    } else if !p.is_null() && text(p, "stage") != "draft_ready" {
        (
            "准备填入输入框".into(),
            "等待目标输入框可用；尚未开始倒计时。".into(),
        )
    } else if !p.is_null() {
        (
            format!(
                "{} · {} 秒",
                if text(p, "direction") == "DSH_TO_CLAUDE" {
                    "DSH → Claude"
                } else {
                    "Claude → DSH"
                },
                number(p, "deadline_ms")
                    .saturating_sub(now())
                    .div_ceil(1000)
            ),
            "内容已填入；未取消则自动发送。".into(),
        )
    } else {
        match phase {
            "unclaimed" if yes(d, "busy") => (
                format!("DSH 第 {} 轮执行中", number(d, "turn")),
                "监听继续；本轮完成前不追加新任务。".into(),
            ),
            "unclaimed" => (
                format!("第 {} 轮待回传", number(&d["result"], "turn")),
                "监听已开启，点击发送给 Claude 接管。".into(),
            ),
            "paused_by_user" => (
                "自动交接已暂停 · 监听仍在".into(),
                "检测到人工介入；讨论结束后点击发送按钮重新接管。".into(),
            ),
            "hold_preparation" => ("填入已暂停".into(), text(w, "notice").to_owned()),
            "hold_send_uncertain" => (
                "投递未确认，已停止重试".into(),
                "草稿或提交未核实；已停止，不会盲目重发。".into(),
            ),
            "idle" => (
                "本轮交接已完成".into(),
                "没有新任务，不会循环发送确认消息。".into(),
            ),
            _ if !d["pending_question"].is_null() => {
                ("DSH 等待答题".into(), "不会擅自代答授权问题。".into())
            }
            // The two `waiting_*` phases each cover opposite situations, so they read
            // the live activity instead of announcing the phase name. Saying 「等待 DSH
            // 执行结果」 while Claude is the one working is exactly what this fixes.
            "waiting_claude" | "waiting_dsh" => match waiting_on_whom(w, d, c) {
                Some("Claude") => (
                    "等待 Claude 回复".into(),
                    format!(
                        "第 {} 轮结果已投递；Claude 回复后转交 DSH。",
                        number(&d["result"], "turn")
                    ),
                ),
                Some("DSH") => (
                    "等待 DSH 执行结果".into(),
                    "按原生结束事件监听，不假设相邻轮次。".into(),
                ),
                _ => (
                    "待投递 Claude".into(),
                    "DSH 结果已就绪但尚未投递；点击发送给 Claude 接管。".into(),
                ),
            },
            _ if !enabled => ("自动交接已暂停".into(), "监听继续；手动发送仍可用。".into()),
            _ => (
                "等待 DSH 执行结果".into(),
                "按原生结束事件监听，不假设相邻轮次。".into(),
            ),
        }
    };
    publish_state(
        root,
        &json!({"schema":1,"mode":"live","pid":std::process::id(),"at_ms":now(),"enabled":enabled,"phase":phase,"sending":sending,"status_text":title,"detail":detail,"notice":w["notice"],"dsh_session":b["dsh_session"],"dsh_title":d["title"],"dsh_turn":d["turn"],"dsh_running":d["busy"],"dsh_ok":yes(d,"ok"),"claude_ok":yes(c,"ok"),"claude_state":c["state"],"reply_turn":d["result"]["turn"],"goal":d["goal"],"last_delivery":w["last_delivery"],"next_poll_at_ms":next_poll_ms(),"pending":if p.is_null(){Value::Null}else{json!({"id":p["id"],"direction":p["direction"],"stage":p["stage"],"deadline_ms":p["deadline_ms"]})}}),
    );
    Ok(())
}
fn commit_receipt(w: &mut Value, p: &Value, r: &Value) {
    record(w, p, text(r, "state"));
    w["pending"] = Value::Null;
    // `claude_reply_index` is recorded so the status text can tell "Claude already
    // answered this handover" from "Claude still owes an answer".
    w["last_delivery"] = json!({"id":p["id"],"direction":p["direction"],"state":r["state"],"source_turn":p["source_turn"],"claude_reply_index":p["claude_reply_index"],"at_ms":now()});
    if text(r, "state") != "sent" {
        w["phase"] = json!("hold_send_uncertain");
        return;
    }
    let a = &r["anchor"];
    if text(p, "direction") == "DSH_TO_CLAUDE" {
        w["dsh_after_seq"] = p["source_seq"].clone();
        w["claude_anchor_index"] = a["claude_user_index"].clone();
        w["claude_anchor_hash"] = a["claude_user_hash"].clone();
        w["claude_floor"] = a["claude_user_index"].clone();
        w["phase"] = json!("waiting_claude");
    } else {
        w["dsh_user_seq"] = a["dsh_user_seq"].clone();
        w["dsh_after_seq"] = a["dsh_user_seq"].clone();
        w["dsh_answer_seq"] = a["dsh_answer_seq"].clone();
        w["claude_floor"] = p["source_seq"].clone();
        w["phase"] = json!("waiting_dsh");
    }
    w["notice"] = json!("已核验发送回执；等待对方新回复");
}
fn recover_delayed_submit(root: &Path, owner: u32, w: &mut Value) -> Result<bool, String> {
    if text(w, "phase") != "hold_send_uncertain"
        || text(&w["last_delivery"], "state") != "submit_uncertain"
        || text(&w["last_delivery"], "direction") != "DSH_TO_CLAUDE"
    {
        return Ok(false);
    }
    let id = text(&w["last_delivery"], "id").to_owned();
    if id.is_empty() {
        return Ok(false);
    }
    let home = root.join("runtime");
    let request_path = home.join(format!("requests/{id}.json"));
    let receipt_path = home.join(format!("receipts/{id}.json"));
    let Some(p) = read(&request_path) else {
        return Ok(false);
    };
    let Some(mut receipt) = read(&receipt_path) else {
        return Ok(false);
    };
    if text(&receipt, "state") != "submit_uncertain" {
        return Ok(false);
    }
    let result = run(
        root,
        owner,
        Command::new("powershell.exe")
            .args(["-NoProfile", "-STA", "-ExecutionPolicy", "Bypass", "-File"])
            .arg(root.join("adapters/windows/recover-uncertain.ps1"))
            .arg("-ProductRoot")
            .arg(root)
            .arg("-RequestFile")
            .arg(&request_path),
    )
    .unwrap_or_else(|e| json!({"ok":false,"error":e}));
    if !yes(&result, "ok") || text(&result, "state") != "sent" {
        return Ok(false);
    }
    receipt["state"] = json!("sent");
    receipt["anchor"] = result["anchor"].clone();
    receipt["recovered_at_ms"] = json!(now());
    receipt["recovery_evidence"] = result["anchor"]["evidence"].clone();
    write(&receipt_path, &receipt)?;
    commit_receipt(w, &p, &receipt);
    diagnostic(
        &home,
        "delayed_submit_recovered",
        &json!({"id":id,"anchor":receipt["anchor"],"messages_sent":0}),
    );
    Ok(true)
}

fn clear_recovered_observation_error(error: &mut String, d: &Value, c: &Value) {
    // Preserve all delivery/ownership holds; clear only a recovered read failure.
    if yes(d, "ok") && yes(c, "ok") && error == "会话尚不可读取，请打开绑定窗口。" {
        error.clear();
    }
}

fn valid_bindings(b: &Value) -> bool {
    [("claude_session", "cse_"), ("dsh_session", "session-")]
        .iter()
        .all(|(k, p)| {
            text(b, k).starts_with(p)
                && text(b, k).len() < 150
                && text(b, k)
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
        })
        && !text(b, "claude_window").is_empty()
}
fn entry() -> Result<(), String> {
    let args: Vec<String> = std::env::args().collect();
    let root = PathBuf::from(
        args.windows(2)
            .find(|a| a[0] == "--product-root")
            .map(|a| a[1].as_str())
            .unwrap_or("."),
    );
    let owner = args
        .windows(2)
        .find(|a| a[0] == "--owner-pid")
        .and_then(|a| a[1].parse::<u32>().ok())
        .unwrap_or(0);
    let home = root.join("runtime");
    for p in ["ipc", "commands", "requests", "receipts"] {
        fs::create_dir_all(home.join(p)).map_err(|e| e.to_string())?;
    }
    let mut b = read(&home.join("bindings.json")).ok_or("bindings unavailable")?;
    if !valid_bindings(&b) {
        return Err("invalid bindings".into());
    }
    if args.iter().any(|a| a == "--init-message-templates") {
        let path = home.join("message-templates.json");
        if !path.exists() {
            write(&path, &json!(MessageTemplates::default()))?;
        }
        load_message_templates(&home)?;
        println!("{}", json!({"message_templates":"ready","messages_sent":0}));
        return Ok(());
    }
    if args.iter().any(|a| a == "--probe") {
        let cfg = handoff_core::config::load(&root).config;
        println!(
            "{}",
            json!({"dsh":observe_dsh(&root,0),"claude":observe_claude(&root,0,&b,&cfg),"messages_sent":0})
        );
        return Ok(());
    }
    let _lock = acquire(&home)?;
    let mut w = read(&home.join("workflow.json"))
        .filter(|w| text(w, "binding_key") == binding_key(&b) && w["ledger"].is_array())
        .unwrap_or_else(|| blank(&b));
    if !w["pending"].is_null() {
        let p = w["pending"].clone();
        if let Some(r) = read(&home.join(format!("receipts/{}.json", text(&p, "id")))) {
            if text(&r, "flow") == "draft-first-v1" && text(&r, "state") == "draft_ready" {
                w["pending"]["stage"] = json!("queued");
                w["pending"]["deadline_ms"] = json!(0);
            } else {
                commit_receipt(&mut w, &p, &r);
            }
        } else {
            w["pending"]["stage"] = json!("queued");
            w["pending"]["deadline_ms"] = json!(0);
        }
    }
    let mut d = json!({"ok":false});
    let mut c = d.clone();
    let (mut nd, mut nc, mut np) = (0, 0, 0);
    let mut saved = String::new();
    let mut error = String::new();
    diagnostic(&home, "runtime_started", &json!({"owner_pid":owner}));
    loop {
        if owner != 0 && !alive(owner) {
            break;
        }
        // The typed config is the single schema source. `nc` is runtime-derived and
        // lives only in memory + state.json; it is never written back to config.json.
        let cfg = handoff_core::config::load(&root).config;
        let templates = match load_message_templates(&home) {
            Ok(t) => t,
            Err(e) => {
                publish(
                    &root,
                    cfg.enabled,
                    &b,
                    &w,
                    &d,
                    &c,
                    &format!("交接文案配置错误，未发送：{e}"),
                    false,
                )?;
                thread::sleep(Duration::from_millis(500));
                continue;
            }
        };
        if let Some(new) = read(&home.join("bindings.json")) {
            if new != b {
                b = new;
                w = blank(&b);
                nd = 0;
                nc = 0;
                error.clear();
                diagnostic(&home, "bindings_changed", &b);
            }
        }
        if !valid_bindings(&b) {
            return Err("invalid bindings; stopped safely".into());
        }
        if now() >= nd {
            d = observe_dsh(&root, owner);
            nd = now() + 2000;
        }
        if now() >= nc {
            c = observe_claude(&root, owner, &b, &cfg);
            nc = now() + cfg.poll_seconds.clamp(1, 7200) * 1000;
            set_next_poll_ms(nc);
        }
        clear_recovered_observation_error(&mut error, &d, &c);
        let mut commands: Vec<_> = fs::read_dir(home.join("commands"))
            .map_err(|e| e.to_string())?
            .filter_map(Result::ok)
            .map(|e| e.path())
            .filter(|p| p.extension().is_some_and(|x| x == "json"))
            .collect();
        commands.sort();
        for path in commands {
            let Some(cmd) = read(&path) else {
                let _ = fs::remove_file(&path);
                continue;
            };
            let _ = fs::remove_file(&path);
            if cmd["bindings"] != b {
                error = "绑定已变化，请重新点击。".into();
                continue;
            }
            match text(&cmd, "command") {
                "restore_listener" => {
                    d = observe_dsh(&root, owner);
                    c = observe_claude(&root, owner, &b, &cfg);
                    if !yes(&d, "ok") || !yes(&c, "ok") {
                        error = "当前会话暂不可读取，恢复监听未执行。".into();
                    } else if !w["pending"].is_null() {
                        error = "当前有交接正在处理，请先取消本次再恢复监听。".into();
                    } else {
                        restore_listener(&mut w, &d, &c);
                        error.clear();
                        diagnostic(
                            &home,
                            "listener_restored",
                            &json!({"dsh_turn":d["turn"],"dsh_busy":d["busy"],"dsh_after_seq":w["dsh_after_seq"]}),
                        );
                    }
                    nd = 0;
                    nc = 0;
                }
                "toggle" => {
                    // Flip through the typed model and save the whole schema, so a
                    // legacy file loses its deprecated keys the first time it changes.
                    let mut next = cfg.clone();
                    next.enabled = !cfg.enabled;
                    if !next.enabled {
                        staged_delivery::cancel(
                            &home,
                            &mut w,
                            "自动已暂停；草稿保留，不再自动发送",
                        )?;
                    }
                    handoff_core::config::save(&root, &next)?;
                    error.clear();
                }
                "poll_now" => {
                    // Observation only: re-read both sessions and reset the poll
                    // schedule. Unlike `restore_listener` this must not touch the
                    // workflow baseline, pending handoff, or phase.
                    d = observe_dsh(&root, owner);
                    c = observe_claude(&root, owner, &b, &cfg);
                    if !yes(&d, "ok") || !yes(&c, "ok") {
                        error = "立即轮询：目标会话暂不可读取，请确认窗口已打开。".into();
                    } else {
                        error.clear();
                        diagnostic(
                            &home,
                            "poll_now",
                            &json!({"dsh_turn":d["turn"],"dsh_busy":d["busy"],"claude_ok":c["ok"]}),
                        );
                    }
                    nd = 0;
                    nc = 0;
                    set_next_poll_ms(0);
                }
                "cancel" => {
                    staged_delivery::cancel(
                        &home,
                        &mut w,
                        "本次已取消；草稿保留，不会自动重新排队",
                    )?;
                    error.clear();
                }
                "send_claude" | "send_dsh" => {
                    if !w["pending"].is_null() {
                        // Auto handoff already armed this draft and is counting down.
                        // The click is the user confirming, so dispatch it now instead
                        // of telling them to wait for a timer they just pre-empted.
                        if !yes(&w["pending"], "manual") {
                            let mut p = w["pending"].clone();
                            override_with_manual_send(&mut p);
                            w["pending"] = p;
                            w["notice"] = json!("已按点击立即发送，跳过倒计时");
                        } else {
                            w["notice"] = json!("本次已经排队，无需重复点击");
                        }
                        continue;
                    }
                    d = observe_dsh(&root, owner);
                    c = observe_claude(&root, owner, &b, &cfg);
                    if !yes(&d, "ok") || !yes(&c, "ok") {
                        error = "会话尚不可读取，请打开绑定窗口。".into();
                        continue;
                    }
                    let dir = if text(&cmd, "command") == "send_claude" {
                        "DSH_TO_CLAUDE"
                    } else {
                        "CLAUDE_TO_DSH"
                    };
                    if dir == "CLAUDE_TO_DSH" && yes(&d, "busy") {
                        error = "DSH 仍在执行，未追加任务。".into();
                        continue;
                    }
                    adopt(&mut w, &d, &c, dir);
                    // Recorded so a refusal can name the phase the planner actually saw,
                    // rather than the phase left behind after the failure branch ran.
                    let phase_before = text(&w, "phase").to_string();
                    // The click means "send THIS reply".
                    //
                    // `send_claude` (DSH -> Claude): adopt() moves `claude_floor` onto the
                    // current last message, and candidate() only looks strictly past it,
                    // so the floor must sit one behind or the clicked round is excluded.
                    //
                    // `send_dsh` (Claude -> DSH): adopt() already sets `claude_floor` to 0,
                    // which is what makes the Claude reply eligible. The DSH side must NOT
                    // be re-opened here: lowering `dsh_after_seq` re-offers a DSH result
                    // that was already delivered, the planner then proposes DSH -> Claude,
                    // and the direction guard turns the click into a silent no-op.
                    let branch_taken = if dir == "DSH_TO_CLAUDE" {
                        w["claude_floor"] = number(&c, "ui_message_index").saturating_sub(1).into();
                        "claude_floor"
                    } else {
                        "claude_floor_from_adopt"
                    };
                    // The click names the side it wants; the planner is only consulted on
                    // the automatic path. See `candidate_for`.
                    match candidate_for(&templates, &w, &b, &d, &c, 0, true, Some(dir)) {
                        Some(p) if text(&p, "direction") == dir => {
                            w["pending"] = p;
                            error.clear();
                        }
                        _ => {
                            w["phase"] = json!("unclaimed");
                            error = format!(
                                "没有新的完整回复，或该回复已经投递过。（原因：{} / dir={} / was={}）",
                                last_plan_reason(),
                                dir,
                                phase_before
                            );
                            // One record with every value the decision used, so a refusal
                            // can be diagnosed from the log instead of by inference.
                            diagnostic(
                                &home,
                                "send_refused",
                                &json!({
                                    "command":text(&cmd, "command"),
                                    "cmd_raw":cmd,
                                    "dir":dir,
                                    "branch_taken":branch_taken,
                                    "floor_after_adopt":number(&w, "claude_floor"),
                                    "phase_before_adopt":phase_before,
                                    "phase_after_adopt":text(&w, "phase"),
                                    "reason":last_plan_reason(),
                                    "verdict_raw":last_plan_verdict(),
                                    "claude_floor":number(&w, "claude_floor"),
                                    "claude_ui_index":number(&c, "ui_message_index"),
                                    "claude_latest_user_index":number(&c, "latest_user_index"),
                                    "claude_anchor_index":number(&w, "claude_anchor_index"),
                                    "claude_reply_available":yes(&c, "reply_available"),
                                    "claude_reply_text_len":text(&c, "reply_text").chars().count(),
                                    "dsh_after_seq":number(&w, "dsh_after_seq"),
                                    "dsh_result_end_seq":number(&d["result"], "end_seq"),
                                    "dsh_busy":yes(&d, "busy"),
                                    "dsh_result_present":!d["result"].is_null(),
                                }),
                            );
                        }
                    }
                }
                _ => {
                    error = "未知操作，未发送。".into();
                }
            }
        }
        if text(&w, "phase") == "hold_send_uncertain" {
            if recover_delayed_submit(&root, owner, &mut w)? {
                error.clear();
                nd = 0;
                nc = 0;
                d = observe_dsh(&root, owner);
                c = observe_claude(&root, owner, &b, &cfg);
            }
        }
        if text(&w, "phase") == "hold_send_uncertain" && manual_changed(&w, &d, &c) {
            w["last_delivery"]["previous_state"] = w["last_delivery"]["state"].clone();
            w["last_delivery"]["state"] = json!("superseded_by_user");
            w["phase"] = json!("paused_by_user");
            w["pending"] = Value::Null;
            w["notice"] = json!("会话已由人工推进；旧稿不再回发，原始回执保留");
            error.clear();
            diagnostic(
                &home,
                "old_draft_superseded",
                &json!({"id":w["last_delivery"]["id"],"dsh_user_seq":d["user_seq"],"claude_user_index":c["latest_user_index"]}),
            );
        }
        if matches!(text(&w, "phase"), "waiting_dsh" | "waiting_claude")
            && manual_changed(&w, &d, &c)
        {
            w["phase"] = json!("paused_by_user");
            w["pending"] = Value::Null;
            error.clear();
            diagnostic(
                &home,
                "manual_intervention",
                &json!({"dsh_user_seq":d["user_seq"],"claude_user_index":c["latest_user_index"]}),
            );
        }
        if text(&w, "phase") == "waiting_claude"
            && yes(&c, "reply_available")
            && number(&c, "ui_message_index") > number(&w, "claude_floor")
            && text(&c, "reply_text")
                .lines()
                .any(|l| l.trim() == "A2A_DONE")
            && number(&d["result"], "end_seq") <= number(&w, "dsh_after_seq")
        {
            w["phase"] = json!("idle");
            w["pending"] = Value::Null;
        }
        if cfg.enabled && w["pending"].is_null() {
            if let Some(p) = candidate(
                &templates,
                &w,
                &b,
                &d,
                &c,
                cfg.dispatch_delay_seconds.clamp(1, 120),
                false,
            ) {
                w["pending"] = p;
                error.clear();
            }
        }
        let before = (w["pending"].clone(), w["last_delivery"].clone());
        staged_delivery::drive(&root, owner, &cfg, &b, &mut w, &mut d, &mut c, &mut error)?;
        if before != (w["pending"].clone(), w["last_delivery"].clone()) {
            nd = 0;
            nc = 0;
        }
        let s = w.to_string();
        if s != saved {
            write(&home.join("workflow.json"), &w)?;
            saved = s;
        }
        if now() >= np {
            let e = if !yes(&d, "ok") {
                format!("DSH 读取失败：{}", text(&d, "error"))
            } else if !yes(&c, "ok") {
                format!("Claude 读取失败：{}", text(&c, "error"))
            } else {
                error.clone()
            };
            publish(&root, cfg.enabled, &b, &w, &d, &c, &e, false)?;
            np = now() + 500;
        }
        thread::sleep(Duration::from_millis(200));
    }
    diagnostic(&home, "runtime_stopped", &json!({"dsh_untouched":true}));
    Ok(())
}
fn main() {
    if let Err(e) = entry() {
        eprintln!("A2AHandoff runtime: {e}");
        std::process::exit(1)
    }
}

#[cfg(test)]
mod live_tests {
    use super::*;
    fn fixture() -> (Value, Value, Value, Value) {
        let b = json!({"claude_session":"cse_test","claude_window":"Test - Claude","dsh_session":"session-test"});
        let d = json!({"ok":true,"turn":7,"busy":false,"user_seq":301,"last_question_answer_seq":279,"result":{"turn":7,"end_seq":988,"reply":"complete report 7"}});
        let c = json!({"ok":true,"latest_user_index":5,"latest_user_hash":"human-hash","ui_message_index":6,"reply_available":true,"reply_text":"previous instruction"});
        let w = blank(&b);
        (b, d, c, w)
    }
    #[test]
    fn fresh_install_never_auto_sends_history() {
        let (b, d, c, w) = fixture();
        assert!(candidate(&MessageTemplates::default(), &w, &b, &d, &c, 10, false).is_none());
    }
    #[test]
    fn manual_send_dispatches_without_the_automatic_countdown() {
        let (b, d, c, mut w) = fixture();
        adopt(&mut w, &d, &c, "DSH_TO_CLAUDE");
        // A manual click already IS the confirmation, so it must not wait out
        // `dispatch_delay_seconds`: no delay, and a deadline already in the past.
        let manual = candidate(&MessageTemplates::default(), &w, &b, &d, &c, 30, true).unwrap();
        assert_eq!(manual["dispatch_delay_seconds"], 0);
        assert!(number(&manual, "deadline_ms") > 0);
        assert!(number(&manual, "deadline_ms") <= now());
        // The automatic path keeps the configured countdown as its cancellable delay.
        let automatic = candidate(&MessageTemplates::default(), &w, &b, &d, &c, 30, false).unwrap();
        assert_eq!(automatic["dispatch_delay_seconds"], 30);
        assert_eq!(automatic["deadline_ms"], 0);
    }
    #[test]
    fn manual_click_overrides_an_armed_automatic_countdown() {
        // Auto handoff armed this draft and started the cancellable countdown.
        let mut p = json!({"id":"abc","stage":"draft_ready","manual":false,
            "dispatch_delay_seconds":30,"deadline_ms":now()+30000,"text":"body"});
        let before = p.clone();
        override_with_manual_send(&mut p);
        assert!(yes(&p, "manual"));
        assert_eq!(p["dispatch_delay_seconds"], 0);
        assert!(number(&p, "deadline_ms") <= now());
        // Same draft, same delivery identity: the click only removes the wait.
        assert_eq!(p["id"], before["id"]);
        assert_eq!(p["text"], before["text"]);
        assert_eq!(p["stage"], before["stage"]);
    }
    #[test]
    fn a_cancelled_handoff_does_not_consume_the_reply() {
        // The draft was written, the send was abandoned. Nothing reached the peer, so
        // the reply must stay selectable - otherwise a cancellation silently burns the
        // reply and every later click reports "已经投递过".
        let (b, d, c, mut w) = fixture();
        adopt(&mut w, &d, &c, "CLAUDE_TO_DSH");
        let p = candidate(&MessageTemplates::default(), &w, &b, &d, &c, 0, true).unwrap();

        record(&mut w, &p, "cancelled");
        assert!(!consumed(
            &w,
            text(&p, "direction"),
            text(&p, "source_hash")
        ));
        let again = candidate(&MessageTemplates::default(), &w, &b, &d, &c, 0, true);
        assert!(again.is_some(), "a cancelled reply must stay selectable");

        // A verified delivery, by contrast, does consume it - the delivered reply
        // must never be selectable again. The fixture can still offer the other
        // direction, so assert on this reply's identity rather than on emptiness.
        record(&mut w, &p, "sent");
        assert!(consumed(&w, text(&p, "direction"), text(&p, "source_hash")));
        let after_sent = candidate(&MessageTemplates::default(), &w, &b, &d, &c, 0, true);
        assert!(
            after_sent
                .as_ref()
                .is_none_or(|x| text(x, "source_hash") != text(&p, "source_hash")),
            "the delivered reply must not be selectable again"
        );
    }
    #[test]
    fn status_text_follows_the_side_that_is_actually_working() {
        // The real case: DSH had already finished round 21, the result was handed to
        // Claude, and Claude was the one working - yet the window said
        // 「等待 DSH 执行结果」 because the phase happened to be `waiting_dsh`.
        let (_, d, mut c, w) = fixture();
        c["state"] = json!("running");
        assert_eq!(waiting_on_whom(&w, &d, &c), Some("Claude"));

        // Claude visibly working outranks DSH being busy.
        let mut busy = d.clone();
        busy["busy"] = json!(true);
        assert_eq!(waiting_on_whom(&w, &busy, &c), Some("Claude"));

        let idle_claude = json!({"ok":true,"state":"replied","ui_message_index":6});
        let idle_dsh = json!({"ok":true,"busy":false,"result":{"end_seq":988}});
        // A coherent state: DSH's result at end_seq 988 has already been handed over, so
        // the watermark covers it. Without this the fixture would claim a handover and
        // an undelivered result at the same time.
        let delivered = |w: &Value| {
            let mut w = w.clone();
            w["dsh_after_seq"] = json!(988);
            w
        };

        // A delivered round whose next move belongs to DSH.
        let mut w2 = delivered(&w);
        w2["last_delivery"] = json!({"direction":"CLAUDE_TO_DSH","claude_reply_index":6});
        let busy_dsh = json!({"ok":true,"busy":true,"result":{"end_seq":988}});
        assert_eq!(waiting_on_whom(&w2, &busy_dsh, &idle_claude), Some("DSH"));

        // The last handover went to Claude and Claude has not answered it yet.
        let mut w3 = delivered(&w);
        w3["last_delivery"] = json!({"direction":"DSH_TO_CLAUDE","claude_reply_index":6});
        let replied_claude = json!({"ok":true,"state":"replied","ui_message_index":7});
        assert_eq!(
            waiting_on_whom(&w3, &idle_dsh, &replied_claude),
            Some("Claude")
        );

        // Nothing is awaited: DSH's result is complete and still undelivered.
        let mut w4 = w.clone();
        w4["last_delivery"] = json!({"direction":"CLAUDE_TO_DSH","claude_reply_index":6});
        let undelivered = json!({"ok":true,"busy":false,"result":{"end_seq":1000}});
        assert_eq!(waiting_on_whom(&w4, &undelivered, &idle_claude), None);

        // Claude answered what was handed over. The next move is still Claude's: it
        // owes the reply that DSH's answer will be based on. Text must NOT claim DSH.
        let mut w5 = delivered(&w);
        w5["last_delivery"] = json!({"direction":"DSH_TO_CLAUDE","claude_reply_index":6});
        assert_eq!(
            waiting_on_whom(&w5, &idle_dsh, &idle_claude),
            Some("Claude")
        );

        // Claude not running and the last handover went to DSH -> DSH owes the work.
        let mut w6 = delivered(&w);
        w6["last_delivery"] = json!({"direction":"CLAUDE_TO_DSH","claude_reply_index":6});
        assert_eq!(waiting_on_whom(&w6, &idle_dsh, &idle_claude), Some("DSH"));

        // An unreadable Claude must never be reported as the one being awaited.
        let broken_claude = json!({"ok":false,"state":"","ui_message_index":0});
        assert_eq!(waiting_on_whom(&w6, &idle_dsh, &broken_claude), Some("DSH"));
    }
    #[test]
    fn a_manual_click_sends_content_that_was_already_delivered_once() {
        // `consumed` and `abandoned` are automatic-handoff policies: they stop polling
        // from re-delivering the same content. A manual click is the user looking at the
        // reply and saying "hand this over now", so it must override them. Without this
        // the reply Claude had just produced could never be forwarded - the click
        // silently did nothing and the handoff appeared stuck forever.
        let (b, mut d, mut c, mut w) = fixture();
        // Shape the observation BEFORE adopting: adopt() derives the anchor from these,
        // so setting them afterwards would leave a stale anchor and fail correlation.
        // The live observer reports the same index for the latest user message and the
        // reply, so align them the way the adapter does.
        c["ui_message_index"] = json!(8);
        c["latest_user_index"] = json!(7);
        adopt(&mut w, &d, &c, "CLAUDE_TO_DSH");
        // The DSH side must hold nothing new, or plan_one proposes DSH -> Claude first and
        // the Claude reply is never reached. That ordering is exactly what made a real
        // click do nothing, so consume it here the way a completed handoff would.
        w["dsh_after_seq"] = d["result"]["end_seq"].clone();
        w["ledger"] = json!([{
            "direction":"DSH_TO_CLAUDE",
            "fingerprint": fingerprint("complete report 7"),
            "source_hash":"dsh-report-7",
            "state":"sent"
        }]);
        // Both sides fingerprint the same string, so correlation holds: adopt() stored
        // the observer's latest_user_hash and candidate() fingerprints it again.
        w["claude_anchor_hash"] = c["latest_user_hash"].clone();

        let click = |w: &Value| {
            candidate_for(
                &MessageTemplates::default(),
                w,
                &b,
                &d,
                &c,
                0,
                true,
                Some("CLAUDE_TO_DSH"),
            )
        };
        let first = match click(&w) {
            Some(p) => p,
            None => panic!("first click produced nothing: {}", last_plan_reason()),
        };
        assert_eq!(first["direction"], "CLAUDE_TO_DSH");
        record(&mut w, &first, "sent");

        // Automatic handoff must not send it again.
        assert!(candidate(&MessageTemplates::default(), &w, &b, &d, &c, 0, false).is_none());
        // A deliberate click still can.
        let manual = click(&w);
        assert!(
            manual.is_some(),
            "a manual click must override already-delivered dedup"
        );
        assert_eq!(manual.unwrap()["direction"], "CLAUDE_TO_DSH");

        // And the same holds after the user cancelled it earlier.
        let mut w2 = w.clone();
        w2["ledger"] = json!([{
            "direction":"CLAUDE_TO_DSH",
            "fingerprint":first["fingerprint"],
            "source_hash":first["source_hash"],
            "state":"cancelled"
        }]);
        assert!(candidate(&MessageTemplates::default(), &w2, &b, &d, &c, 0, false).is_none());
        assert!(click(&w2).is_some());
        let _ = &mut d;
    }
    #[test]
    fn each_send_button_delivers_its_own_side() {
        // A click names its side. `plan_one` is a PRIORITY function, so with a DSH result
        // pending it always proposes DSH -> Claude; a click on the other button therefore
        // has to bypass that ordering, or it is refused while the reply it asked for sits
        // right there. This pins both buttons against the SAME state, so a regression that
        // lets one direction shadow the other fails here.
        let (b, d, mut c, mut w) = fixture();
        // Shape the observation before adopting: adopt() derives the anchor from it.
        c["ui_message_index"] = json!(8);
        c["latest_user_index"] = json!(7);
        adopt(&mut w, &d, &c, "CLAUDE_TO_DSH");
        w["claude_anchor_hash"] = c["latest_user_hash"].clone();
        // Leave the DSH side genuinely pending, which is what tempts the planner to pick
        // it instead of the side the user clicked.
        w["dsh_after_seq"] = number(&d["result"], "end_seq").saturating_sub(1).into();

        let to_dsh = candidate_for(
            &MessageTemplates::default(),
            &w,
            &b,
            &d,
            &c,
            0,
            true,
            Some("CLAUDE_TO_DSH"),
        )
        .expect("发送给 DSH must produce a handoff even while a DSH result is pending");
        assert_eq!(to_dsh["direction"], "CLAUDE_TO_DSH");

        let to_claude = candidate_for(
            &MessageTemplates::default(),
            &w,
            &b,
            &d,
            &c,
            0,
            true,
            Some("DSH_TO_CLAUDE"),
        )
        .expect("发送给 Claude must produce a handoff from the same state");
        assert_eq!(to_claude["direction"], "DSH_TO_CLAUDE");

        // And a manual handoff carries no countdown: the click IS the confirmation.
        assert_eq!(to_dsh["dispatch_delay_seconds"], 0);
        assert_eq!(to_claude["dispatch_delay_seconds"], 0);
    }
    #[test]
    fn manual_adoption_selects_real_seventh_result() {
        let (b, d, c, mut w) = fixture();
        adopt(&mut w, &d, &c, "DSH_TO_CLAUDE");
        let p = candidate(&MessageTemplates::default(), &w, &b, &d, &c, 0, true).unwrap();
        assert_eq!(p["source_seq"], 988);
        assert_eq!(p["source_turn"], 7);
        assert_eq!(p["direction"], "DSH_TO_CLAUDE");
    }
    #[test]
    fn later_internal_round_does_not_hide_finished_report() {
        let (b, mut d, c, mut w) = fixture();
        adopt(&mut w, &d, &c, "DSH_TO_CLAUDE");
        d["turn"] = json!(9);
        d["busy"] = json!(true);
        assert_eq!(
            candidate(&MessageTemplates::default(), &w, &b, &d, &c, 10, false).unwrap()
                ["source_turn"],
            7
        );
    }
    #[test]
    fn restore_running_turn_attaches_without_resending() {
        let (_, mut d, c, mut w) = fixture();
        d["busy"] = json!(true);
        d["open_seq"] = json!(900);
        d["last_seq"] = json!(905);
        d["turn"] = json!(8);
        restore_listener(&mut w, &d, &c);
        assert_eq!(w["phase"], "waiting_dsh");
        assert_eq!(w["dsh_after_seq"], 899);
        assert!(text(&w, "notice").contains("第 8 轮"));
        assert!(w["pending"].is_null());
    }
    #[test]
    fn restore_idle_waits_next_turn_and_skips_old_result() {
        let (b, mut d, c, mut w) = fixture();
        d["busy"] = json!(false);
        d["last_seq"] = json!(1000);
        restore_listener(&mut w, &d, &c);
        assert_eq!(w["dsh_after_seq"], 1000);
        assert!(candidate(&MessageTemplates::default(), &w, &b, &d, &c, 10, false).is_none());
        assert!(text(&w, "notice").contains("等待第 8 轮"));
    }
    #[test]
    fn changed_human_input_pauses() {
        let (_, mut d, c, mut w) = fixture();
        adopt(&mut w, &d, &c, "DSH_TO_CLAUDE");
        d["user_seq"] = json!(1001);
        assert!(manual_changed(&w, &d, &c));
    }
    #[test]
    fn cancelled_reply_does_not_requeue_after_restart() {
        let (b, d, c, mut w) = fixture();
        adopt(&mut w, &d, &c, "DSH_TO_CLAUDE");
        let p = candidate(&MessageTemplates::default(), &w, &b, &d, &c, 10, false).unwrap();
        record(&mut w, &p, "cancelled");
        let restored = serde_json::from_str(&w.to_string()).unwrap();
        assert!(candidate(
            &MessageTemplates::default(),
            &restored,
            &b,
            &d,
            &c,
            10,
            false
        )
        .is_none());
    }
    #[test]
    fn receipt_moves_to_correlated_claude_wait() {
        let (b, d, c, mut w) = fixture();
        adopt(&mut w, &d, &c, "DSH_TO_CLAUDE");
        let p = candidate(&MessageTemplates::default(), &w, &b, &d, &c, 10, false).unwrap();
        commit_receipt(
            &mut w,
            &p,
            &json!({"state":"sent","anchor":{"claude_user_index":7,"claude_user_hash":"tool-hash"}}),
        );
        assert_eq!(w["phase"], "waiting_claude");
        assert_eq!(w["dsh_after_seq"], 988);
        assert_eq!(w["claude_anchor_index"], 7);
        assert!(candidate(&MessageTemplates::default(), &w, &b, &d, &c, 10, false).is_none());
    }
    #[test]
    fn uncertain_attempt_is_not_retried() {
        let (b, d, c, mut w) = fixture();
        adopt(&mut w, &d, &c, "DSH_TO_CLAUDE");
        let p = candidate(&MessageTemplates::default(), &w, &b, &d, &c, 0, true).unwrap();
        commit_receipt(&mut w, &p, &json!({"state":"submit_uncertain"}));
        assert_eq!(w["phase"], "hold_send_uncertain");
        assert!(candidate(&MessageTemplates::default(), &w, &b, &d, &c, 10, false).is_none());
    }
    #[test]
    fn changed_binding_has_a_different_delivery_identity() {
        let (mut b, d, c, mut w) = fixture();
        adopt(&mut w, &d, &c, "DSH_TO_CLAUDE");
        let a = candidate(&MessageTemplates::default(), &w, &b, &d, &c, 0, true).unwrap();
        b["claude_session"] = json!("cse_other");
        let z = candidate(&MessageTemplates::default(), &w, &b, &d, &c, 0, true).unwrap();
        assert_ne!(a["id"], z["id"]);
    }

    #[test]
    fn changing_affixes_never_changes_delivery_identity_or_resends_a_consumed_reply() {
        let (b, d, c, mut w) = fixture();
        adopt(&mut w, &d, &c, "DSH_TO_CLAUDE");
        let a = candidate(&MessageTemplates::default(), &w, &b, &d, &c, 0, true).unwrap();
        let mut t = MessageTemplates::default();
        t.to_claude.prefix = "我的前置说明".into();
        t.to_claude.suffix = "我的后置说明".into();
        let z = candidate(&t, &w, &b, &d, &c, 0, true).unwrap();
        assert_eq!(a["id"], z["id"]);
        assert_eq!(a["source_hash"], z["source_hash"]);
        assert_ne!(a["text"], z["text"]);
        assert!(text(&z, "text").contains("我的后置说明"));
        record(&mut w, &a, "sent");
        assert!(candidate(&t, &w, &b, &d, &c, 0, true).is_none());
    }
    #[test]
    fn dsh_instruction_is_independent_of_claude_reply_but_dedup_is_not() {
        let (b, d, mut c, mut w) = fixture();
        adopt(&mut w, &d, &c, "CLAUDE_TO_DSH");
        w["dsh_after_seq"] = d["result"]["end_seq"].clone();
        let mut t = MessageTemplates::default();
        t.to_dsh.prefix = "读取 docs/NEXT_TASK.md 并执行。".into();
        let a = candidate(&t, &w, &b, &d, &c, 0, true).unwrap();
        assert_eq!(a["text"], "读取 docs/NEXT_TASK.md 并执行。");
        assert_eq!(a["message_contract"], "configured-plain-v2");
        assert!(!text(&a, "text").contains(text(&c, "reply_text")));
        record(&mut w, &a, "sent");
        assert!(candidate(&t, &w, &b, &d, &c, 0, true).is_none());
        c["reply_text"] = json!("A different completed Claude reply");
        c["ui_message_index"] = json!(8);
        let z = candidate(&t, &w, &b, &d, &c, 0, true).unwrap();
        assert_eq!(a["text"], z["text"]);
        assert_ne!(a["id"], z["id"]);
    }
    #[test]
    fn empty_dsh_instruction_is_not_replaced_with_claude_reply() {
        let (b, d, c, mut w) = fixture();
        adopt(&mut w, &d, &c, "CLAUDE_TO_DSH");
        w["dsh_after_seq"] = d["result"]["end_seq"].clone();
        let mut t = MessageTemplates::default();
        t.to_dsh.prefix.clear();
        t.to_dsh.suffix.clear();
        assert!(candidate(&t, &w, &b, &d, &c, 0, true).is_none());
    }
    #[test]
    fn recovered_observer_clears_only_stale_read_error() {
        let mut e = "会话尚不可读取，请打开绑定窗口。".to_string();
        clear_recovered_observation_error(&mut e, &json!({"ok":true}), &json!({"ok":true}));
        assert!(e.is_empty());
    }
    #[test]
    fn incomplete_observer_recovery_keeps_read_error() {
        let original = "会话尚不可读取，请打开绑定窗口。".to_string();
        let mut e = original.clone();
        clear_recovered_observation_error(&mut e, &json!({"ok":true}), &json!({"ok":false}));
        assert_eq!(e, original);
    }
    #[test]
    fn observer_recovery_never_clears_delivery_hold() {
        let mut e = "发送结果未确认，禁止重试".to_string();
        let original = e.clone();
        clear_recovered_observation_error(&mut e, &json!({"ok":true}), &json!({"ok":true}));
        assert_eq!(e, original);
    }

    /// A locked status file must not stop monitoring.
    ///
    /// `state.json` is written by temp file + rename, and Windows fails that rename with
    /// "access denied" while anything holds the destination open without
    /// FILE_SHARE_DELETE - which is what a virus scanner, a text editor, or a monitoring
    /// script polling the file does. Propagating it exited the runtime and left the window
    /// showing 心跳过期 with monitoring silently off.
    #[test]
    fn a_locked_state_file_does_not_stop_the_runtime() {
        use std::os::windows::fs::OpenOptionsExt;

        let dir = std::env::temp_dir().join(format!("a2a-state-write-{}", std::process::id()));
        let runtime = dir.join("runtime");
        fs::create_dir_all(&runtime).unwrap();
        let path = runtime.join("state.json");
        fs::write(&path, "{}").unwrap();

        // No sharing at all, the way a scanner or an editor holds it.
        let held = fs::OpenOptions::new()
            .read(true)
            .share_mode(0)
            .open(&path)
            .unwrap();
        // Returns instead of exiting, and does not disturb the locked file.
        publish_state(&dir, &json!({"probe": 1}));
        drop(held);
        assert_eq!(
            fs::read_to_string(&path).unwrap(),
            "{}",
            "the locked attempt must not have written"
        );

        // And it recovers as soon as the holder lets go.
        publish_state(&dir, &json!({"probe": 2}));
        let text = fs::read_to_string(&path).unwrap();
        assert!(text.contains("\"probe\": 2"), "must recover, got: {text}");
        let _ = fs::remove_dir_all(&dir);
    }
}
