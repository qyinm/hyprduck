use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DocumentFormat {
    Pdf,
    Docx,
    Doc,
    Image,
    Markdown,
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
    Partial,
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
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub user_context: String,
    #[serde(default)]
    pub ingest_instruction: String,
    pub pages: Vec<PageArtifact>,
    pub created_at: u64,
    pub updated_at: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RetryPageArtifactUpdate {
    pub page_index: usize,
    #[serde(default)]
    pub markdown: Option<String>,
    #[serde(default)]
    pub plain_text: Option<String>,
    #[serde(default)]
    pub image_asset_path: Option<String>,
    #[serde(default)]
    pub error_message: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RetryFailedPagesRequest {
    pub source_manifest_path: String,
    pub pages: Vec<RetryPageArtifactUpdate>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RetryFailedPagesResponseData {
    pub source_manifest: SourceArtifactManifest,
    pub retried_page_count: usize,
    pub remaining_failed_count: usize,
    pub warnings_before: usize,
    pub warnings_after: usize,
    pub source_pack_path: String,
    pub evidence_index_path: String,
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
    #[serde(default)]
    pub citation_ready: bool,
    #[serde(default)]
    pub graph_ready: bool,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub user_context: String,
    #[serde(default)]
    pub ingest_instruction: String,
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
    #[serde(default)]
    pub skip_graph_generation: Option<bool>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CompileProjectResponseData {
    pub project_id: String,
    pub workspace_id: WorkspaceId,
    pub source_id: SourceId,
    #[serde(default)]
    pub graph_generation_status: Option<String>,
    #[serde(default)]
    pub graph_generation_skipped_reason: Option<String>,
    #[serde(default)]
    pub graph_generation_error_message: Option<String>,
    #[serde(default)]
    pub graph_generation_retryable: Option<bool>,
    #[serde(default)]
    pub graph_generation_failed_reason: Option<String>,
    #[serde(default)]
    pub graph_generation_stage: Option<String>,
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
