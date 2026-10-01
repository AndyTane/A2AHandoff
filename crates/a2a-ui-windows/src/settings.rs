//! Pure form model for the Settings dialog.
//!
//! Kept free of Win32 so the load → edit → save round-trip and the validation
//! behaviour can be unit tested. The dialog in `settings_dialog.rs` only binds
//! these values to `EDIT` controls.
use handoff_core::config::{self, RuntimeConfig};

/// The eight editable fields, as the user sees them.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SettingsForm {
    pub dsh_data_home: String,
    pub dsh_web_origin: String,
    pub poll_seconds: String,
    pub dispatch_delay_seconds: String,
    pub dsh_browser_processes: String,
    pub workspace: String,
    pub claude_host: String,
    pub dsh_page_title_pattern: String,
}

impl SettingsForm {
    /// Fill the form from a loaded config.
    pub fn from_config(c: &RuntimeConfig) -> Self {
        Self {
            dsh_data_home: c.dsh_data_home.clone(),
            dsh_web_origin: c.dsh_web_origin.clone(),
            poll_seconds: c.poll_seconds.to_string(),
            dispatch_delay_seconds: c.dispatch_delay_seconds.to_string(),
            dsh_browser_processes: c.browser_processes().join(", "),
            workspace: c.workspace.clone(),
            claude_host: c.claude_host.clone(),
            dsh_page_title_pattern: c.dsh_page_title_pattern.clone(),
        }
    }

    /// Apply the form on top of an existing config. `enabled` and any field the
    /// form does not own are preserved, so saving settings can never change the
    /// automatic-handoff switch.
    pub fn apply(&self, base: &RuntimeConfig) -> Result<RuntimeConfig, String> {
        let mut next = base.clone();
        next.dsh_data_home = self.dsh_data_home.trim().to_owned();
        next.dsh_web_origin = self.dsh_web_origin.trim().to_owned();
        next.poll_seconds = parse_u64(&self.poll_seconds, "自动轮询周期")?;
        next.dispatch_delay_seconds = parse_u64(&self.dispatch_delay_seconds, "发送倒计时")?;
        next.claude_host = self.claude_host.trim().to_ascii_lowercase();
        next.dsh_page_title_pattern = self.dsh_page_title_pattern.trim().to_owned();
        next.workspace = self.workspace.trim().to_owned();
        next.dsh_browser_processes = parse_browser_list(&self.dsh_browser_processes);

        // Normalize the origin so the stored value and the field agree, and so the
        // adapters never see a value they would reject.
        match config::DshWebOrigin::parse(&next.dsh_web_origin) {
            Ok(origin) => next.dsh_web_origin = origin.to_origin_string(),
            Err(e) => return Err(e),
        }
        let problems = next.validate();
        if problems.is_empty() {
            Ok(next)
        } else {
            Err(problems.join("\n"))
        }
    }
}

fn parse_u64(text: &str, label: &str) -> Result<u64, String> {
    text.trim()
        .parse::<u64>()
        .map_err(|_| format!("{label}必须是整数。"))
}

