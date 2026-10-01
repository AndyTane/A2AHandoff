//! Draft first, then one cancellable deadline, then one submit. No confirmation branch.
use super::*;
pub(super) fn ready(p: &Value, t: u64) -> bool {
    text(p, "stage") == "draft_ready"
        && number(p, "deadline_ms") > 0
        && t >= number(p, "deadline_ms")
}
pub(super) fn accept_draft(p: &mut Value, receipt: &Value) -> Result<(), String> {
    let ready_at = number(receipt, "draft_ready_at_ms");
    let deadline_ms = number(receipt, "deadline_ms");
    if text(receipt, "id") != text(p, "id")
        || text(receipt, "state") != "draft_ready"
        || text(receipt, "flow") != "draft-first-v1"
        || text(receipt, "outgoing_hash") != hash(text(p, "text"))
        // Checked before any arithmetic: a deadline earlier than the write is
        // malformed, and unsigned subtraction would underflow on it.
        || deadline_ms < ready_at
    {
        return Err("DEADLINE_BEFORE_WRITE".into());
    }
    // The adapter decides the deadline, because it is the side that knows whether the
    // verified write left a cancellable countdown. Re-deriving it here from
    // `dispatch_delay_seconds` duplicated that rule, and it broke as soon as the two
    // sides disagreed: a manual request dispatches immediately, so a receipt whose
    // deadline equals the write time is correct even though the configured delay is
    // larger. So validate the reported value against the countdown the adapter was
    // entitled to grant for this request - zero for a manual request, otherwise the
    // configured delay, never shorter than the window the adapter actually measured.
    let configured = number(p, "dispatch_delay_seconds");
    let effective = if yes(p, "manual") {
        0
    } else {
        configured.max((deadline_ms - ready_at).div_ceil(1000))
    };
    if deadline_ms < ready_at.saturating_add(effective.saturating_mul(1000))
        || deadline_ms > ready_at.saturating_add(configured.max(120).saturating_mul(1000))
    {
        return Err("DEADLINE_OUT_OF_WINDOW".into());
    }
    p["stage"] = json!("draft_ready");
    p["draft_ready_at_ms"] = receipt["draft_ready_at_ms"].clone();
    p["deadline_ms"] = receipt["deadline_ms"].clone();
    p["retry_after_ms"] = json!(0);
    Ok(())
}
pub(super) fn cancel(home: &Path, w: &mut Value, reason: &str) -> Result<(), String> {
    let p = w["pending"].clone();
    if p.is_null() {
        return Ok(());
    }
    record(w, &p, "cancelled");
    let path = home.join(format!("receipts/{}.json", text(&p, "id")));
    if let Some(mut receipt) = read(&path) {
        if text(&receipt, "flow") == "draft-first-v1" && text(&receipt, "state") == "draft_ready" {
            receipt["state"] = json!("cancelled_before_send");
            receipt["at_ms"] = json!(now());
            write(&path, &receipt)?;
        }
    }
    w["pending"] = Value::Null;
    w["notice"] = json!(reason);
    diagnostic(
        home,
        "delivery_cancelled",
        &json!({"id":p["id"],"reason":reason}),
    );
    Ok(())
}
/// A receipt we will not act on: keep the draft, stay up, and try again.
///
/// Distinct from `preparation_failed`, which means the adapter could not stage the
/// draft at all. Here the draft IS staged and verified - only the countdown could not
/// be accepted - so the pending item is preserved untouched and the retry is short.
fn deferred(
    home: &Path,
    w: &mut Value,
    p: &Value,
    problem: &str,
    error: &mut String,
    receipt: &Value,
) {
    w["pending"] = p.clone();
    w["pending"]["retry_after_ms"] = json!(now() + 5000);
    *error = format!("草稿回执暂未被接受（{problem}），保持未发送，稍后重试");
    diagnostic(
        home,
        "draft_receipt_deferred",
        &json!({
            "id":p["id"],
            "problem":problem,
            "receipt_state":receipt["state"],
            "deadline_ms":receipt["deadline_ms"],
            "draft_ready_at_ms":receipt["draft_ready_at_ms"],
            "dispatch_delay_seconds":p["dispatch_delay_seconds"],
            "manual":p["manual"],
            "phase":w["phase"]
        }),
    );
}
fn preparation_failed(w: &mut Value, problem: &str, error: &mut String) {
    let occupied =
        problem.starts_with("DRAFT_OCCUPIED") || problem == "EXISTING_ATTACHMENT_PRESERVED";
    let blocked = occupied
        || matches!(
            problem,
            "DRAFT_READBACK_CONFLICT" | "PLAIN_TEXT_INPUT_UNSUPPORTED"
        );
    if blocked {
        w["pending"] = Value::Null;
        w["phase"] = json!("hold_preparation");
        *error = if occupied {
            "目标输入框已有草稿，已保留；清空后再点发送。".into()
        } else {
            format!("输入框状态无法确认，已停止重试：{problem}")
        };
        w["notice"] = json!(&*error);
    } else {
        w["pending"]["stage"] = json!("waiting_target");
        w["pending"]["retry_after_ms"] = json!(now() + 5000);
        *error = format!("等待目标输入框恢复：{problem}");
    }
}
pub(super) fn drive(
    root: &Path,
    owner: u32,
    cfg: &handoff_core::config::RuntimeConfig,
    b: &Value,
    w: &mut Value,
    d: &mut Value,
    c: &mut Value,
    error: &mut String,
) -> Result<(), String> {
    let home = root.join("runtime");
    let mut p = w["pending"].clone();
    if p.is_null() {
        return Ok(());
    }
    if !cfg.enabled && !yes(&p, "manual") {
        return cancel(&home, w, "自动已暂停；草稿保留，不会自动发送");
    }
    if number(&p, "retry_after_ms") > now() {
        return Ok(());
    }
    let staged = text(&p, "stage") == "draft_ready";
    if staged && !ready(&p, now()) {
        return Ok(());
    }
    *d = observe_dsh(root, owner);
    *c = observe_claude(root, owner, b, cfg);
    if read(&home.join("bindings.json")).as_ref() != Some(b) || manual_changed(w, d, c) {
        cancel(&home, w, "会话发生变化，已取消发送；草稿保留")?;
        w["phase"] = json!("paused_by_user");
        error.clear();
        return Ok(());
    }
    if !yes(d, "ok") || !yes(c, "ok") {
        *error = "会话暂不可读，未发送".into();
        w["pending"]["retry_after_ms"] = json!(now() + 2000);
        return Ok(());
    }
    let templates = load_message_templates(&home)?;
    let q = candidate(
        &templates,
        w,
        b,
        d,
        c,
        number(&p, "dispatch_delay_seconds"),
        yes(&p, "manual"),
    );
    let same = q.as_ref().is_some_and(|q| {
        q["id"] == p["id"]
            && q["source_seq"] == p["source_seq"]
            && q["text"] == p["text"]
            && q["message_templates"] == p["message_templates"]
    });
    if !same {
        cancel(&home, w, "来源或文案已变化，本次取消；不会覆盖现有草稿")?;
        *error = "来源或文案发生变化，未发送".into();
        return Ok(());
    }
    if !staged {
        if let Some(fresh) = q.as_ref() {
            p["claude_reply_index"] = fresh["claude_reply_index"].clone();
            p["claude_reply_hash"] = fresh["claude_reply_hash"].clone();
            p["dsh_turn"] = fresh["dsh_turn"].clone();
            w["pending"] = p.clone();
        }
    }
    let receipt_path = home.join(format!("receipts/{}.json", text(&p, "id")));
    if let Some(r) = read(&receipt_path) {
        if text(&r, "flow") != "draft-first-v1" || receipt_is_terminal(text(&r, "state")) {
            commit_receipt(w, &p, &r);
            *error = "已有投递记录，未重复发送".into();
            return Ok(());
        }
    }
    let request = home.join(format!("requests/{}.json", text(&p, "id")));
    write(&request, &p)?;
    write(&home.join("workflow.json"), w)?;
    if text(&p, "stage") == "waiting_target" {
        publish(root, cfg.enabled, b, w, d, c, error, false)?;
    } else {
        publish(root, cfg.enabled, b, w, d, c, "", true)?;
    }
    let operation = if staged { "Commit" } else { "Prepare" };
    let result = run(
        root,
        owner,
        Command::new("powershell.exe")
            .args(["-NoProfile", "-STA", "-File"])
            .arg(root.join("adapters/windows/draft-flow.ps1"))
            .arg("-ProductRoot")
            .arg(root)
            .arg("-RequestFile")
            .arg(&request)
            .args(["-Operation", operation]),
    )
    .unwrap_or_else(|e| json!({"ok":false,"error":e}));
    if let Some(r) = read(&receipt_path) {
        if !staged && yes(&result, "ok") && text(&r, "state") == "draft_ready" {
            if let Err(problem) = accept_draft(&mut w["pending"], &r) {
                // A receipt the runtime will not accept is a retryable condition, not
                // a fatal one. Treating it as fatal killed the whole runtime and left
                // the draft stranded in the composer - which is exactly what a
                // version skew between the runtime and the adapter produced. Stay up,
                // keep the draft, and try again; nothing is sent without acceptance.
                deferred(&home, w, &p, &problem, error, &r);
                write(&home.join("workflow.json"), w)?;
                return Ok(());
            }
            w["notice"] = json!("内容已填入输入框；倒计时结束自动发送，可取消");
            error.clear();
            diagnostic(&home, "draft_ready", &r);
        } else if staged
            && text(&r, "state") == "draft_ready"
            && text(&result, "error") == "COUNTDOWN_NOT_FINISHED"
        {
            w["pending"]["deadline_ms"] = r["deadline_ms"].clone();
        } else {
            commit_receipt(w, &p, &r);
            diagnostic(&home, "delivery_receipt", &r);
            if text(&r, "state") == "sent" {
                error.clear()
            } else {
                *error = format!("本次已停止：{}", text(&result, "error"))
            }
        }
    } else {
        preparation_failed(w, text(&result, "error"), error);
        diagnostic(
            &home,
            "preparation_deferred",
            &json!({"id":p["id"],"error":result["error"],"phase":w["phase"]}),
        );
    }
    write(&home.join("workflow.json"), w)?;
    publish(root, cfg.enabled, b, w, d, c, error, false)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn pending() -> Value {
        json!({"id":"test","text":"report","stage":"queued","deadline_ms":0,"dispatch_delay_seconds":10})
    }
    fn receipt() -> Value {
        json!({"id":"test","state":"draft_ready","flow":"draft-first-v1","outgoing_hash":hash("report"),"draft_ready_at_ms":1000,"deadline_ms":11000})
    }
    #[test]
    fn no_timer_before_successful_write() {
        let p = pending();
        assert!(!ready(&p, u64::MAX));
    }
    #[test]
    fn manual_send_is_ready_immediately_after_the_verified_write() {
        let mut p = pending();
        p["manual"] = json!(true);
        p["dispatch_delay_seconds"] = json!(0);
        let mut r = receipt();
        // A zero delay makes the reported deadline equal to the write time.
        r["deadline_ms"] = json!(1000);
        accept_draft(&mut p, &r).unwrap();
        assert!(ready(&p, 1000));
        assert!(ready(&p, u64::MAX));
    }
    #[test]
    fn immediate_deadline_is_accepted_even_when_a_configured_delay_is_present() {
        // The adapter decides the deadline. An immediate dispatch must not be
        // rejected just because the request still carries the configured delay -
        // that mismatch is what stalled a real manual send.
        let mut p = pending();
        p["manual"] = json!(true);
        assert_eq!(p["dispatch_delay_seconds"], 10);
        let mut r = receipt();
        r["deadline_ms"] = json!(1000);
        accept_draft(&mut p, &r).unwrap();
        assert!(ready(&p, 1000));
    }
    #[test]
    fn deadline_outside_the_permitted_window_is_rejected() {
        let mut p = pending();
        // Earlier than the verified write is never legitimate.
        let mut r = receipt();
        r["deadline_ms"] = json!(999);
        assert_eq!(
            accept_draft(&mut p, &r).unwrap_err(),
            "DEADLINE_BEFORE_WRITE"
        );
        // A delay is capped at 120s, so a far-future deadline is not a real countdown.
        let mut r2 = receipt();
        r2["deadline_ms"] = json!(1000 + 130 * 1000);
        assert_eq!(
            accept_draft(&mut p, &r2).unwrap_err(),
            "DEADLINE_OUT_OF_WINDOW"
        );
        // A manual request dispatches immediately, so an immediate deadline is
        // accepted even though the request still carries a configured delay.
        p["manual"] = json!(true);
        let mut r3 = receipt();
        r3["deadline_ms"] = json!(1000);
        assert!(accept_draft(&mut p, &r3).is_ok());
    }
    #[test]
    fn a_deferred_receipt_keeps_the_draft_and_backs_off() {
        // The runtime must never die over a receipt it will not act on: the draft is
        // already verified and sitting in the composer, so it is preserved for retry.
        let p = pending();
        let mut w = json!({"phase":"waiting_claude","pending":p.clone(),"ledger":[],"notice":""});
        let mut error = String::new();
        let before = now();
        // A throwaway home: this writes a diagnostic and must not touch the real one.
        let home = std::env::temp_dir().join(format!("a2a-deferred-test-{}", std::process::id()));
        deferred(
            &home,
            &mut w,
            &p,
            "DEADLINE_OUT_OF_WINDOW",
            &mut error,
            &receipt(),
        );
        let _ = fs::remove_dir_all(&home);
        assert_eq!(w["pending"]["id"], p["id"]);
        assert_eq!(w["pending"]["text"], p["text"]);
        assert_eq!(w["pending"]["stage"], p["stage"]);
        assert!(number(&w["pending"], "retry_after_ms") >= before + 5000);
        assert!(!error.is_empty());
        assert!(error.contains("DEADLINE_OUT_OF_WINDOW"));
    }
    #[test]
    fn full_ten_seconds_after_verified_write() {
        let mut p = pending();
        accept_draft(&mut p, &receipt()).unwrap();
        assert!(!ready(&p, 10999));
        assert!(ready(&p, 11000));
    }
    #[test]
    fn slow_preparation_does_not_consume_countdown() {
        let mut p = pending();
        let mut r = receipt();
        r["draft_ready_at_ms"] = json!(19000);
        r["deadline_ms"] = json!(29000);
        accept_draft(&mut p, &r).unwrap();
        assert!(!ready(&p, 20000));
        assert!(ready(&p, 29000));
    }
    #[test]
    fn failed_write_cannot_start_countdown() {
        let mut p = pending();
        let mut r = receipt();
        r["state"] = json!("draft_unverified");
        assert!(accept_draft(&mut p, &r).is_err());
        assert!(!ready(&p, u64::MAX));
    }
    #[test]
    fn shortened_deadline_is_rejected() {
        let mut p = pending();
        let mut r = receipt();
        r["deadline_ms"] = json!(10000);
        assert!(accept_draft(&mut p, &r).is_err());
    }
    #[test]
    fn only_a_receipt_that_went_out_is_terminal() {
        // `sent` and `submit_uncertain` must block a second attempt forever - that is
        // the durable-receipt guarantee. Everything else records an abandoned draft,
        // so the delivery never left and must stay retryable; otherwise cancelling a
        // handoff makes its reply impossible to send even by an explicit click.
        assert!(receipt_is_terminal("sent"));
        assert!(receipt_is_terminal("submit_uncertain"));
        for retryable in [
            "draft_ready",
            "cancelled_before_send",
            "draft_write_attempted",
            "draft_unverified",
            "send_attempted",
        ] {
            assert!(
                !receipt_is_terminal(retryable),
                "{retryable} must stay retryable"
            );
        }
    }
    #[test]
    fn changed_content_is_rejected() {
        let mut p = pending();
        p["text"] = json!("different");
        assert!(accept_draft(&mut p, &receipt()).is_err());
    }
    #[test]
    fn occupied_input_stops_without_consuming_reply() {
        let mut w =
            json!({"phase":"waiting_claude","pending":{"id":"test","stage":"queued"},"ledger":[]});
        let mut error = String::new();
        preparation_failed(&mut w, "DRAFT_OCCUPIED_PRESERVED", &mut error);
        assert!(w["pending"].is_null());
        assert_eq!(w["phase"], "hold_preparation");
        assert_eq!(w["ledger"], json!([]));
        assert!(!error.is_empty());
    }
    #[test]
    fn unavailable_window_has_stable_retry_state() {
        let mut w =
            json!({"phase":"waiting_claude","pending":{"id":"test","stage":"queued"},"ledger":[]});
        let mut error = String::new();
        let before = now();
        preparation_failed(&mut w, "TARGET_WINDOW_UNAVAILABLE_OR_AMBIGUOUS", &mut error);
        assert_eq!(w["pending"]["stage"], "waiting_target");
        assert!(number(&w["pending"], "retry_after_ms") >= before + 5000);
        assert_eq!(w["pending"]["id"], "test");
    }
}
