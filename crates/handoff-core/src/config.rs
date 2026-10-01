//! The one place that defines the persisted runtime configuration schema.
//!
//! Adapters must not invent their own fallbacks: Rust owns the schema, and the
//! PowerShell and Node adapters read the same file with the same field semantics
//! (see `adapters/common/a2a-config.ps1` and `adapters/common/a2a-config.mjs`).
//!
//! Two layers are exposed to users:
//!
//! * basic: `dsh_data_home`, `dsh_web_origin`, `poll_seconds`, `dispatch_delay_seconds`
//! * advanced: `dsh_browser_processes`, `workspace`, `claude_host`, `dsh_page_title_pattern`
//!
//! Deprecated keys (`task_file`, `mode`, `next_poll_at_ms`) are ignored on load and
//! never written back.
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::path::{Path, PathBuf};

pub const CONFIG_FILE: &str = "runtime/config.json";

/// Automatic handoff switch default for a brand new installation.
pub const DEFAULT_ENABLED: bool = false;
pub const DEFAULT_POLL_SECONDS: u64 = 60;
pub const DEFAULT_DISPATCH_DELAY_SECONDS: u64 = 10;
pub const DEFAULT_DSH_WEB_ORIGIN: &str = "http://127.0.0.1:3080";
pub const DEFAULT_CLAUDE_HOST: &str = "claude.ai";
pub const DEFAULT_DSH_PAGE_TITLE_PATTERN: &str = "DeepSeek Harness";
pub const DEFAULT_DSH_BROWSER_PROCESSES: [&str; 2] = ["msedge", "chrome"];

/// Claude observation interval bounds (seconds).
pub const MIN_POLL_SECONDS: u64 = 1;
pub const MAX_POLL_SECONDS: u64 = 7200;
/// Draft-to-submit delay bounds (seconds).
pub const MIN_DISPATCH_DELAY_SECONDS: u64 = 1;
pub const MAX_DISPATCH_DELAY_SECONDS: u64 = 120;

/// Keys that used to live in `config.json` and must not be written again.
pub const DEPRECATED_KEYS: [&str; 3] = ["task_file", "mode", "next_poll_at_ms"];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DshWebOrigin {
    pub scheme: String,
    pub host: String,
    pub port: u16,
}

impl DshWebOrigin {
    /// Parse `scheme://host[:port]`. Only http/https are accepted; a missing port
    /// falls back to the scheme default so "http://localhost" stays usable.
    pub fn parse(text: &str) -> Result<Self, String> {
        let trimmed = text.trim();
        let (scheme, rest) = trimmed.split_once("://").ok_or_else(|| {
            format!("DSH web origin must start with http:// or https://: {trimmed}")
        })?;
        let scheme = scheme.to_ascii_lowercase();
        if scheme != "http" && scheme != "https" {
            return Err(format!(
                "DSH web origin scheme must be http or https, got {scheme:?}"
            ));
        }
        // Drop any path/query: only the origin is meaningful here.
        let authority = rest
            .split(['/', '?', '#'])
            .next()
            .unwrap_or_default()
            .trim();
        if authority.is_empty() {
            return Err(format!("DSH web origin has no host: {trimmed}"));
        }
        // Strip userinfo if present; it is never part of the identity check.
        let authority = authority.rsplit('@').next().unwrap_or(authority);
        let (host, port) = match authority.rsplit_once(':') {
            Some((h, p))
                if !h.is_empty() && !p.is_empty() && p.chars().all(|c| c.is_ascii_digit()) =>
            {
                let port: u16 = p
                    .parse()
                    .map_err(|_| format!("DSH web origin port out of range: {p}"))?;
                (h, port)
            }
            _ => (authority, if scheme == "https" { 443 } else { 80 }),
        };
        if host.is_empty() {
            return Err(format!("DSH web origin has no host: {trimmed}"));
        }
        Ok(Self {
            scheme,
            host: host.to_ascii_lowercase(),
            port,
        })
    }

    /// Canonical `scheme://host:port` used when persisting.
    pub fn to_origin_string(&self) -> String {
        format!("{}://{}:{}", self.scheme, self.host, self.port)
    }
}

impl std::fmt::Display for DshWebOrigin {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.to_origin_string())
    }
}

/// The persisted runtime configuration. Field order here is the JSON field order.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct RuntimeConfig {
    pub enabled: bool,
    pub poll_seconds: u64,
    pub dispatch_delay_seconds: u64,
    pub dsh_data_home: String,
    pub dsh_web_origin: String,
    pub dsh_browser_processes: Vec<String>,
    pub workspace: String,
    pub claude_host: String,
    pub dsh_page_title_pattern: String,
}

impl Default for RuntimeConfig {
    fn default() -> Self {
        Self {
            enabled: DEFAULT_ENABLED,
            poll_seconds: DEFAULT_POLL_SECONDS,
            dispatch_delay_seconds: DEFAULT_DISPATCH_DELAY_SECONDS,
            dsh_data_home: String::new(),
            dsh_web_origin: DEFAULT_DSH_WEB_ORIGIN.to_owned(),
            dsh_browser_processes: default_browser_processes(),
            workspace: String::new(),
            claude_host: DEFAULT_CLAUDE_HOST.to_owned(),
            dsh_page_title_pattern: DEFAULT_DSH_PAGE_TITLE_PATTERN.to_owned(),
        }
    }
}

