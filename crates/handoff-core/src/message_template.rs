//! Pure message composition shared by the UI preview and the runtime.
use crate::Direction;
use serde::{Deserialize, Serialize};

pub const MAX_TEMPLATE_UNITS: usize = 16_000;
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MessageAffixes {
    pub prefix: String,
    pub suffix: String,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MessageTemplates {
    pub version: u32,
    pub to_dsh: MessageAffixes,
    pub to_claude: MessageAffixes,
}
impl Default for MessageTemplates {
    fn default() -> Self {
        Self {
            version: 1,
            to_dsh: MessageAffixes {
                prefix: "读取 DSH_TASKS.MD 并执行。".into(),
                suffix: String::new(),
            },
            to_claude: MessageAffixes {
                prefix: "DSH 已回复:
["
                .into(),
                suffix: "]

如需继续，请更新任务文件。需要我作决定时，请停下来问我。"
                    .into(),
            },
        }
    }
}
impl MessageTemplates {
    pub fn validate(&self) -> Result<(), String> {
        if self.version != 1 {
            return Err("不支持的交接文案配置版本。".into());
        }
        for value in [
            &self.to_dsh.prefix,
            &self.to_dsh.suffix,
            &self.to_claude.prefix,
            &self.to_claude.suffix,
        ] {
            if value.encode_utf16().count() > MAX_TEMPLATE_UNITS {
                return Err("每段文案最多 16000 个 UTF-16 字符。".into());
            }
            if value
                .chars()
                .any(|c| c.is_control() && c != '\n' && c != '\r' && c != '\t')
            {
                return Err("文案包含不支持的控制字符。".into());
            }
        }
        Ok(())
    }
    /// The delivery id remains local. DSH receives only the user's configured instruction.
    pub fn render(&self, direction: Direction, _id: &str, body: &str, turn: u64) -> String {
        let a = match direction {
            Direction::ClaudeToDsh => &self.to_dsh,
            Direction::DshToClaude => &self.to_claude,
        };
        let expand = |s: &str| s.replace("{{source_turn}}", &turn.to_string());
        let mut parts = Vec::new();
        if !a.prefix.is_empty() {
            parts.push(expand(&a.prefix));
        }
        if direction == Direction::DshToClaude {
            parts.push(body.to_owned());
        }
        if !a.suffix.is_empty() {
            parts.push(expand(&a.suffix));
        }
        parts.join(
            "

",
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn dsh_gets_only_configured_instruction_and_filename() {
        let mut t = MessageTemplates::default();
        t.to_dsh.prefix = "读取 docs/NEXT_TASK.md 并执行。".into();
        t.to_dsh.suffix = "执行完后停下。".into();
        assert_eq!(
            t.render(
                Direction::ClaudeToDsh,
                "internal-id",
                "CLAUDE_FULL_REPLY",
                7
            ),
            "读取 docs/NEXT_TASK.md 并执行。

执行完后停下。"
        );
    }
    #[test]
    fn claude_body_is_verbatim_and_uses_ascii_brackets() {
        let mut t = MessageTemplates::default();
        let body = "  原文\r
{{source_turn}}
```rust
{}
```
";
        t.to_claude = MessageAffixes {
            prefix: "[".into(),
            suffix: "]".into(),
        };
        assert_eq!(
            t.render(Direction::DshToClaude, "internal-id", body, 7),
            format!(
                "[

{body}

]"
            )
        );
        t.to_claude = MessageAffixes {
            prefix: "<".into(),
            suffix: ">".into(),
        };
        assert_eq!(
            t.render(Direction::DshToClaude, "internal-id", body, 7),
            format!(
                "<

{body}

>"
            )
        );
    }
    #[test]
    fn no_visible_identity_or_preview_is_injected() {
        let t = MessageTemplates::default();
        for d in [Direction::ClaudeToDsh, Direction::DshToClaude] {
            let out = t.render(d, "preview", "ORIGINAL", 7);
            for forbidden in [
                "A2AHandoff",
                "preview",
                "A2A_READY",
                "A2A_DONE",
                "第 7 轮",
                "【",
                "】",
                "（",
                "）",
            ] {
                assert!(
                    !out.contains(forbidden),
                    "unexpected injected text: {forbidden}"
                );
            }
        }
    }
    #[test]
    fn empty_affixes_do_not_restore_claude_body_in_dsh_direction() {
        let mut t = MessageTemplates::default();
        t.to_dsh = MessageAffixes {
            prefix: String::new(),
            suffix: String::new(),
        };
        t.to_claude = t.to_dsh.clone();
        assert_eq!(
            t.render(Direction::ClaudeToDsh, "id", "CLAUDE_REPLY", 0),
            ""
        );
        assert_eq!(
            t.render(Direction::DshToClaude, "id", "DSH_REPLY", 0),
            "DSH_REPLY"
        );
    }
    #[test]
    fn validates_version_length_and_controls() {
        let mut t = MessageTemplates::default();
        assert!(t.validate().is_ok());
        t.version = 2;
        assert!(t.validate().is_err());
        t.version = 1;
        t.to_dsh.prefix = "x".repeat(MAX_TEMPLATE_UNITS + 1);
        assert!(t.validate().is_err());
        t.to_dsh.prefix = "a\0b".into();
        assert!(t.validate().is_err());
        t.to_dsh.prefix = "\r\n\t可编辑".into();
        assert!(t.validate().is_ok());
    }
}
