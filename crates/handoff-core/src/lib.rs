//! Cross-platform handoff state machine.
//! Platform automation belongs behind adapters.

pub mod config;

use std::collections::HashSet;
use std::hash::{Hash, Hasher};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    ClaudeToDsh,
    DshToClaude,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AutomationOwnership {
    Tool,
    User,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HandoffAnchor {
    pub target_session: String,
    pub tool_input_fingerprint: u64,
    pub target_user_message_index: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConversationEvidence {
    pub ownership: AutomationOwnership,
    pub latest_user_fingerprint: Option<u64>,
    pub latest_user_message_index: Option<u64>,
    pub latest_assistant_message_index: Option<u64>,
    pub anchor: Option<HandoffAnchor>,
}

impl ConversationEvidence {
    pub fn correlation_confirmed(&self) -> bool {
        let Some(anchor) = &self.anchor else {
            return false;
        };
        self.ownership == AutomationOwnership::Tool
            && self.latest_user_fingerprint == Some(anchor.tool_input_fingerprint)
            && self.latest_user_message_index == Some(anchor.target_user_message_index)
            && self
                .latest_assistant_message_index
                .map(|i| i > anchor.target_user_message_index)
                .unwrap_or(false)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PlannedAction {
    Send {
        direction: Direction,
        fingerprint: u64,
    },
    None {
        reason: &'static str,
    },
    Hold {
        reason: &'static str,
    },
    PausedByUser {
        reason: &'static str,
    },
}

#[derive(Debug, Default)]
pub struct DeliveryLedger {
    claude_to_dsh: HashSet<u64>,
    dsh_to_claude: HashSet<u64>,
}
impl DeliveryLedger {
    pub fn has(&self, d: Direction, fp: u64) -> bool {
        match d {
            Direction::ClaudeToDsh => self.claude_to_dsh.contains(&fp),
            Direction::DshToClaude => self.dsh_to_claude.contains(&fp),
        }
    }
    pub fn record(&mut self, d: Direction, fp: u64) {
        match d {
            Direction::ClaudeToDsh => {
                self.claude_to_dsh.insert(fp);
            }
            Direction::DshToClaude => {
                self.dsh_to_claude.insert(fp);
            }
        }
    }
}

pub fn fingerprint(text: &str) -> u64 {
    let normalized = text.replace("\r\n", "\n").trim().to_owned();
    let mut h = std::collections::hash_map::DefaultHasher::new();
    normalized.hash(&mut h);
    h.finish()
}

pub struct PollInput<'a> {
    pub dsh_busy: bool,
    pub dsh_result: Option<&'a str>,
    pub dsh_result_complete: bool,
    pub claude_reply: Option<&'a str>,
    pub claude_reply_complete: bool,
    pub claude_conversation: Option<&'a ConversationEvidence>,
}

pub fn plan_one(input: PollInput<'_>, ledger: &DeliveryLedger) -> PlannedAction {
    if let Some(body) = input.dsh_result {
        if !input.dsh_result_complete {
            return PlannedAction::Hold {
                reason: "dsh_result_incomplete",
            };
        }
        let fp = fingerprint(body);
        if !ledger.has(Direction::DshToClaude, fp) {
            return PlannedAction::Send {
                direction: Direction::DshToClaude,
                fingerprint: fp,
            };
        }
    }
    if input.dsh_busy {
        return PlannedAction::None { reason: "dsh_busy" };
    }
    if let Some(body) = input.claude_reply {
        if !input.claude_reply_complete {
            return PlannedAction::Hold {
                reason: "claude_reply_incomplete",
            };
        }
        let Some(conv) = input.claude_conversation else {
            return PlannedAction::Hold {
                reason: "claude_correlation_missing",
            };
        };
        match conv.ownership {
            AutomationOwnership::User => {
                return PlannedAction::PausedByUser {
                    reason: "claude_manual_intervention",
                }
            }
            AutomationOwnership::Unknown => {
                return PlannedAction::Hold {
                    reason: "claude_ownership_unknown",
                }
            }
            AutomationOwnership::Tool => {}
        }
        if !conv.correlation_confirmed() {
            return PlannedAction::Hold {
                reason: "claude_reply_not_correlated_to_tool_input",
            };
        }
        let fp = fingerprint(body);
        if !ledger.has(Direction::ClaudeToDsh, fp) {
            return PlannedAction::Send {
                direction: Direction::ClaudeToDsh,
                fingerprint: fp,
            };
        }
        return PlannedAction::None {
            reason: "claude_reply_already_dispatched",
        };
    }
    PlannedAction::None {
        reason: "no_new_handoff",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn tool_conv(input: &str, user: u64, assistant: u64) -> ConversationEvidence {
        ConversationEvidence {
            ownership: AutomationOwnership::Tool,
            latest_user_fingerprint: Some(fingerprint(input)),
            latest_user_message_index: Some(user),
            latest_assistant_message_index: Some(assistant),
            anchor: Some(HandoffAnchor {
                target_session: "claude".into(),
                tool_input_fingerprint: fingerprint(input),
                target_user_message_index: user,
            }),
        }
    }
    fn poll<'a>(reply: Option<&'a str>, conv: Option<&'a ConversationEvidence>) -> PollInput<'a> {
        PollInput {
            dsh_busy: false,
            dsh_result: None,
            dsh_result_complete: true,
            claude_reply: reply,
            claude_reply_complete: true,
            claude_conversation: conv,
        }
    }

    #[test]
    fn correlated_tool_reply_can_go_to_dsh() {
        let l = DeliveryLedger::default();
        let c = tool_conv("DSH result A", 20, 21);
        assert!(matches!(
            plan_one(poll(Some("Claude B"), Some(&c)), &l),
            PlannedAction::Send {
                direction: Direction::ClaudeToDsh,
                ..
            }
        ));
    }
    #[test]
    fn manual_user_message_pauses_automation() {
        let l = DeliveryLedger::default();
        let mut c = tool_conv("DSH result A", 20, 21);
        c.ownership = AutomationOwnership::User;
        c.latest_user_fingerprint = Some(fingerprint("user changed plan"));
        c.latest_user_message_index = Some(22);
        c.latest_assistant_message_index = Some(23);
        assert_eq!(
            plan_one(poll(Some("Claude answer to user"), Some(&c)), &l),
            PlannedAction::PausedByUser {
                reason: "claude_manual_intervention"
            }
        );
    }
    #[test]
    fn changed_user_message_without_explicit_ownership_holds() {
        let l = DeliveryLedger::default();
        let mut c = tool_conv("DSH result A", 20, 23);
        c.latest_user_fingerprint = Some(fingerprint("unexpected"));
        c.latest_user_message_index = Some(22);
        assert_eq!(
            plan_one(poll(Some("reply"), Some(&c)), &l),
            PlannedAction::Hold {
                reason: "claude_reply_not_correlated_to_tool_input"
            }
        );
    }
    #[test]
    fn unknown_ownership_holds() {
        let l = DeliveryLedger::default();
        let mut c = tool_conv("A", 1, 2);
        c.ownership = AutomationOwnership::Unknown;
        assert_eq!(
            plan_one(poll(Some("B"), Some(&c)), &l),
            PlannedAction::Hold {
                reason: "claude_ownership_unknown"
            }
        );
    }
    #[test]
    fn explicit_resume_creates_new_tool_anchor() {
        let l = DeliveryLedger::default();
        let c = tool_conv("user-approved-current-claude-reply", 30, 31);
        assert!(matches!(
            plan_one(poll(Some("new reply"), Some(&c)), &l),
            PlannedAction::Send { .. }
        ));
    }
    #[test]
    fn repeated_claude_reply_is_blocked() {
        let mut l = DeliveryLedger::default();
        let c = tool_conv("A", 10, 11);
        l.record(Direction::ClaudeToDsh, fingerprint("B"));
        assert_eq!(
            plan_one(poll(Some("B"), Some(&c)), &l),
            PlannedAction::None {
                reason: "claude_reply_already_dispatched"
            }
        );
    }
    #[test]
    fn dsh_result_has_priority() {
        let l = DeliveryLedger::default();
        let c = tool_conv("A", 1, 2);
        let p = PollInput {
            dsh_busy: false,
            dsh_result: Some("DSH"),
            dsh_result_complete: true,
            claude_reply: Some("Claude"),
            claude_reply_complete: true,
            claude_conversation: Some(&c),
        };
        assert!(matches!(
            plan_one(p, &l),
            PlannedAction::Send {
                direction: Direction::DshToClaude,
                ..
            }
        ));
    }
    #[test]
    fn dsh_busy_blocks_claude_dispatch() {
        let l = DeliveryLedger::default();
        let c = tool_conv("A", 1, 2);
        let mut p = poll(Some("B"), Some(&c));
        p.dsh_busy = true;
        assert_eq!(plan_one(p, &l), PlannedAction::None { reason: "dsh_busy" });
    }
    #[test]
    fn incomplete_evidence_holds() {
        let l = DeliveryLedger::default();
        let p = PollInput {
            dsh_busy: false,
            dsh_result: Some("partial"),
            dsh_result_complete: false,
            claude_reply: None,
            claude_reply_complete: true,
            claude_conversation: None,
        };
        assert_eq!(
            plan_one(p, &l),
            PlannedAction::Hold {
                reason: "dsh_result_incomplete"
            }
        );
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PendingDispatch {
    pub direction: Direction,
    pub fingerprint: u64,
    pub created_at_ms: u64,
    pub deadline_ms: u64,
    pub cancelled: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DispatchGate {
    Waiting { remaining_ms: u64 },
    Ready,
    CancelledByUser,
    Invalidated { reason: &'static str },
}

impl PendingDispatch {
    pub fn new(direction: Direction, fingerprint: u64, now_ms: u64, delay_ms: u64) -> Self {
        Self {
            direction,
            fingerprint,
            created_at_ms: now_ms,
            deadline_ms: now_ms.saturating_add(delay_ms),
            cancelled: false,
        }
    }
    pub fn cancel(&mut self) {
        self.cancelled = true;
    }
    pub fn gate(&self, now_ms: u64, current_fingerprint: u64, still_allowed: bool) -> DispatchGate {
        if self.cancelled {
            return DispatchGate::CancelledByUser;
        }
        if !still_allowed {
            return DispatchGate::Invalidated {
                reason: "preconditions_changed",
            };
        }
        if current_fingerprint != self.fingerprint {
            return DispatchGate::Invalidated {
                reason: "message_changed_during_countdown",
            };
        }
        if now_ms < self.deadline_ms {
            return DispatchGate::Waiting {
                remaining_ms: self.deadline_ms - now_ms,
            };
        }
        DispatchGate::Ready
    }
}

#[cfg(test)]
mod countdown_tests {
    use super::*;
    #[test]
    fn countdown_waits_full_ten_seconds() {
        let p = PendingDispatch::new(Direction::ClaudeToDsh, 42, 1000, 10_000);
        assert_eq!(
            p.gate(10_999, 42, true),
            DispatchGate::Waiting { remaining_ms: 1 }
        );
        assert_eq!(p.gate(11_000, 42, true), DispatchGate::Ready);
    }
    #[test]
    fn user_can_cancel_before_deadline() {
        let mut p = PendingDispatch::new(Direction::DshToClaude, 7, 0, 10_000);
        p.cancel();
        assert_eq!(p.gate(10_001, 7, true), DispatchGate::CancelledByUser);
    }
    #[test]
    fn changed_message_invalidates_pending_send() {
        let p = PendingDispatch::new(Direction::ClaudeToDsh, 7, 0, 10_000);
        assert_eq!(
            p.gate(10_001, 8, true),
            DispatchGate::Invalidated {
                reason: "message_changed_during_countdown"
            }
        );
    }
    #[test]
    fn changed_ownership_or_target_invalidates_pending_send() {
        let p = PendingDispatch::new(Direction::ClaudeToDsh, 7, 0, 10_000);
        assert_eq!(
            p.gate(10_001, 7, false),
            DispatchGate::Invalidated {
                reason: "preconditions_changed"
            }
        );
    }
}

pub mod message_template;