pub fn default_browser_processes() -> Vec<String> {
    DEFAULT_DSH_BROWSER_PROCESSES
        .iter()
        .map(|s| (*s).to_owned())
        .collect()
}

/// Why a value had to be corrected while loading. Reported for diagnostics.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfigNotice {
    pub field: &'static str,
    pub detail: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoadedConfig {
    pub config: RuntimeConfig,
    pub notices: Vec<ConfigNotice>,
}

impl LoadedConfig {
    fn new(config: RuntimeConfig) -> Self {
        Self {
            config,
            notices: Vec::new(),
        }
    }
    fn note(&mut self, field: &'static str, detail: impl Into<String>) {
        self.notices.push(ConfigNotice {
            field,
            detail: detail.into(),
        });
    }
}

fn as_u64(value: &Value) -> Option<u64> {
    value.as_u64()
}

fn as_string(value: &Value) -> Option<String> {
    value.as_str().map(str::to_owned)
}

fn as_bool(value: &Value) -> Option<bool> {
    value.as_bool()
}

/// Build a config from JSON, filling defaults for missing/illegal values and
/// ignoring deprecated keys. Never fails: a machine-readable config that is
/// partly wrong still starts, with the bad parts replaced and reported.
pub fn from_value(value: &Value) -> LoadedConfig {
    let mut out = LoadedConfig::new(RuntimeConfig::default());
    let Some(object) = value.as_object() else {
        out.note("config", "not a JSON object; using defaults");
        return out;
    };

    for key in DEPRECATED_KEYS {
        if object.contains_key(key) {
            out.note(key, "deprecated key ignored");
        }
    }

    if let Some(v) = object.get("enabled") {
        match as_bool(v) {
            Some(b) => out.config.enabled = b,
            None => out.note("enabled", "not a boolean; using default false"),
        }
    }
    if let Some(v) = object.get("poll_seconds") {
        match as_u64(v) {
            Some(n) if (MIN_POLL_SECONDS..=MAX_POLL_SECONDS).contains(&n) => {
                out.config.poll_seconds = n
            }
            Some(n) => {
                out.config.poll_seconds = n.clamp(MIN_POLL_SECONDS, MAX_POLL_SECONDS);
                out.note(
                    "poll_seconds",
                    format!("{n} out of range; clamped to {}", out.config.poll_seconds),
                );
            }
            None => out.note(
                "poll_seconds",
                format!("not a positive integer; using {DEFAULT_POLL_SECONDS}"),
            ),
        }
    }
    if let Some(v) = object.get("dispatch_delay_seconds") {
        match as_u64(v) {
            Some(n) if (MIN_DISPATCH_DELAY_SECONDS..=MAX_DISPATCH_DELAY_SECONDS).contains(&n) => {
                out.config.dispatch_delay_seconds = n
            }
            Some(n) => {
                out.config.dispatch_delay_seconds =
                    n.clamp(MIN_DISPATCH_DELAY_SECONDS, MAX_DISPATCH_DELAY_SECONDS);
                out.note(
                    "dispatch_delay_seconds",
                    format!(
                        "{n} out of range; clamped to {}",
                        out.config.dispatch_delay_seconds
                    ),
                );
            }
            None => out.note(
                "dispatch_delay_seconds",
                format!("not a positive integer; using {DEFAULT_DISPATCH_DELAY_SECONDS}"),
            ),
        }
    }
    if let Some(v) = object.get("dsh_data_home") {
        match as_string(v) {
            Some(s) => out.config.dsh_data_home = s.trim().to_owned(),
            None => out.note("dsh_data_home", "not a string; treated as unset"),
        }
    }
    if let Some(v) = object.get("dsh_web_origin") {
        match as_string(v) {
            Some(s) => match DshWebOrigin::parse(&s) {
                Ok(origin) => out.config.dsh_web_origin = origin.to_origin_string(),
                Err(e) => out.note(
                    "dsh_web_origin",
                    format!("{e}; using default {DEFAULT_DSH_WEB_ORIGIN}"),
                ),
            },
            None => out.note(
                "dsh_web_origin",
                format!("not a string; using default {DEFAULT_DSH_WEB_ORIGIN}"),
            ),
        }
    }
    if let Some(v) = object.get("dsh_browser_processes") {
        match v.as_array() {
            Some(items) => {
                let names: Vec<String> = items
                    .iter()
                    .filter_map(as_string)
                    .map(|s| s.trim().to_ascii_lowercase())
                    .filter(|s| !s.is_empty())
                    .collect();
                if names.is_empty() {
                    out.config.dsh_browser_processes = default_browser_processes();
                    out.note(
                        "dsh_browser_processes",
                        "empty or unusable list; fell back to msedge, chrome",
                    );
                } else {
                    out.config.dsh_browser_processes = names;
                }
            }
            None => {
                out.config.dsh_browser_processes = default_browser_processes();
                out.note(
                    "dsh_browser_processes",
                    "not an array; fell back to msedge, chrome",
                );
            }
        }
    }
    if let Some(v) = object.get("workspace") {
        match as_string(v) {
            Some(s) => out.config.workspace = s.trim().to_owned(),
            None => out.note("workspace", "not a string; workspace check disabled"),
        }
    }
    if let Some(v) = object.get("claude_host") {
        match as_string(v) {
            Some(s) if !s.trim().is_empty() => {
                out.config.claude_host = s.trim().to_ascii_lowercase()
            }
            Some(_) => out.note(
                "claude_host",
                format!("empty; using default {DEFAULT_CLAUDE_HOST}"),
            ),
            None => out.note(
                "claude_host",
                format!("not a string; using default {DEFAULT_CLAUDE_HOST}"),
            ),
        }
    }
    if let Some(v) = object.get("dsh_page_title_pattern") {
        match as_string(v) {
            Some(s) if !s.trim().is_empty() => out.config.dsh_page_title_pattern = s,
            Some(_) => out.note(
                "dsh_page_title_pattern",
                format!("empty; using default {DEFAULT_DSH_PAGE_TITLE_PATTERN}"),
            ),
            None => out.note(
                "dsh_page_title_pattern",
                format!("not a string; using default {DEFAULT_DSH_PAGE_TITLE_PATTERN}"),
            ),
        }
    }
    out
}

