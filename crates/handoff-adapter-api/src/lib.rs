//! Stable client adapter contracts for Agent Handoff V1.

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MessageObservation {
    pub session_id: String,
    pub content: String,
    pub native_turn: Option<u64>,
    pub complete: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AgentActivity {
    Idle,
    Busy,
    Unknown,
}

pub trait AgentAdapter {
    fn adapter_id(&self) -> &'static str;
    fn activity(&self) -> Result<AgentActivity, String>;
    fn latest_completed_message(&self) -> Result<Option<MessageObservation>, String>;
}
