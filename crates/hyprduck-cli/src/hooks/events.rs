#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum CodexHookEvent {
    SessionStart,
    UserPromptSubmit,
    PreToolUse,
    PermissionRequest,
    PostToolUse,
    Stop,
    Other(String),
}

impl CodexHookEvent {
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
pub(crate) struct CodexHookInput {
    pub(crate) event: CodexHookEvent,
    pub(crate) prompt: Option<String>,
    pub(crate) tool_name: Option<String>,
}