/// Parse `runtime/config.json` text. Deprecated keys are ignored, never fatal.
pub fn from_json_str(text: &str) -> LoadedConfig {
    let stripped = text.strip_prefix('\u{feff}').unwrap_or(text);
    match serde_json::from_str::<Value>(stripped) {
        Ok(v) => from_value(&v),
        Err(e) => {
            let mut out = LoadedConfig::new(RuntimeConfig::default());
            out.note("config", format!("unreadable JSON ({e}); using defaults"));
            out
        }
    }
}

/// Load from `<product>/runtime/config.json`. A missing file yields defaults with
/// `dsh_data_home` unset, which is the first-run signal.
pub fn load(product: &Path) -> LoadedConfig {
    let path = product.join(CONFIG_FILE);
    match std::fs::read_to_string(&path) {
        Ok(text) => from_json_str(&text),
        Err(_) => {
            let mut out = LoadedConfig::new(RuntimeConfig::default());
            out.note("config", "not found; first run");
            out
        }
    }
}

/// Serialize exactly the schema fields, in a stable order. Deprecated keys cannot
/// reappear because the value is rebuilt from typed fields.
pub fn to_value(config: &RuntimeConfig) -> Value {
    let mut map = Map::new();
    map.insert("enabled".into(), Value::Bool(config.enabled));
    map.insert("poll_seconds".into(), Value::from(config.poll_seconds));
    map.insert(
        "dispatch_delay_seconds".into(),
        Value::from(config.dispatch_delay_seconds),
    );
    map.insert(
        "dsh_data_home".into(),
        Value::String(config.dsh_data_home.clone()),
    );
    map.insert(
        "dsh_web_origin".into(),
        Value::String(config.dsh_web_origin.clone()),
    );
    map.insert(
        "dsh_browser_processes".into(),
        Value::Array(
            config
                .dsh_browser_processes
                .iter()
                .map(|s| Value::String(s.clone()))
                .collect(),
        ),
    );
    map.insert("workspace".into(), Value::String(config.workspace.clone()));
    map.insert(
        "claude_host".into(),
        Value::String(config.claude_host.clone()),
    );
    map.insert(
        "dsh_page_title_pattern".into(),
        Value::String(config.dsh_page_title_pattern.clone()),
    );
    Value::Object(map)
}

pub fn to_json_string(config: &RuntimeConfig) -> String {
    let mut text = serde_json::to_string_pretty(&to_value(config)).unwrap_or_else(|_| "{}".into());
    text.push('\n');
    text
}

/// Atomically write the config, leaving no deprecated keys behind.
pub fn save(product: &Path, config: &RuntimeConfig) -> Result<(), String> {
    let path = product.join(CONFIG_FILE);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let tmp = path.with_file_name(format!("config.a2a-{}.tmp", std::process::id()));
    std::fs::write(&tmp, to_json_string(config)).map_err(|e| e.to_string())?;
    let result = std::fs::rename(&tmp, &path).map_err(|e| e.to_string());
    if result.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    result
}

