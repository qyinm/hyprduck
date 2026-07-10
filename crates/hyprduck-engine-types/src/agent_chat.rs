use serde::{Deserialize, Serialize};

use hyprduck_knowledge::AnswerResponse;

use crate::{
    BrainReadScope, ContextPackEvidenceV1, ContextPackRetrievalTraceV1, SourceId,
};

pub const AGENT_CHAT_SCHEMA_VERSION: &str = "hyprduck.agent_chat.v1";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentChatScopeMode {
    Auto,
    AllDocs,
    SelectedSource,
    GraphContext,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentChatAnswerMode {
    General,
    Evidence,
    Blocked,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentChatStreamStatus {
    ResolvingScope,
    RetrievingContext,
    ClassifyingQuestion,
    ConnectingProvider,
    Generating,
    ValidatingCitations,
    Complete,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentChatMessageRole {
    User,
    Assistant,
    System,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentChatMessage {
    pub id: String,
    pub role: AgentChatMessageRole,
    pub text: String,
    pub created_at: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentChatAskRequest {
    pub schema_version: String,
    pub conversation_id: String,
    #[serde(default)]
    pub assistant_message_id: Option<String>,
    pub scope: BrainReadScope,
    pub mode: AgentChatScopeMode,
    #[serde(default)]
    pub selected_node_id: Option<String>,
    #[serde(default)]
    pub source_ids: Vec<SourceId>,
    pub question: String,
    #[serde(default)]
    pub history: Vec<AgentChatMessage>,
    #[serde(default)]
    pub budget: Option<usize>,
    #[serde(default)]
    pub persist_context_pack: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentChatProviderSummary {
    pub id: String,
    pub label: String,
    pub model_id: String,
    pub hosted: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentChatAskResponseData {
    pub schema_version: String,
    pub conversation_id: String,
    pub answer_mode: AgentChatAnswerMode,
    pub assistant_message: AgentChatMessage,
    pub answer: AnswerResponse,
    pub context_pack_id: String,
    #[serde(default)]
    pub persisted_context_pack_path: Option<String>,
    #[serde(default)]
    pub citations: Vec<ContextPackEvidenceV1>,
    pub retrieval_trace: ContextPackRetrievalTraceV1,
    pub provider: AgentChatProviderSummary,
    #[serde(default)]
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AgentChatStreamEvent {
    Started {
        conversation_id: String,
        assistant_message_id: String,
        provider: AgentChatProviderSummary,
        #[serde(default)]
        answer_mode: Option<AgentChatAnswerMode>,
    },
    Status {
        status: AgentChatStreamStatus,
        message: String,
    },
    Delta {
        text: String,
    },
    CitationUpdate {
        citations: Vec<ContextPackEvidenceV1>,
    },
    Final {
        result: AgentChatAskResponseData,
    },
    Error {
        code: String,
        message: String,
    },
    Stopped {
        partial_text: String,
    },
}
