//! V1 presentation model and explicit UI command queue; no V0 runtime reads.
use serde_json::{json, Value};
use std::{
    fs,
    io::Read,
    path::Path,
    time::{SystemTime, UNIX_EPOCH},
};

pub const MAX_JSON_BYTES: u64 = 2 * 1024 * 1024;
pub fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}
pub fn read_json(path: &Path) -> Option<Value> {
    let file = fs::File::open(path).ok()?;
    if file.metadata().ok()?.len() > MAX_JSON_BYTES {
        return None;
    }
    let mut bytes = Vec::new();
    file.take(MAX_JSON_BYTES + 1).read_to_end(&mut bytes).ok()?;
    if bytes.len() as u64 > MAX_JSON_BYTES {
        return None;
    }
    let bytes = bytes.strip_prefix(&[0xef, 0xbb, 0xbf]).unwrap_or(&bytes);
    serde_json::from_slice(bytes).ok()
}
pub fn str_field(v: &Value, key: &str) -> String {
    v[key].as_str().unwrap_or("").to_owned()
}
pub fn age_seconds(path: &Path) -> Option<u64> {
    Some(
        SystemTime::now()
            .duration_since(fs::metadata(path).ok()?.modified().ok()?)
            .ok()?
            .as_secs(),
    )
}
pub fn minutes_from_seconds(seconds: u64) -> u64 {
    seconds.div_ceil(60).clamp(1, 120)
}

/// Changes only poll_seconds. No start, resume, send, or pending request is emitted.
pub fn save_interval(product: &Path, minutes: u64) -> Result<(), String> {
    if !(1..=120).contains(&minutes) {
        return Err("请输入 1～120 分钟。".into());
    }
    let path = product.join("runtime/config.json");
    let mut value = read_json(&path).ok_or("无法读取现有配置，未覆盖文件。")?;
    if !value.is_object() {
        return Err("配置结构不正确，未写入。".into());
    }
    value["poll_seconds"] = json!(minutes * 60);
    let bytes = serde_json::to_vec_pretty(&value).map_err(|e| e.to_string())?;
    let temporary = path.with_file_name(format!("config.ui-{}.tmp", std::process::id()));
    fs::write(&temporary, &bytes).map_err(|e| e.to_string())?;
    let result = fs::rename(&temporary, &path).map_err(|e| e.to_string());
    if result.is_err() {
        let _ = fs::remove_file(temporary);
    }
    result
}

#[derive(Clone, Debug, PartialEq)]
pub struct BindingConfig {
    pub claude_title: String,
    pub claude_window: String,
    pub claude_session: String,
    pub dsh_session: String,
}
impl BindingConfig {
    pub fn load(product: &Path, legacy: &Value, target: &Value, collector: &Value) -> Self {
        let saved = read_json(&product.join("runtime/bindings.json")).unwrap_or(Value::Null);
        let legacy_title = str_field(target, "Title");
        let claude_window = saved["claude_window"]
            .as_str()
            .filter(|v| !v.is_empty())
            .unwrap_or(&legacy_title)
            .to_owned();
        let fallback_title = claude_window.trim_end_matches(" - Claude").to_owned();
        let claude_title = saved["claude_title"]
            .as_str()
            .filter(|v| !v.is_empty())
            .unwrap_or(&fallback_title)
            .to_owned();
        let claude_session = saved["claude_session"]
            .as_str()
            .filter(|v| !v.is_empty())
            .unwrap_or_else(|| collector["target_session_id"].as_str().unwrap_or(""))
            .to_owned();
        let dsh_session = saved["dsh_session"]
            .as_str()
            .filter(|v| !v.is_empty())
            .unwrap_or_else(|| legacy["targetSessionId"].as_str().unwrap_or(""))
            .to_owned();
        Self {
            claude_title,
            claude_window,
            claude_session,
            dsh_session,
        }
    }
}
fn valid_id(value: &str, prefix: &str) -> bool {
    value.starts_with(prefix)
        && value
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}
pub fn save_bindings(product: &Path, b: &BindingConfig) -> Result<(), String> {
    if b.claude_title.trim().is_empty() || b.claude_window.trim().is_empty() {
        return Err("Claude 会话名称和窗口标题不能为空。".into());
    }
    if !valid_id(b.claude_session.trim(), "cse_") {
        return Err("Claude Session ID 格式不正确，应以 cse_ 开头。".into());
    }
    if !valid_id(b.dsh_session.trim(), "session-") {
        return Err("DSH Session ID 格式不正确，应以 session- 开头。".into());
    }
    let path = product.join("runtime/bindings.json");
    let value = json!({
        "claude_title": b.claude_title.trim(),
        "claude_window": b.claude_window.trim(),
        "claude_session": b.claude_session.trim(),
        "dsh_session": b.dsh_session.trim()
    });
    let bytes = serde_json::to_vec_pretty(&value).map_err(|e| e.to_string())?;
    let temporary = path.with_file_name(format!("bindings.ui-{}.tmp", std::process::id()));
    fs::write(&temporary, &bytes).map_err(|e| e.to_string())?;
    let result = fs::rename(&temporary, &path).map_err(|e| e.to_string());
    if result.is_err() {
        let _ = fs::remove_file(temporary);
    }
    result
}