impl RuntimeConfig {
    /// Validation shared by the UI, bootstrap and the runtime. Returns every
    /// problem so a settings form can show them all at once.
    pub fn validate(&self) -> Vec<String> {
        let mut problems = Vec::new();
        if !(MIN_POLL_SECONDS..=MAX_POLL_SECONDS).contains(&self.poll_seconds) {
            problems.push(format!(
                "自动轮询周期必须在 {MIN_POLL_SECONDS}–{MAX_POLL_SECONDS} 秒之间。"
            ));
        }
        if !(MIN_DISPATCH_DELAY_SECONDS..=MAX_DISPATCH_DELAY_SECONDS)
            .contains(&self.dispatch_delay_seconds)
        {
            problems.push(format!(
                "发送倒计时必须在 {MIN_DISPATCH_DELAY_SECONDS}–{MAX_DISPATCH_DELAY_SECONDS} 秒之间。"
            ));
        }
        if let Err(e) = DshWebOrigin::parse(&self.dsh_web_origin) {
            problems.push(e);
        }
        if !self.dsh_data_home.trim().is_empty() && !Path::new(&self.dsh_data_home).is_dir() {
            problems.push(format!("DSH 数据目录不存在：{}", self.dsh_data_home));
        }
        if self.claude_host.trim().is_empty() {
            problems.push("Claude Host 不能为空。".into());
        }
        if self.dsh_page_title_pattern.trim().is_empty() {
            problems.push("DSH 页面标题特征不能为空。".into());
        }
        problems
    }

    /// Resolve the DSH web origin, falling back to the default when the stored
    /// value is unusable. Used by adapters that must never fail closed on a typo.
    pub fn parsed_origin(&self) -> DshWebOrigin {
        DshWebOrigin::parse(&self.dsh_web_origin).unwrap_or_else(|_| {
            DshWebOrigin::parse(DEFAULT_DSH_WEB_ORIGIN).expect("default origin")
        })
    }

    /// Effective browser process list: an empty list is never allowed.
    pub fn browser_processes(&self) -> Vec<String> {
        if self.dsh_browser_processes.is_empty() {
            default_browser_processes()
        } else {
            self.dsh_browser_processes.clone()
        }
    }

    /// True when the workspace cross-check is enabled.
    pub fn workspace_check(&self) -> Option<&str> {
        let trimmed = self.workspace.trim();
        if trimmed.is_empty() {
            None
        } else {
            Some(trimmed)
        }
    }

    /// First-run signal: no usable DSH data directory yet.
    pub fn needs_dsh_data_home(&self) -> bool {
        self.dsh_data_home.trim().is_empty()
    }
}

/// A DSH install discovered on this machine, with where it was found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DetectedDsh {
    pub data_home: PathBuf,
    pub origin: Option<String>,
}

/// Describe the machine state a first run has to reason about.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct FirstRunState {
    pub config_exists: bool,
    pub bindings_exist: bool,
    pub templates_exist: bool,
}

impl FirstRunState {
    pub fn probe(product: &Path) -> Self {
        Self {
            config_exists: product.join(CONFIG_FILE).is_file(),
            bindings_exist: product.join("runtime/bindings.json").is_file(),
            templates_exist: product.join("runtime/message-templates.json").is_file(),
        }
    }
    /// True when this is a brand new installation.
    pub fn is_first_run(&self) -> bool {
        !self.config_exists
    }
}

/// What a first run created, so the UI and scripts can report it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FirstRunReport {
    pub config_written: bool,
    pub bindings_written: bool,
    pub templates_written: bool,
    pub detected: Option<DetectedDsh>,
}

/// The placeholder bindings written on first run. They are deliberately
/// unusable: `valid_bindings` rejects both prefixes, so nothing can be sent and the
/// UI must not present them as a real session.
pub const PLACEHOLDER_CLAUDE_SESSION: &str = "cse_unconfigured";
pub const PLACEHOLDER_DSH_SESSION: &str = "session-unconfigured";
pub const PLACEHOLDER_CLAUDE_TITLE: &str = "Claude Desktop";

pub fn placeholder_bindings() -> Value {
    serde_json::json!({
        "claude_session": PLACEHOLDER_CLAUDE_SESSION,
        "claude_title": PLACEHOLDER_CLAUDE_TITLE,
        "claude_window": PLACEHOLDER_CLAUDE_TITLE,
        "dsh_session": PLACEHOLDER_DSH_SESSION
    })
}

/// True when the bindings are still the first-run placeholders.
pub fn is_placeholder_binding(session: &str, prefix: &str) -> bool {
    session == PLACEHOLDER_CLAUDE_SESSION && prefix == "cse_"
        || session == PLACEHOLDER_DSH_SESSION && prefix == "session-"
}

/// Mirror of the scripts' placeholder check, for the UI.
pub fn bindings_are_placeholder(value: &Value) -> bool {
    let c = value["claude_session"].as_str().unwrap_or("");
    let d = value["dsh_session"].as_str().unwrap_or("");
    c == PLACEHOLDER_CLAUDE_SESSION || d == PLACEHOLDER_DSH_SESSION
}

/// The marker that identifies the DSH entry script inside one argument.
const DSH_ENTRY: &str = r"\runtime\node_modules\@deepseek-ai\dsh\lib\bin.js";

