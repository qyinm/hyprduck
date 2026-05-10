use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

pub use duckdocs_knowledge::{
    AnswerResponse, AnswerStatus, CorrectionAction, CorrectionKind, EvidenceRef, GraphNodeDetail,
    GraphNodeKind, GraphNodePosition, GraphNodeSummary, KnowledgeProject, ProjectOverview,
    ProjectStatus, RelationEdgeDetail, RelationEdgeSummary, RelationKind, SourceBacking,
    SuggestedAction, SuggestedActionKind,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EngineCommand {
    Parse,
    CompileProject,
    LoadProject,
    ApplyCorrection,
    AnswerProject,
    LoadConfig,
    SaveConfig,
    ValidateProvider,
    ListProviderModels,
    CheckReadiness,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DocumentFormat {
    Pdf,
    Docx,
    Doc,
    Image,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ParseInput {
    pub path: String,
    pub format: DocumentFormat,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct ParseOptions {
    pub preserve_images: bool,
    pub emit_structured_json: bool,
    pub emit_svg: bool,
    pub language_hints: Vec<String>,
    #[serde(default)]
    pub debug_request_path: Option<String>,
    #[serde(default)]
    pub debug_result_path: Option<String>,
}

impl Default for ParseOptions {
    fn default() -> Self {
        Self {
            preserve_images: true,
            emit_structured_json: false,
            emit_svg: false,
            language_hints: Vec::new(),
            debug_request_path: None,
            debug_result_path: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct ParseOutputTarget {
    pub root_dir: Option<String>,
    pub name: Option<String>,
    #[serde(default)]
    pub workspace_id: Option<WorkspaceId>,
    #[serde(default)]
    pub source_id: Option<SourceId>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ParseRequest {
    pub version: String,
    pub input: ParseInput,
    pub template: String,
    pub options: ParseOptions,
    pub output: Option<ParseOutputTarget>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ParsedPage {
    pub index: usize,
    pub markdown: Option<String>,
    pub plain_text: Option<String>,
    pub svg: Option<String>,
    #[serde(default)]
    pub image_asset_path: Option<String>,
    #[serde(default)]
    pub error_message: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OutputAsset {
    pub relative_path: String,
    pub mime_type: String,
    pub base64: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ParseMetadata {
    pub engine_id: String,
    pub duration_ms: u64,
    pub page_count: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ParseResult {
    pub version: String,
    pub markdown: String,
    pub pages: Vec<ParsedPage>,
    pub assets: Vec<OutputAsset>,
    pub metadata: ParseMetadata,
    #[serde(default)]
    pub success_count: usize,
    #[serde(default)]
    pub failed_count: usize,
}

pub type WorkspaceId = String;
pub type SourceId = String;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IngestStatus {
    Added,
    Rendering,
    Ingesting,
    Ingested,
    NeedsReview,
    Failed,
    Stale,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PageArtifact {
    pub index: usize,
    pub label: String,
    #[serde(default)]
    pub image_path: Option<String>,
    #[serde(default)]
    pub markdown_path: Option<String>,
    #[serde(default)]
    pub plain_text_path: Option<String>,
    #[serde(default)]
    pub error_message: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourceArtifactManifest {
    pub workspace_id: WorkspaceId,
    pub source_id: SourceId,
    pub original_path: String,
    pub source_path: String,
    pub markdown_path: String,
    pub artifact_root: String,
    pub manifest_path: String,
    pub format: DocumentFormat,
    pub output_name: String,
    pub status: IngestStatus,
    pub pages: Vec<PageArtifact>,
    pub created_at: u64,
    pub updated_at: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourceSummary {
    pub workspace_id: WorkspaceId,
    pub source_id: SourceId,
    pub original_path: String,
    pub source_path: String,
    pub markdown_path: String,
    pub format: DocumentFormat,
    pub status: IngestStatus,
    pub page_count: usize,
    pub success_count: usize,
    pub failed_count: usize,
    pub updated_at: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IngestRun {
    pub workspace_id: WorkspaceId,
    pub source_id: SourceId,
    pub status: IngestStatus,
    pub started_at: u64,
    #[serde(default)]
    pub completed_at: Option<u64>,
    pub source_manifest_path: String,
    pub page_count: usize,
    pub success_count: usize,
    pub failed_count: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ParseResponseData {
    pub result: ParseResult,
    #[serde(default)]
    pub saved_output_path: Option<String>,
    #[serde(default)]
    pub source_manifest: Option<SourceArtifactManifest>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CompileProjectRequest {
    pub source_markdown_path: String,
    #[serde(default)]
    pub source_document_path: Option<String>,
    #[serde(default)]
    pub source_manifest_path: Option<String>,
    #[serde(default)]
    pub workspace_id: Option<WorkspaceId>,
    #[serde(default)]
    pub source_id: Option<SourceId>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CompileProjectResponseData {
    pub project_id: String,
    pub workspace_id: WorkspaceId,
    pub source_id: SourceId,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct LoadProjectRequest {
    #[serde(default)]
    pub project_id: Option<String>,
    #[serde(default)]
    pub workspace_id: Option<WorkspaceId>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LoadProjectResponseData {
    #[serde(default)]
    pub project: Option<KnowledgeProject>,
    #[serde(default)]
    pub workspace_id: Option<WorkspaceId>,
    #[serde(default)]
    pub sources: Vec<SourceSummary>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApplyCorrectionRequest {
    pub project_id: String,
    pub node_id: String,
    pub kind: CorrectionKind,
    #[serde(default)]
    pub target_node_id: Option<String>,
    #[serde(default)]
    pub value: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ApplyCorrectionResponseData {
    pub project: KnowledgeProject,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AnswerProjectRequest {
    pub project_id: String,
    #[serde(default)]
    pub node_id: Option<String>,
    pub question: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AnswerProjectResponseData {
    pub answer: AnswerResponse,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProviderOption {
    pub id: String,
    pub label: String,
    pub requires_api_key: bool,
    pub supports_base_url: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EngineConfigPayload {
    pub provider: String,
    pub model_id: String,
    pub api_key: String,
    #[serde(default)]
    pub base_url: Option<String>,
    pub prompt_template: String,
    #[serde(default)]
    pub provider_options: Vec<ProviderOption>,
    #[serde(default)]
    pub model_options: Vec<String>,
    #[serde(default)]
    pub prompt_template_options: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SaveConfigResponseData {
    pub config: EngineConfigPayload,
    pub persisted: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ValidationIssue {
    pub code: String,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ValidateProviderResponseData {
    pub ready: bool,
    #[serde(default)]
    pub issues: Vec<ValidationIssue>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct ListProviderModelsRequest {}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProviderModelCatalogResponseData {
    pub provider_models: BTreeMap<String, Vec<String>>,
    pub ollama_vision_prefixes: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct CheckReadinessRequest {}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReadinessCheck {
    pub id: String,
    pub label: String,
    pub ready: bool,
    #[serde(default = "default_readiness_required")]
    pub required: bool,
    pub message: String,
}

fn default_readiness_required() -> bool {
    true
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RuntimeReadinessResponseData {
    pub ready: bool,
    pub provider: String,
    pub model_id: String,
    #[serde(default)]
    pub checks: Vec<ReadinessCheck>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LoadConfigRequest {}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SaveConfigRequest {
    pub config: EngineConfigPayload,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ValidateProviderRequest {
    #[serde(default)]
    pub config: Option<EngineConfigPayload>,
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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EngineRuntimeEvent {
    pub id: Uuid,
    #[serde(rename = "type")]
    pub message_type: EngineRuntimeMessageType,
    pub event: ParseEvent,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "command", content = "payload", rename_all = "snake_case")]
pub enum EngineRequest {
    Parse(ParseRequest),
    CompileProject(CompileProjectRequest),
    LoadProject(LoadProjectRequest),
    ApplyCorrection(ApplyCorrectionRequest),
    AnswerProject(AnswerProjectRequest),
    LoadConfig(LoadConfigRequest),
    SaveConfig(SaveConfigRequest),
    ValidateProvider(ValidateProviderRequest),
    ListProviderModels(ListProviderModelsRequest),
    CheckReadiness(CheckReadinessRequest),
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
    pub fn new(id: Uuid, event: ParseEvent) -> Self {
        Self {
            id,
            message_type: EngineRuntimeMessageType::Event,
            event,
        }
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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ParseEvent {
    Queued,
    DocumentOpened { format: DocumentFormat },
    ConvertingPages { current: u32, total: u32 },
    Parsing { current: u32, total: u32 },
    Packaging,
    Completed,
    Failed { message: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ParseProgress {
    Queued,
    ConvertingPages { current: u32, total: u32 },
    Parsing { current: u32, total: u32 },
    Packaging,
    Completed,
    Failed { message: String },
}

impl From<ParseEvent> for ParseProgress {
    fn from(value: ParseEvent) -> Self {
        match value {
            ParseEvent::Queued => Self::Queued,
            ParseEvent::DocumentOpened { .. } => Self::Queued,
            ParseEvent::ConvertingPages { current, total } => {
                Self::ConvertingPages { current, total }
            }
            ParseEvent::Parsing { current, total } => Self::Parsing { current, total },
            ParseEvent::Packaging => Self::Packaging,
            ParseEvent::Completed => Self::Completed,
            ParseEvent::Failed { message } => Self::Failed { message },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_request_round_trip() {
        let request = EngineRequest::Parse(ParseRequest {
            version: "1".into(),
            input: ParseInput {
                path: "/tmp/sample.pdf".into(),
                format: DocumentFormat::Pdf,
            },
            template: "General".into(),
            options: ParseOptions::default(),
            output: Some(ParseOutputTarget {
                root_dir: Some("/tmp/out".into()),
                name: Some("sample".into()),
                workspace_id: Some("default".into()),
                source_id: None,
            }),
        });

        let json = serde_json::to_string(&request).unwrap();
        let decoded: EngineRequest = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded, request);
    }

    #[test]
    fn runtime_request_envelope_round_trip() {
        let request = EngineRuntimeRequest {
            id: Uuid::parse_str("019e0b95-7f53-7502-8886-e8c01d3aaad4").unwrap(),
            request: EngineRequest::LoadConfig(LoadConfigRequest {}),
        };

        let json = serde_json::to_string(&request).unwrap();
        assert!(json.contains("\"id\""));
        assert!(json.contains("\"command\":\"load_config\""));
        let decoded: EngineRuntimeRequest = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded, request);
    }

    #[test]
    fn runtime_response_envelope_round_trip() {
        let id = Uuid::parse_str("019e0b95-7f53-7502-8886-e8c01d3aaad4").unwrap();
        let response = EngineRuntimeResponse::new(
            id,
            EngineSuccess::new(
                EngineCommand::LoadConfig,
                serde_json::json!({"ready": true}),
            ),
        );

        let json = serde_json::to_string(&response).unwrap();
        assert!(json.contains("\"id\""));
        assert!(json.contains("\"type\":\"response\""));
        assert!(json.contains("\"command\":\"load_config\""));

        let decoded: EngineRuntimeResponse<serde_json::Value> =
            serde_json::from_str(&json).unwrap();
        assert_eq!(decoded, response);
    }

    #[test]
    fn runtime_event_envelope_round_trip() {
        let id = Uuid::parse_str("019e0b95-7f53-7502-8886-e8c01d3aaad4").unwrap();
        let event = EngineRuntimeEvent::new(id, ParseEvent::Queued);

        let json = serde_json::to_string(&event).unwrap();
        assert!(json.contains("\"type\":\"event\""));
        assert!(json.contains("\"event\":{\"type\":\"queued\"}"));

        let decoded: EngineRuntimeEvent = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded, event);
    }

    #[test]
    fn parse_success_round_trip() {
        let response = EngineSuccess::new(
            EngineCommand::Parse,
            ParseResponseData {
                result: ParseResult {
                    version: "1".into(),
                    markdown: "# sample".into(),
                    pages: vec![ParsedPage {
                        index: 0,
                        markdown: Some("# page".into()),
                        plain_text: Some("page".into()),
                        svg: None,
                        image_asset_path: Some("images/page_1.png".into()),
                        error_message: None,
                    }],
                    assets: vec![OutputAsset {
                        relative_path: "images/page_1.png".into(),
                        mime_type: "image/png".into(),
                        base64: "cG5n".into(),
                    }],
                    metadata: ParseMetadata {
                        engine_id: "stub".into(),
                        duration_ms: 5,
                        page_count: 1,
                    },
                    success_count: 1,
                    failed_count: 0,
                },
                saved_output_path: Some("/tmp/out/sample.md".into()),
                source_manifest: None,
            },
        );

        let json = serde_json::to_string(&response).unwrap();
        let decoded: EngineSuccess<ParseResponseData> = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded, response);
    }

    #[test]
    fn config_success_round_trip() {
        let response = EngineSuccess::new(
            EngineCommand::LoadConfig,
            EngineConfigPayload {
                provider: "open_router".into(),
                model_id: "openai/gpt-4.1-mini".into(),
                api_key: "key".into(),
                base_url: None,
                prompt_template: "General".into(),
                provider_options: vec![ProviderOption {
                    id: "open_router".into(),
                    label: "OpenRouter".into(),
                    requires_api_key: true,
                    supports_base_url: true,
                }],
                model_options: vec!["openai/gpt-4.1-mini".into()],
                prompt_template_options: vec!["General".into()],
            },
        );

        let json = serde_json::to_string(&response).unwrap();
        let decoded: EngineSuccess<EngineConfigPayload> = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded, response);
    }

    #[test]
    fn load_project_round_trip() {
        let response = EngineSuccess::new(
            EngineCommand::LoadProject,
            LoadProjectResponseData {
                project: None,
                workspace_id: Some("default".into()),
                sources: Vec::new(),
            },
        );

        let json = serde_json::to_string(&response).unwrap();
        let decoded: EngineSuccess<LoadProjectResponseData> = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded.command, EngineCommand::LoadProject);
        assert!(decoded.data.project.is_none());
        assert_eq!(decoded.data.workspace_id.as_deref(), Some("default"));
    }

    #[test]
    fn source_artifact_contract_round_trip() {
        let manifest = SourceArtifactManifest {
            workspace_id: "default".into(),
            source_id: "source-123".into(),
            original_path: "/tmp/input.pdf".into(),
            source_path: "/tmp/HyprDuck/default/sources/source-123/input.pdf".into(),
            markdown_path: "/tmp/HyprDuck/default/artifacts/source-123/input.md".into(),
            artifact_root: "/tmp/HyprDuck/default/artifacts/source-123".into(),
            manifest_path: "/tmp/HyprDuck/default/artifacts/source-123/source-manifest.json".into(),
            format: DocumentFormat::Pdf,
            output_name: "input".into(),
            status: IngestStatus::Ingested,
            pages: vec![PageArtifact {
                index: 0,
                label: "Page 1".into(),
                image_path: Some(
                    "/tmp/HyprDuck/default/artifacts/source-123/images/page_1.png".into(),
                ),
                markdown_path: Some(
                    "/tmp/HyprDuck/default/artifacts/source-123/pages/page_1.md".into(),
                ),
                plain_text_path: None,
                error_message: None,
            }],
            created_at: 1,
            updated_at: 2,
        };

        let json = serde_json::to_string(&manifest).unwrap();
        assert!(json.contains("\"status\":\"ingested\""));
        assert!(json.contains("\"format\":\"pdf\""));
        let decoded: SourceArtifactManifest = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded, manifest);
    }

    #[test]
    fn answer_project_round_trip() {
        let request = EngineRequest::AnswerProject(AnswerProjectRequest {
            project_id: "project-123".into(),
            node_id: Some("concept-a".into()),
            question: "What does this concept cover?".into(),
        });
        let json = serde_json::to_string(&request).unwrap();
        let decoded: EngineRequest = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded, request);

        let response = EngineSuccess::new(
            EngineCommand::AnswerProject,
            AnswerProjectResponseData {
                answer: AnswerResponse {
                    status: AnswerStatus::Grounded,
                    text: Some("Grounded answer".into()),
                    explanation: "Based on visible evidence.".into(),
                    citations: vec![],
                    related_node_ids: vec!["concept-b".into()],
                    suggested_actions: vec![],
                },
            },
        );
        let json = serde_json::to_string(&response).unwrap();
        let decoded: EngineSuccess<AnswerProjectResponseData> =
            serde_json::from_str(&json).unwrap();
        assert_eq!(decoded.command, EngineCommand::AnswerProject);
        assert_eq!(decoded.data.answer.status, AnswerStatus::Grounded);
    }

    #[test]
    fn provider_model_catalog_round_trip() {
        let mut provider_models = BTreeMap::new();
        provider_models.insert("open_router".into(), vec!["openai/gpt-4.1-mini".into()]);
        provider_models.insert("ollama".into(), vec!["qwen3-vl:8b".into()]);

        let response = EngineSuccess::new(
            EngineCommand::ListProviderModels,
            ProviderModelCatalogResponseData {
                provider_models,
                ollama_vision_prefixes: vec!["qwen3-vl".into()],
            },
        );

        let json = serde_json::to_string(&response).unwrap();
        let decoded: EngineSuccess<ProviderModelCatalogResponseData> =
            serde_json::from_str(&json).unwrap();
        assert_eq!(decoded.command, EngineCommand::ListProviderModels);
        assert!(decoded.data.provider_models.contains_key("open_router"));
    }

    #[test]
    fn readiness_response_round_trip() {
        let response = EngineSuccess::new(
            EngineCommand::CheckReadiness,
            RuntimeReadinessResponseData {
                ready: true,
                provider: "ollama".into(),
                model_id: "qwen3-vl:8b".into(),
                checks: vec![ReadinessCheck {
                    id: "runtime_process".into(),
                    label: "Runtime process".into(),
                    ready: true,
                    required: true,
                    message: "Runtime process is accepting commands.".into(),
                }],
            },
        );

        let json = serde_json::to_string(&response).unwrap();
        assert!(json.contains("\"command\":\"check_readiness\""));
        let decoded: EngineSuccess<RuntimeReadinessResponseData> =
            serde_json::from_str(&json).unwrap();
        assert_eq!(decoded.command, EngineCommand::CheckReadiness);
        assert!(decoded.data.ready);
    }

    #[test]
    fn failure_round_trip() {
        let failure = EngineFailure::new(
            EngineCommand::ValidateProvider,
            "invalid_api_key",
            "missing key",
        );
        let json = serde_json::to_string(&failure).unwrap();
        let decoded: EngineFailure = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded, failure);
    }

    #[test]
    fn event_round_trip() {
        let event = ParseEvent::Parsing {
            current: 1,
            total: 3,
        };
        let json = serde_json::to_string(&event).unwrap();
        let decoded: ParseEvent = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded, event);
    }

    #[test]
    fn options_decode_with_missing_fields() {
        let decoded: ParseOptions = serde_json::from_str("{}").unwrap();
        assert_eq!(decoded, ParseOptions::default());
    }
}

/// Returns the list of supported model IDs for a given provider slug.
/// Single source of truth — used by both the engine and the desktop UI.
pub fn model_options_for(provider_slug: &str) -> Vec<&'static str> {
    match provider_slug {
        "open_router" => vec![
            "google/gemma-4-31b-it",
            "z-ai/glm-5v-turbo",
            "anthropic/claude-sonnet-4.6",
            "anthropic/claude-opus-4.6",
            "google/gemini-3-flash-preview",
            "qwen/qwen3.6-plus:free",
            "x-ai/grok-4.1-fast",
            "google/gemini-2.5-flash-lite",
            "google/gemini-2.5-flash",
            "moonshotai/kimi-k2.5",
        ],
        "open_ai" => vec![
            "gpt-4.1",
            "gpt-4.1-mini",
            "gpt-4.1-nano",
            "gpt-4o",
            "gpt-4o-mini",
        ],
        "anthropic" => vec![
            "claude-3-7-sonnet-20250219",
            "claude-3-5-sonnet-20241022",
            "claude-3-5-haiku-20241022",
        ],
        "ollama" => vec![
            "gemma4:latest",
            "qwen3.5:latest",
            "qwen3-vl:8b",
            "qwen3-vl:72b",
            "kimi-k2.5:latest",
            "glm-ocr:latest",
            "deepseek-ocr:latest",
        ],
        _ => Vec::new(),
    }
}

/// Prefixes used to identify local Ollama models that can process page images.
pub fn ollama_vision_prefixes() -> Vec<&'static str> {
    vec![
        "gemma4",
        "qwen3.5",
        "qwen3-vl",
        "kimi-k2.5",
        "glm-ocr",
        "deepseek-ocr",
    ]
}
