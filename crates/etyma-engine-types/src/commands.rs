use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

use crate::{
    AgentChatAskRequest, AgentChatStreamEvent, AnswerProjectRequest, ApplyCorrectionRequest,
    ApplyGraphPatchRequest, CheckReadinessRequest, CompileProjectRequest, GetBrainHealthRequest,
    GetContextPackRequest, ListProviderModelsRequest, LoadConfigRequest, LoadProjectRequest,
    ParseEvent, ParseRequest, ReadContextPackRequest, ReadGraphHistoryRequest,
    ReadGraphSnapshotRequest, ReadImportJobRequest, ReadNodeRequest, ReadPageEvidenceRequest,
    ReadRecentEventsRequest, ReadSourceRequest, ReadWikiPageRequest, ReconstructBrainRequest,
    RetryFailedPagesRequest, SaveConfigRequest, SearchBrainRequest, UpdateImportJobGraphStatusRequest,
    ValidateProviderRequest, WriteCommitAllRequest, WriteCommitRequest, WriteListRequest,
    WriteProposeRequest, WriteRejectRequest,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EngineCommand {
    Parse,
    RetryFailedPages,
    CompileProject,
    ReadImportJob,
    UpdateImportJobGraphStatus,
    LoadProject,
    ApplyCorrection,
    AnswerProject,
    AgentChatAsk,
    SearchBrain,
    ReadSource,
    ReadPageEvidence,
    ReadContextPack,
    ReadWikiPage,
    ReadNode,
    ReadRecentEvents,
    ReadGraphHistory,
    ReadGraphSnapshot,
    ReconstructBrain,
    GetContextPack,
    GetBrainHealth,
    LoadConfig,
    SaveConfig,
    ValidateProvider,
    ListProviderModels,
    CheckReadiness,
    ApplyGraphPatch,
    WritePropose,
    WriteCommit,
    WriteCommitAll,
    WriteList,
    WriteReject,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EngineRuntimeRequest {
    pub id: Uuid,
    #[serde(flatten)]
    pub request: EngineRequest,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EngineRuntimeMessageType {
    Response,
    Event,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EngineRuntimeResponse<T> {
    pub id: Uuid,
    #[serde(rename = "type")]
    pub message_type: EngineRuntimeMessageType,
    #[serde(flatten)]
    pub response: EngineSuccess<T>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EngineRuntimeFailure {
    pub id: Uuid,
    #[serde(rename = "type")]
    pub message_type: EngineRuntimeMessageType,
    #[serde(flatten)]
    pub failure: EngineFailure,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EngineRuntimeEvent {
    pub id: Uuid,
    #[serde(rename = "type")]
    pub message_type: EngineRuntimeMessageType,
    pub event: EngineEvent,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum EngineEvent {
    Parse(ParseEvent),
    AgentChat(AgentChatStreamEvent),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "command", content = "payload", rename_all = "snake_case")]
pub enum EngineRequest {
    Parse(ParseRequest),
    RetryFailedPages(RetryFailedPagesRequest),
    CompileProject(CompileProjectRequest),
    ReadImportJob(ReadImportJobRequest),
    UpdateImportJobGraphStatus(UpdateImportJobGraphStatusRequest),
    LoadProject(LoadProjectRequest),
    ApplyCorrection(ApplyCorrectionRequest),
    AnswerProject(AnswerProjectRequest),
    AgentChatAsk(AgentChatAskRequest),
    SearchBrain(SearchBrainRequest),
    ReadSource(ReadSourceRequest),
    ReadPageEvidence(ReadPageEvidenceRequest),
    ReadContextPack(ReadContextPackRequest),
    ReadWikiPage(ReadWikiPageRequest),
    ReadNode(ReadNodeRequest),
    ReadRecentEvents(ReadRecentEventsRequest),
    ReadGraphHistory(ReadGraphHistoryRequest),
    ReadGraphSnapshot(ReadGraphSnapshotRequest),
    ReconstructBrain(ReconstructBrainRequest),
    GetContextPack(GetContextPackRequest),
    GetBrainHealth(GetBrainHealthRequest),
    LoadConfig(LoadConfigRequest),
    SaveConfig(SaveConfigRequest),
    ValidateProvider(ValidateProviderRequest),
    ListProviderModels(ListProviderModelsRequest),
    CheckReadiness(CheckReadinessRequest),
    ApplyGraphPatch(ApplyGraphPatchRequest),
    WritePropose(WriteProposeRequest),
    WriteCommit(WriteCommitRequest),
    WriteCommitAll(WriteCommitAllRequest),
    WriteList(WriteListRequest),
    WriteReject(WriteRejectRequest),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EngineError {
    pub code: String,
    pub message: String,
    #[serde(default)]
    pub details: Option<Value>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EngineSuccess<T> {
    pub ok: bool,
    pub command: EngineCommand,
    pub data: T,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EngineFailure {
    pub ok: bool,
    pub command: EngineCommand,
    pub error: EngineError,
}

impl<T> EngineSuccess<T> {
    pub fn new(command: EngineCommand, data: T) -> Self {
        Self {
            ok: true,
            command,
            data,
        }
    }
}

impl<T> EngineRuntimeResponse<T> {
    pub fn new(id: Uuid, response: EngineSuccess<T>) -> Self {
        Self {
            id,
            message_type: EngineRuntimeMessageType::Response,
            response,
        }
    }
}

impl EngineRuntimeFailure {
    pub fn new(id: Uuid, failure: EngineFailure) -> Self {
        Self {
            id,
            message_type: EngineRuntimeMessageType::Response,
            failure,
        }
    }
}

impl EngineRuntimeEvent {
    pub fn new(id: Uuid, event: EngineEvent) -> Self {
        Self {
            id,
            message_type: EngineRuntimeMessageType::Event,
            event,
        }
    }

    pub fn parse(id: Uuid, event: ParseEvent) -> Self {
        Self::new(id, EngineEvent::Parse(event))
    }

    pub fn agent_chat(id: Uuid, event: AgentChatStreamEvent) -> Self {
        Self::new(id, EngineEvent::AgentChat(event))
    }
}

impl EngineFailure {
    pub fn new(
        command: EngineCommand,
        code: impl Into<String>,
        message: impl Into<String>,
    ) -> Self {
        Self {
            ok: false,
            command,
            error: EngineError {
                code: code.into(),
                message: message.into(),
                details: None,
            },
        }
    }
}