/// The install root of the DSH process described by a `node.exe` command line, or `None` when
/// the line is not one.
///
/// The process carries its paths on its own command line, and the node.exe running it is
/// normally an absolute path too:
///
/// ```text
/// "C:\Program Files\nodejs\node.exe" D:\apps\dsh\runtime\node_modules\@deepseek-ai\dsh\lib\bin.js web
/// ```
///
/// Scanning that for the first drive-lettered path and stretching it to the marker captures
/// node's own path, the quote between the two arguments and everything up to the marker, which
/// is not a path at all - first-run detection then quietly found nothing on a machine whose DSH
/// is launched by an absolute node path. Split the line into arguments the way Windows does and
/// test each one on its own. `scripts/dsh-detect.ps1` implements the same rule for bootstrap,
/// and both test suites use the same fixtures so a divergence fails on one side.
pub fn dsh_install_root(command_line: &str) -> Option<String> {
    for (index, segment) in command_line.split('"').enumerate() {
        // Odd segments sit between quotes, so each is exactly one argument and may contain
        // spaces. The even segments are the unquoted runs, which Windows splits on whitespace.
        let tokens: Vec<&str> = if index % 2 == 1 {
            vec![segment]
        } else {
            segment.split_whitespace().collect()
        };
        for token in tokens {
            if let Some(root) = dsh_root_from_argument(token) {
                return Some(root.to_owned());
            }
        }
    }
    None
}

fn dsh_root_from_argument(argument: &str) -> Option<&str> {
    // Windows paths are case-insensitive, so compare folded. ASCII folding keeps the byte
    // length, which is what makes the slice below land on a character boundary.
    if !argument.to_ascii_lowercase().ends_with(DSH_ENTRY) {
        return None;
    }
    let root = &argument[..argument.len() - DSH_ENTRY.len()];
    let bytes = root.as_bytes();
    let drive_rooted =
        bytes.len() >= 3 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':' && bytes[2] == b'\\';
    if !drive_rooted {
        return None;
    }
    Some(root)
}

/// Find a running DSH install by scanning node.exe command lines, the same way
/// `scripts/bootstrap.ps1` does. Returns the first usable data directory.
#[cfg(windows)]
pub fn detect_dsh() -> Option<DetectedDsh> {
    // PowerShell only fetches the command lines. The parsing rule stays above, where it can be
    // tested without a running DSH.
    let script = r#"$ErrorActionPreference='SilentlyContinue'
Get-CimInstance Win32_Process -Filter "Name='node.exe'" | ForEach-Object { [string]$_.CommandLine }"#;
    let out = std::process::Command::new("powershell.exe")
        .args(["-NoProfile", "-ExecutionPolicy", "Bypass", "-Command"])
        .arg(script)
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&out.stdout);
    for line in text.lines() {
        let Some(root) = dsh_install_root(line) else {
            continue;
        };
        if let Ok(data_home) = validate_dsh_data_home(&Path::new(&root).join("data")) {
            return Some(DetectedDsh {
                data_home,
                origin: None,
            });
        }
    }
    None
}

#[cfg(not(windows))]
pub fn detect_dsh() -> Option<DetectedDsh> {
    None
}

/// First-run provisioning. Creates only what is missing and never overwrites an
/// existing file, so it is safe to call on every launch.
pub fn first_run(product: &Path, detected: Option<DetectedDsh>) -> Result<FirstRunReport, String> {
    let state = FirstRunState::probe(product);
    let mut report = FirstRunReport {
        config_written: false,
        bindings_written: false,
        templates_written: false,
        detected: detected.clone(),
    };
    std::fs::create_dir_all(product.join("runtime")).map_err(|e| e.to_string())?;

    if !state.config_exists {
        let mut config = RuntimeConfig::default();
        if let Some(d) = &detected {
            config.dsh_data_home = d.data_home.to_string_lossy().into_owned();
            if let Some(origin) = &d.origin {
                config.dsh_web_origin = origin.clone();
            }
        }
        save(product, &config)?;
        report.config_written = true;
    }
    if !state.bindings_exist {
        let path = product.join("runtime/bindings.json");
        let mut text =
            serde_json::to_string_pretty(&placeholder_bindings()).map_err(|e| e.to_string())?;
        text.push('\n');
        std::fs::write(&path, text).map_err(|e| e.to_string())?;
        report.bindings_written = true;
    }
    if !state.templates_exist {
        // Copy the shipped example so users start from the documented text.
        let example = product.join("examples/message-templates.json");
        let target = product.join("runtime/message-templates.json");
        if let Ok(text) = std::fs::read_to_string(&example) {
            std::fs::write(&target, text).map_err(|e| e.to_string())?;
            report.templates_written = true;
        }
    }
    Ok(report)
}

