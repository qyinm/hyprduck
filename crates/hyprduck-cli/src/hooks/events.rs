use serde_json::Value;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum AgentTarget {
    Codex,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum HookEvent {
    SessionStart,
    UserPromptSubmit,
    PreToolUse,
    PermissionRequest,
    PostToolUse,
    Stop,
    Other(String),
}

impl HookEvent {
    pub(crate) fn from_name(name: &str) -> Self {
        match name {
            "SessionStart" => Self::SessionStart,
            "UserPromptSubmit" => Self::UserPromptSubmit,
            "PreToolUse" => Self::PreToolUse,
            "PermissionRequest" => Self::PermissionRequest,
            "PostToolUse" => Self::PostToolUse,
            "Stop" => Self::Stop,
            other => Self::Other(other.to_string()),
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) struct HookInput {
    pub(crate) target: AgentTarget,
    pub(crate) event: HookEvent,
    pub(crate) cwd: Option<String>,
    pub(crate) prompt: Option<String>,
    pub(crate) tool_name: Option<String>,
    pub(crate) tool_input: Option<Value>,
    pub(crate) source: Option<String>,
}