/// Comma / semicolon / full-width separated list; an empty list falls back to the
/// documented default pair rather than disabling browser detection.
pub fn parse_browser_list(text: &str) -> Vec<String> {
    let names: Vec<String> = text
        .split([',', '，', ';', '；'])
        .map(|s| s.trim().to_ascii_lowercase())
        .filter(|s| !s.is_empty())
        .collect();
    if names.is_empty() {
        config::default_browser_processes()
    } else {
        names
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use handoff_core::config::RuntimeConfig;
    use serde_json::json;

    fn loaded(value: serde_json::Value) -> RuntimeConfig {
        config::from_value(&value).config
    }

    #[test]
    fn form_loads_every_field_from_config() {
        let c = loaded(json!({
            "poll_seconds": 45,
            "dispatch_delay_seconds": 7,
            "dsh_data_home": "C:\\\\dsh\\\\data",
            "dsh_web_origin": "http://localhost:3080",
            "dsh_browser_processes": ["brave"],
            "workspace": "D:\\\\proj",
            "claude_host": "claude.example",
            "dsh_page_title_pattern": "My DSH"
        }));
        let f = SettingsForm::from_config(&c);
        assert_eq!(f.poll_seconds, "45");
        assert_eq!(f.dispatch_delay_seconds, "7");
        assert_eq!(f.dsh_data_home, "C:\\\\dsh\\\\data");
        assert_eq!(f.dsh_web_origin, "http://localhost:3080");
        assert_eq!(f.dsh_browser_processes, "brave");
        assert_eq!(f.workspace, "D:\\\\proj");
        assert_eq!(f.claude_host, "claude.example");
        assert_eq!(f.dsh_page_title_pattern, "My DSH");
    }

    #[test]
    fn form_round_trips_unchanged_and_preserves_enabled() {
        let base = loaded(json!({"enabled": true, "poll_seconds": 30}));
        let form = SettingsForm::from_config(&base);
        let saved = form.apply(&base).unwrap();
        assert_eq!(saved, base, "an untouched form must not change the config");
        assert!(
            saved.enabled,
            "settings must never touch the automatic switch"
        );
    }

    #[test]
    fn editing_fields_updates_only_those_fields() {
        let base = loaded(json!({"enabled": true}));
        let mut form = SettingsForm::from_config(&base);
        form.poll_seconds = " 90 ".into();
        form.claude_host = "Claude.AI".into();
        form.dsh_browser_processes = " Brave , MSEDGE ".into();
        let saved = form.apply(&base).unwrap();
        assert_eq!(saved.poll_seconds, 90);
        assert_eq!(saved.claude_host, "claude.ai");
        assert_eq!(saved.dsh_browser_processes, vec!["brave", "msedge"]);
        // Untouched fields keep their values.
        assert_eq!(saved.dispatch_delay_seconds, base.dispatch_delay_seconds);
        assert_eq!(saved.dsh_web_origin, base.dsh_web_origin);
    }

    #[test]
    fn illegal_numbers_are_rejected_before_writing() {
        let base = RuntimeConfig::default();
        let mut form = SettingsForm::from_config(&base);
        for bad in ["", "abc", "1.5", "-3"] {
            form.poll_seconds = bad.into();
            assert!(form.apply(&base).is_err(), "{bad:?} must be rejected");
        }
        form.poll_seconds = "60".into();
        form.dispatch_delay_seconds = "0".into();
        assert!(
            form.apply(&base).is_err(),
            "delay out of range must be rejected"
        );
        form.dispatch_delay_seconds = "10".into();
        form.dsh_web_origin = "not-a-url".into();
        assert!(form.apply(&base).is_err(), "bad origin must be rejected");
    }

    #[test]
    fn origin_is_normalized_on_save() {
        let base = RuntimeConfig::default();
        let mut form = SettingsForm::from_config(&base);
        form.dsh_web_origin = "HTTP://LocalHost:3080/".into();
        let saved = form.apply(&base).unwrap();
        assert_eq!(saved.dsh_web_origin, "http://localhost:3080");
    }

    #[test]
    fn empty_browser_list_falls_back_to_defaults() {
        assert_eq!(parse_browser_list(""), vec!["msedge", "chrome"]);
        assert_eq!(parse_browser_list("  ,  ; "), vec!["msedge", "chrome"]);
        assert_eq!(parse_browser_list("brave"), vec!["brave"]);
        assert_eq!(parse_browser_list("a，b;c"), vec!["a", "b", "c"]);
        let base = RuntimeConfig::default();
        let mut form = SettingsForm::from_config(&base);
        form.dsh_browser_processes = String::new();
        assert_eq!(
            form.apply(&base).unwrap().dsh_browser_processes,
            vec!["msedge", "chrome"]
        );
    }

    #[test]
    fn empty_workspace_and_optional_fields_are_allowed() {
        let base = RuntimeConfig::default();
        let mut form = SettingsForm::from_config(&base);
        form.workspace = "   ".into();
        form.dsh_data_home = String::new();
        let saved = form.apply(&base).unwrap();
        assert_eq!(saved.workspace, "");
        assert_eq!(saved.workspace_check(), None);
        assert!(saved.needs_dsh_data_home());
    }

    #[test]
    fn empty_required_text_is_rejected() {
        let base = RuntimeConfig::default();
        let mut form = SettingsForm::from_config(&base);
        form.claude_host = "  ".into();
        assert!(form.apply(&base).is_err());
        form.claude_host = "claude.ai".into();
        form.dsh_page_title_pattern = "".into();
        assert!(form.apply(&base).is_err());
    }
}