#[derive(Clone, Debug, PartialEq)]
pub struct Snapshot {
    pub claude_title: String,
    pub claude_session: String,
    pub dsh_title: String,
    pub dsh_session: String,
    pub target_window: String,
    pub dsh_turn: Option<u64>,
    pub dsh_running: Option<bool>,
    pub watch_turn: Option<u64>,
    pub watch_pid: u32,
    pub watch_state: String,
    pub watch_age: Option<u64>,
    pub watch_session_matches: bool,
    pub reply_turn: Option<u64>,
    pub receipt_state: String,
    /// The runtime's own notice for the last thing it did. It is the only place some verdicts
    /// appear - 「绑定已变化，请重新点击。」, 「本次已经排队，无需重复点击」, 「上一笔不是未送达的草稿，
    /// 未重试」 - so the window reads it and shows it when it changes; without that, a refused
    /// click and a dead button look exactly alike.
    pub runtime_notice: String,
    pub runtime_age: Option<u64>,
    pub poll_minutes: u64,
    pub poll_seconds: u64,
    pub dispatch_delay: u64,
    pub live: Value,
    pub config_available: bool,
    pub demo: Option<String>,
    pub demo_deadline_ms: Option<u64>,
    /// `--demo-enabled`: paint controls in their enabled look (clicks stay inert in demo).
    pub demo_enabled: bool,
}
impl Snapshot {
    pub fn load(product: &Path) -> Self {
        let config = read_json(&product.join("runtime/config.json"));
        let cfg = config.as_ref().unwrap_or(&Value::Null);
        let b = BindingConfig::load(product, &Value::Null, &Value::Null, &Value::Null);
        let path = product.join("runtime/state.json");
        let live = read_json(&path).unwrap_or(Value::Null);
        let matches = live["dsh_session"].as_str() == Some(b.dsh_session.as_str())
            && !b.dsh_session.is_empty();
        Self {
            claude_title: b.claude_title,
            claude_session: b.claude_session,
            target_window: b.claude_window,
            dsh_session: b.dsh_session,
            dsh_title: live["dsh_title"].as_str().unwrap_or("正在读取会话").into(),
            dsh_turn: if matches {
                live["dsh_turn"].as_u64()
            } else {
                None
            },
            dsh_running: if matches {
                live["dsh_running"].as_bool()
            } else {
                None
            },
            watch_turn: live["dsh_turn"].as_u64(),
            watch_pid: live["pid"].as_u64().unwrap_or(0) as u32,
            watch_state: str_field(&live, "phase"),
            watch_age: age_seconds(&path),
            watch_session_matches: matches,
            reply_turn: live["reply_turn"].as_u64(),
            receipt_state: str_field(&live["last_delivery"], "state"),
            runtime_notice: str_field(&live, "notice"),
            runtime_age: age_seconds(&path),
            poll_minutes: minutes_from_seconds(cfg["poll_seconds"].as_u64().unwrap_or(60)),
            poll_seconds: cfg["poll_seconds"].as_u64().unwrap_or(60).max(1),
            dispatch_delay: cfg["dispatch_delay_seconds"].as_u64().unwrap_or(10),
            live,
            config_available: config.is_some(),
            demo: None,
            demo_deadline_ms: None,
            demo_enabled: false,
        }
    }
    pub fn for_demo(name: &str) -> Self {
        // Reference states from design_reference.html use their own sample strings.
        let reference = matches!(name, "paused" | "running" | "stopped" | "success" | "error");
        // T3: `--demo=longtext` is a pure display fixture for the single-line ellipsis
        // check. It never reads runtime data and never sends anything (demo mode is
        // inert), so no real task name or error code is touched.
        let longtext = name == "longtext";
        let task = "超长任务名".repeat(40);
        Self {
            claude_title: if longtext {
                task.clone()
            } else if reference {
                "Frontend design handoff".into()
            } else {
                "静谧深空视觉优化设计".into()
            },
            claude_session: "demo-claude".into(),
            dsh_title: if longtext {
                task
            } else if reference {
                "读取交接文件准备新任务".into()
            } else {
                "修复环形星系节点布局".into()
            },
            dsh_session: "demo-dsh".into(),
            target_window: "静谧深空视觉优化设计 - Claude".into(),
            dsh_turn: Some(if reference { 14 } else { 108 }),
            dsh_running: Some(name == "busy"),
            watch_turn: Some(108),
            watch_pid: 0,
            watch_state: "running".into(),
            watch_age: Some(1),
            watch_session_matches: true,
            reply_turn: Some(107),
            receipt_state: "sent".into(),
            runtime_age: Some(0),
            poll_minutes: if reference { 1 } else { 10 },
            poll_seconds: if reference { 60 } else { 600 },
            dispatch_delay: 10,
            live: if longtext {
                // Long raw code for the banner chip: `derive` splits at the first
                // colon, so this paints an over-long code next to an over-long name.
                serde_json::json!({
                    "mode": "live",
                    "phase": "hold_send_uncertain",
                    "detail": format!(
                        "发送失败：SEND_UNCERTAIN: composer not confirmed {}",
                        "retry-budget-exhausted;".repeat(6)
                    )
                })
            } else {
                Value::Null
            },
            config_available: true,
            runtime_notice: String::new(),
            demo: Some(name.into()),
            demo_deadline_ms: if name == "countdown" {
                Some(now_ms() + 10_000)
            } else {
                None
            },
            demo_enabled: false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    fn temporary() -> PathBuf {
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        std::env::temp_dir().join(format!(
            "a2a-ui-test-{}-{}-{}",
            std::process::id(),
            now_ms(),
            NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ))
    }
    #[test]
    fn timing_clamped_and_rounded() {
        assert_eq!(minutes_from_seconds(60), 1);
        assert_eq!(minutes_from_seconds(61), 2);
        assert_eq!(minutes_from_seconds(0), 1);
        assert_eq!(minutes_from_seconds(u64::MAX), 120);
    }
    #[test]
    fn save_changes_only_interval_and_never_creates_requests() {
        let d = temporary();
        fs::create_dir_all(d.join("runtime")).unwrap();
        let original = json!({"enabled":false,"mode":"shadow","poll_seconds":60,"dispatch_delay_seconds":10,"extra":{"keep":true}});
        fs::write(
            d.join("runtime/config.json"),
            serde_json::to_vec(&original).unwrap(),
        )
        .unwrap();
        save_interval(&d, 3).unwrap();
        let v = read_json(&d.join("runtime/config.json")).unwrap();
        assert_eq!(v["poll_seconds"], 180);
        assert_eq!(v["enabled"], false);
        assert_eq!(v["extra"], original["extra"]);
        assert_eq!(fs::read_dir(d.join("runtime")).unwrap().count(), 1);
        assert!(save_interval(&d, 0).is_err());
        assert!(save_interval(&d, 121).is_err());
        fs::remove_dir_all(d).unwrap();
    }

    #[test]
    fn poll_now_is_accepted_and_unknown_commands_are_not() {
        let d = temporary();
        fs::create_dir_all(d.join("runtime")).unwrap();
        fs::write(
            d.join("runtime/bindings.json"),
            json!({"claude_title":"t","claude_window":"t - Claude",
                   "claude_session":"cse_x","dsh_session":"session-x"})
            .to_string(),
        )
        .unwrap();
        request_command(&d, "poll_now").unwrap();
        // The retry button's command must be on this list too: a command the window refuses
        // to emit looks exactly like a dead button ("操作未提交 / 不支持的操作").
        request_command(&d, "retry_delivery").unwrap();
        let dir = d.join("runtime/commands");
        let files: Vec<_> = fs::read_dir(&dir).unwrap().filter_map(Result::ok).collect();
        assert_eq!(files.len(), 2, "one command file per accepted command");
        let cmd = read_json(&files[0].path()).unwrap();
        assert_eq!(cmd["command"], "poll_now");
        assert_eq!(cmd["bindings"]["claude_session"], "cse_x");
        let retry = read_json(&files[1].path()).unwrap();
        assert_eq!(retry["command"], "retry_delivery");
        // Nothing else may be emitted: no pending request, no config write.
        assert!(!d.join("runtime/requests").exists());
        assert!(!d.join("runtime/config.json").exists());
        assert!(request_command(&d, "reverify").is_err());
        fs::remove_dir_all(d).unwrap();
    }

    #[test]
    fn bindings_round_trip_and_override_legacy() {
        let d = temporary();
        fs::create_dir_all(d.join("runtime")).unwrap();
        let b = BindingConfig {
            claude_title: "会话甲".into(),
            claude_window: "会话甲 - Claude".into(),
            claude_session: "cse_test_1".into(),
            dsh_session: "session-test-1".into(),
        };
        save_bindings(&d, &b).unwrap();
        let loaded = BindingConfig::load(&d, &Value::Null, &Value::Null, &Value::Null);
        assert_eq!(loaded, b);
        fs::remove_dir_all(d).unwrap();
    }
    #[test]
    fn utf8_bom_is_supported() {
        let d = temporary();
        fs::create_dir_all(&d).unwrap();
        let p = d.join("bom.json");
        fs::write(&p, b"\xef\xbb\xbf{\"enabled\":true}").unwrap();
        assert_eq!(read_json(&p).unwrap()["enabled"], true);
        fs::remove_dir_all(d).unwrap();
    }
    #[test]
    fn broken_data_is_not_reported_as_idle_or_connected() {
        let d = temporary();
        let s = Snapshot::load(&d);
        assert!(s.dsh_turn.is_none());
        assert!(s.dsh_running.is_none());
        assert!(!s.config_available);
    }
    #[test]
    fn oversize_json_is_rejected() {
        let d = temporary();
        fs::create_dir_all(&d).unwrap();
        let p = d.join("big.json");
        fs::File::create(&p)
            .unwrap()
            .set_len(MAX_JSON_BYTES + 1)
            .unwrap();
        assert!(read_json(&p).is_none());
        fs::remove_dir_all(d).unwrap();
    }
}

pub fn request_command(product: &Path, command: &str) -> Result<(), String> {
    if ![
        "send_dsh",
        "send_claude",
        "toggle",
        "cancel",
        "restore_listener",
        "poll_now",
        "retry_delivery",
    ]
    .contains(&command)
    {
        return Err("不支持的操作".into());
    }
    let bindings = read_json(&product.join("runtime/bindings.json")).ok_or("绑定尚未保存")?;
    let dir = product.join("runtime/commands");
    fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let id = format!(
        "{}-{}-{}",
        now_ms(),
        std::process::id(),
        NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    );
    let tmp = dir.join(format!("{id}.tmp"));
    let target = dir.join(format!("{id}.json"));
    fs::write(
        &tmp,
        serde_json::to_vec(&json!({"command":command,"bindings":bindings,"at_ms":now_ms()}))
            .unwrap(),
    )
    .map_err(|e| e.to_string())?;
    fs::rename(tmp, target).map_err(|e| e.to_string())
}

pub use handoff_core::message_template::MessageTemplates;
pub fn load_templates(product: &Path) -> Result<MessageTemplates, String> {
    let path = product.join("runtime/message-templates.json");
    if !path.exists() {
        return Ok(MessageTemplates::default());
    }
    let value = read_json(&path).ok_or("交接文案配置无法读取；原文件未改动。")?;
    let templates: MessageTemplates =
        serde_json::from_value(value).map_err(|e| format!("交接文案配置格式错误：{e}"))?;
    templates.validate()?;
    Ok(templates)
}
pub fn save_templates(product: &Path, templates: &MessageTemplates) -> Result<(), String> {
    templates.validate()?;
    let path = product.join("runtime/message-templates.json");
    let tmp = path.with_file_name(format!("message-templates.ui-{}.tmp", std::process::id()));
    let bytes = serde_json::to_vec_pretty(templates).map_err(|e| e.to_string())?;
    {
        use std::io::Write;
        let mut file = fs::File::create(&tmp).map_err(|e| e.to_string())?;
        file.write_all(&bytes).map_err(|e| e.to_string())?;
        file.sync_all().map_err(|e| e.to_string())?;
    }
    let result = fs::rename(&tmp, &path).map_err(|e| e.to_string());
    if result.is_err() {
        let _ = fs::remove_file(tmp);
    }
    result
}
#[cfg(test)]
mod message_config_tests {
    use super::*;
    use std::path::PathBuf;
    fn temp() -> PathBuf {
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let d = std::env::temp_dir().join(format!(
            "a2a-message-{}-{}-{}",
            std::process::id(),
            now_ms(),
            NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ));
        fs::create_dir_all(d.join("runtime")).unwrap();
        d
    }
    #[test]
    fn saving_affixes_only_writes_message_configuration() {
        let d = temp();
        let initial = json!({"enabled":true,"poll_seconds":60});
        fs::write(d.join("runtime/config.json"), initial.to_string()).unwrap();
        fs::write(
            d.join("runtime/workflow.json"),
            "{\"phase\":\"hold_send_uncertain\"}",
        )
        .unwrap();
        let old = fs::read(d.join("runtime/workflow.json")).unwrap();
        let mut t = load_templates(&d).unwrap();
        t.to_dsh.prefix = "中文\n前置".into();
        t.to_dsh.suffix = "\n后置\n".into();
        save_templates(&d, &t).unwrap();
        assert_eq!(load_templates(&d).unwrap(), t);
        assert_eq!(read_json(&d.join("runtime/config.json")).unwrap(), initial);
        assert_eq!(fs::read(d.join("runtime/workflow.json")).unwrap(), old);
        assert!(!d.join("runtime/commands").exists());
        assert!(!d.join("runtime/requests").exists());
        fs::remove_dir_all(d).unwrap();
    }
    #[test]
    fn broken_configuration_is_never_silently_replaced_with_defaults() {
        let d = temp();
        fs::write(d.join("runtime/message-templates.json"), "{broken}").unwrap();
        assert!(load_templates(&d).is_err());
        assert_eq!(
            fs::read_to_string(d.join("runtime/message-templates.json")).unwrap(),
            "{broken}"
        );
        fs::remove_dir_all(d).unwrap();
    }
}