/// Validate a candidate DSH data directory. Mirrors bootstrap.ps1 so the GUI and
/// the script accept exactly the same layout.
pub fn validate_dsh_data_home(path: &Path) -> Result<PathBuf, String> {
    if !path.is_dir() {
        return Err(format!("目录不存在：{}", path.display()));
    }
    if !path.join("storages/session_projcache/sessions").is_dir() {
        return Err(format!(
            "不是受支持的 DSH 数据目录（缺少 storages/session_projcache/sessions）：{}",
            path.display()
        ));
    }
    if !path.join("sessions").is_dir() {
        return Err(format!(
            "不是受支持的 DSH 数据目录（缺少 sessions 原生日志目录）：{}",
            path.display()
        ));
    }
    std::fs::canonicalize(path).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn loaded(v: Value) -> RuntimeConfig {
        from_value(&v).config
    }

    #[test]
    fn missing_fields_fall_back_to_defaults() {
        let c = loaded(json!({}));
        assert_eq!(c, RuntimeConfig::default());
        assert!(!c.enabled);
        assert_eq!(c.poll_seconds, 60);
        assert_eq!(c.dispatch_delay_seconds, 10);
        assert_eq!(c.dsh_web_origin, "http://127.0.0.1:3080");
        assert_eq!(c.dsh_browser_processes, vec!["msedge", "chrome"]);
        assert_eq!(c.claude_host, "claude.ai");
        assert_eq!(c.dsh_page_title_pattern, "DeepSeek Harness");
        assert_eq!(c.workspace, "");
        assert!(c.needs_dsh_data_home());
    }

    #[test]
    fn illegal_poll_and_delay_are_clamped_and_reported() {
        let out = from_value(&json!({"poll_seconds": 0, "dispatch_delay_seconds": 9999}));
        assert_eq!(out.config.poll_seconds, MIN_POLL_SECONDS);
        assert_eq!(
            out.config.dispatch_delay_seconds,
            MAX_DISPATCH_DELAY_SECONDS
        );
        assert!(out.notices.iter().any(|n| n.field == "poll_seconds"));
        assert!(out
            .notices
            .iter()
            .any(|n| n.field == "dispatch_delay_seconds"));

        let out = from_value(&json!({"poll_seconds": "soon", "dispatch_delay_seconds": -5}));
        assert_eq!(out.config.poll_seconds, 60);
        assert_eq!(out.config.dispatch_delay_seconds, 10);

        let out = from_value(&json!({"poll_seconds": 12.5}));
        assert_eq!(out.config.poll_seconds, 60);
    }

    #[test]
    fn illegal_dsh_url_is_rejected_and_defaulted() {
        for bad in [
            json!("127.0.0.1:3080"),
            json!("ftp://127.0.0.1:3080"),
            json!("http://"),
            json!(""),
            json!(42),
        ] {
            let out = from_value(&json!({"dsh_web_origin": bad}));
            assert_eq!(
                out.config.dsh_web_origin, DEFAULT_DSH_WEB_ORIGIN,
                "bad origin {bad} must not be accepted"
            );
            assert!(out.notices.iter().any(|n| n.field == "dsh_web_origin"));
        }
    }

    #[test]
    fn origin_parsing_normalizes_and_keeps_the_port() {
        let o = DshWebOrigin::parse("HTTP://LocalHost:3080/some/path?q=1").unwrap();
        assert_eq!(o.scheme, "http");
        assert_eq!(o.host, "localhost");
        assert_eq!(o.port, 3080);
        assert_eq!(o.to_origin_string(), "http://localhost:3080");

        let o = DshWebOrigin::parse("https://dsh.example.com").unwrap();
        assert_eq!(
            (o.scheme.as_str(), o.host.as_str(), o.port),
            ("https", "dsh.example.com", 443)
        );
        let o = DshWebOrigin::parse("http://127.0.0.1").unwrap();
        assert_eq!(o.port, 80);
        assert!(DshWebOrigin::parse("http://127.0.0.1:99999").is_err());
    }

    #[test]
    fn empty_browser_list_falls_back() {
        for bad in [json!([]), json!(["", "  "]), json!("msedge"), json!([1, 2])] {
            let out = from_value(&json!({"dsh_browser_processes": bad}));
            assert_eq!(
                out.config.dsh_browser_processes,
                vec!["msedge", "chrome"],
                "bad list {bad} must fall back"
            );
        }
        let c = loaded(json!({"dsh_browser_processes": ["Brave", " firefox "]}));
        assert_eq!(c.dsh_browser_processes, vec!["brave", "firefox"]);
        assert!(RuntimeConfig::default().browser_processes().len() == 2);
    }

    #[test]
    fn empty_workspace_means_the_extra_check_is_off() {
        let c = loaded(json!({"workspace": "   "}));
        assert_eq!(c.workspace_check(), None);
        let c = loaded(json!({"workspace": "D:\\\\proj"}));
        assert_eq!(c.workspace_check(), Some("D:\\\\proj"));
    }

    #[test]
    fn legacy_config_with_deprecated_keys_still_loads() {
        let out = from_value(&json!({
            "enabled": true,
            "poll_seconds": 30,
            "dispatch_delay_seconds": 5,
            "dsh_data_home": "C:\\\\dsh\\\\data",
            "task_file": "D:\\\\x\\\\DSH_TASKS.MD",
            "mode": "live",
            "next_poll_at_ms": 1790762911361u64,
            "workspace": "D:\\\\proj"
        }));
        assert!(out.config.enabled);
        assert_eq!(out.config.poll_seconds, 30);
        assert_eq!(out.config.dispatch_delay_seconds, 5);
        assert_eq!(out.config.dsh_data_home, "C:\\\\dsh\\\\data");
        // The deprecated keys are reported but harmless.
        for key in DEPRECATED_KEYS {
            assert!(
                out.notices.iter().any(|n| n.field == key),
                "{key} should be reported as ignored"
            );
        }
    }

    #[test]
    fn save_never_writes_deprecated_keys_back() {
        let product = std::env::temp_dir().join(format!(
            "a2a-config-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(product.join("runtime")).unwrap();
        std::fs::write(
            product.join(CONFIG_FILE),
            json!({
                "enabled": false,
                "poll_seconds": 60,
                "dispatch_delay_seconds": 10,
                "dsh_data_home": "C:\\\\dsh\\\\data",
                "task_file": "gone",
                "mode": "live",
                "next_poll_at_ms": 123
            })
            .to_string(),
        )
        .unwrap();

        let loaded = load(&product);
        save(&product, &loaded.config).unwrap();

        let text = std::fs::read_to_string(product.join(CONFIG_FILE)).unwrap();
        let value: Value = serde_json::from_str(&text).unwrap();
        for key in DEPRECATED_KEYS {
            assert!(value.get(key).is_none(), "{key} must not be persisted");
        }
        assert_eq!(
            value.as_object().unwrap().len(),
            9,
            "schema must have exactly 9 fields: {text}"
        );
        // Round-trips unchanged.
        assert_eq!(load(&product).config, loaded.config);

        let _ = std::fs::remove_dir_all(&product);
    }

    #[test]
    fn broken_json_still_produces_a_usable_config() {
        let out = from_json_str("{not json");
        assert_eq!(out.config, RuntimeConfig::default());
        assert!(out.notices.iter().any(|n| n.field == "config"));
        // BOM is tolerated.
        let out = from_json_str("\u{feff}{\"poll_seconds\": 45}");
        assert_eq!(out.config.poll_seconds, 45);
    }

    #[test]
    fn validation_reports_every_problem() {
        let c = RuntimeConfig {
            poll_seconds: 0,
            dispatch_delay_seconds: 0,
            dsh_web_origin: "nope".into(),
            claude_host: "".into(),
            dsh_page_title_pattern: " ".into(),
            dsh_data_home: "D:\\\\definitely\\\\missing".into(),
            ..RuntimeConfig::default()
        };
        let problems = c.validate();
        assert!(problems.len() >= 5, "{problems:?}");
        assert!(RuntimeConfig::default().validate().is_empty());
    }

    fn temp_product(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "a2a-firstrun-{tag}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(dir.join("examples")).unwrap();
        std::fs::write(
            dir.join("examples/message-templates.json"),
            "{\"version\":1,\"to_dsh\":{\"prefix\":\"\",\"suffix\":\"\"},\"to_claude\":{\"prefix\":\"p\",\"suffix\":\"s\"}}",
        )
        .unwrap();
        dir
    }

    #[test]
    fn first_run_creates_config_placeholders_and_templates() {
        let product = temp_product("fresh");
        let state = FirstRunState::probe(&product);
        assert!(state.is_first_run());

        let report = first_run(&product, None).unwrap();
        assert!(report.config_written && report.bindings_written && report.templates_written);

        // Config uses the documented defaults.
        let value: Value =
            serde_json::from_str(&std::fs::read_to_string(product.join(CONFIG_FILE)).unwrap())
                .unwrap();
        assert_eq!(value["enabled"], false);
        assert_eq!(value["poll_seconds"], 60);
        assert_eq!(value["dispatch_delay_seconds"], 10);
        assert_eq!(value["dsh_web_origin"], DEFAULT_DSH_WEB_ORIGIN);
        assert_eq!(value["dsh_browser_processes"], json!(["msedge", "chrome"]));
        assert_eq!(value["claude_host"], DEFAULT_CLAUDE_HOST);
        assert_eq!(
            value["dsh_page_title_pattern"],
            DEFAULT_DSH_PAGE_TITLE_PATTERN
        );
        assert_eq!(value.as_object().unwrap().len(), 9);

        // Placeholder bindings: present, and unusable on purpose.
        let bindings: Value = serde_json::from_str(
            &std::fs::read_to_string(product.join("runtime/bindings.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(bindings["claude_session"], PLACEHOLDER_CLAUDE_SESSION);
        assert_eq!(bindings["dsh_session"], PLACEHOLDER_DSH_SESSION);
        assert!(bindings_are_placeholder(&bindings));

        // Templates were copied verbatim.
        assert!(
            std::fs::read_to_string(product.join("runtime/message-templates.json"))
                .unwrap()
                .contains("\"prefix\":\"p\"")
        );

        // A second run changes nothing.
        let again = first_run(&product, None).unwrap();
        assert!(!again.config_written && !again.bindings_written && !again.templates_written);

        let _ = std::fs::remove_dir_all(&product);
    }

    #[test]
    fn first_run_never_overwrites_existing_files() {
        let product = temp_product("keep");
        std::fs::create_dir_all(product.join("runtime")).unwrap();
        let mine = to_json_string(&RuntimeConfig {
            poll_seconds: 123,
            claude_host: "claude.example".into(),
            ..RuntimeConfig::default()
        });
        std::fs::write(product.join(CONFIG_FILE), &mine).unwrap();
        std::fs::write(product.join("runtime/bindings.json"), "{\"mine\":true}").unwrap();
        std::fs::write(
            product.join("runtime/message-templates.json"),
            "{\"mine\":true}",
        )
        .unwrap();

        let report = first_run(&product, None).unwrap();
        assert!(!report.config_written && !report.bindings_written && !report.templates_written);
        assert_eq!(
            std::fs::read_to_string(product.join(CONFIG_FILE)).unwrap(),
            mine
        );
        assert_eq!(
            std::fs::read_to_string(product.join("runtime/bindings.json")).unwrap(),
            "{\"mine\":true}"
        );
        assert_eq!(
            std::fs::read_to_string(product.join("runtime/message-templates.json")).unwrap(),
            "{\"mine\":true}"
        );

        let _ = std::fs::remove_dir_all(&product);
    }

    #[test]
    fn first_run_uses_a_detected_data_home_when_available() {
        let product = temp_product("detected");
        let data = product.join("dsh-data");
        std::fs::create_dir_all(data.join("storages/session_projcache/sessions")).unwrap();
        std::fs::create_dir_all(data.join("sessions")).unwrap();
        let detected = DetectedDsh {
            data_home: validate_dsh_data_home(&data).unwrap(),
            origin: Some("http://127.0.0.1:9999".into()),
        };
        let report = first_run(&product, Some(detected)).unwrap();
        assert!(report.config_written);
        let loaded = load(&product).config;
        // Canonicalized on the way in, so compare against the canonical form.
        let expected = std::fs::canonicalize(&data).unwrap();
        assert_eq!(
            std::fs::canonicalize(Path::new(&loaded.dsh_data_home)).unwrap(),
            expected
        );
        assert_eq!(loaded.dsh_web_origin, "http://127.0.0.1:9999");
        assert!(!loaded.needs_dsh_data_home());
        let _ = std::fs::remove_dir_all(&product);
    }

    #[test]
    fn data_home_validation_rejects_incomplete_layouts() {
        let product = temp_product("layout");
        // Missing everything.
        assert!(validate_dsh_data_home(&product.join("nope")).is_err());
        // Present but not a DSH layout.
        let plain = product.join("plain");
        std::fs::create_dir_all(&plain).unwrap();
        assert!(validate_dsh_data_home(&plain).is_err());
        // Missing the native sessions directory.
        let partial = product.join("partial");
        std::fs::create_dir_all(partial.join("storages/session_projcache/sessions")).unwrap();
        assert!(validate_dsh_data_home(&partial).is_err());
        // Complete layout is accepted.
        std::fs::create_dir_all(partial.join("sessions")).unwrap();
        assert!(validate_dsh_data_home(&partial).is_ok());
        let _ = std::fs::remove_dir_all(&product);
    }

    #[test]
    fn placeholder_sessions_are_not_valid_bindings() {
        // `valid_bindings` in the runner requires the real prefixes, so the
        // placeholders can never be used to send anything.
        assert!(is_placeholder_binding(PLACEHOLDER_CLAUDE_SESSION, "cse_"));
        assert!(is_placeholder_binding(PLACEHOLDER_DSH_SESSION, "session-"));
        assert!(!is_placeholder_binding("cse_real_123", "cse_"));
    }

    // The same fixtures appear in tests/test-bootstrap-firstrun.ps1, so the PowerShell rule in
    // scripts/dsh-detect.ps1 and this one cannot drift apart unnoticed.

    #[test]
    fn a_quoted_node_path_does_not_swallow_the_dsh_root() {
        let spaced = r#""C:\Program Files\nodejs\node.exe" D:\apps\dsh\runtime\node_modules\@deepseek-ai\dsh\lib\bin.js web --host 127.0.0.1 --port 3080"#;
        assert_eq!(dsh_install_root(spaced).as_deref(), Some(r"D:\apps\dsh"));
        let plain = r#""C:\nodejs\node.exe" D:\apps\dsh\runtime\node_modules\@deepseek-ai\dsh\lib\bin.js web"#;
        assert_eq!(dsh_install_root(plain).as_deref(), Some(r"D:\apps\dsh"));
    }

    #[test]
    fn the_dsh_entry_may_be_the_quoted_argument() {
        let line = r#"node "D:\apps\dsh\runtime\node_modules\@deepseek-ai\dsh\lib\bin.js" web"#;
        assert_eq!(dsh_install_root(line).as_deref(), Some(r"D:\apps\dsh"));
    }

    #[test]
    fn neither_argument_needs_quoting() {
        let line =
            r"D:\nodejs\node.exe D:\apps\dsh\runtime\node_modules\@deepseek-ai\dsh\lib\bin.js web";
        assert_eq!(dsh_install_root(line).as_deref(), Some(r"D:\apps\dsh"));
    }

    #[test]
    fn another_dsh_process_is_not_the_install_root() {
        let runner = r#""C:\Program Files\nodejs\node.exe" D:\apps\dsh\runtime\node_modules\@deepseek-ai\dsh-subprocess-local\lib\runner.js -- cmd"#;
        assert_eq!(dsh_install_root(runner), None);
        assert_eq!(dsh_install_root(""), None);
    }

    #[test]
    fn a_relative_entry_script_is_not_an_install_root() {
        let line = r"node runtime\node_modules\@deepseek-ai\dsh\lib\bin.js web";
        assert_eq!(dsh_install_root(line), None);
    }
}
