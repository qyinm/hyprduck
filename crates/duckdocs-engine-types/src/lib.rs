use serde::{Deserialize, Serialize};
use serde_json::Value;

pub use duckdocs_knowledge::{
    AnswerResponse, AnswerStatus, CorrectionAction, CorrectionKind, EvidenceRef, GraphNodeDetail,
    GraphNodeKind, GraphNodePosition, GraphNodeSummary, KnowledgeProject, ProjectOverview,
    ProjectStatus, RelationEdgeDetail, RelationEdgeSummary, RelationKind, SuggestedAction,
    SuggestedActionKind,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EngineCommand {
    Parse,
    CompileProject,
    LoadProject,
    LoadConfig,
    SaveConfig,
    ValidateProvider,
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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ParseResponseData {
    pub result: ParseResult,
    #[serde(default)]
    pub saved_output_path: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CompileProjectRequest {
    pub source_markdown_path: String,
    #[serde(default)]
    pub source_document_path: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CompileProjectResponseData {
    pub project_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct LoadProjectRequest {
    #[serde(default)]
    pub project_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LoadProjectResponseData {
    #[serde(default)]
    pub project: Option<KnowledgeProject>,
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
#[serde(tag = "command", content = "payload", rename_all = "snake_case")]
pub enum EngineRequest {
    Parse(ParseRequest),
    CompileProject(CompileProjectRequest),
    LoadProject(LoadProjectRequest),
    LoadConfig(LoadConfigRequest),
    SaveConfig(SaveConfigRequest),
    ValidateProvider(ValidateProviderRequest),
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
            }),
        });

        let json = serde_json::to_string(&request).unwrap();
        let decoded: EngineRequest = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded, request);
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
            LoadProjectResponseData { project: None },
        );

        let json = serde_json::to_string(&response).unwrap();
        let decoded: EngineSuccess<LoadProjectResponseData> = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded.command, EngineCommand::LoadProject);
        assert!(decoded.data.project.is_none());
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
