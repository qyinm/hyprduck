use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DocumentFormat {
    Pdf,
    Docx,
    Doc,
    Image,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ParseInput {
    pub path: String,
    pub format: DocumentFormat,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ParseOptions {
    pub preserve_images: bool,
    pub emit_structured_json: bool,
    pub emit_svg: bool,
    pub language_hints: Vec<String>,
}

impl Default for ParseOptions {
    fn default() -> Self {
        Self {
            preserve_images: true,
            emit_structured_json: false,
            emit_svg: false,
            language_hints: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ParseOutputTarget {
    pub root_dir: Option<String>,
    pub name: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ParseRequest {
    pub version: String,
    pub input: ParseInput,
    pub template: String,
    pub options: ParseOptions,
    pub output: Option<ParseOutputTarget>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ParsedPage {
    pub index: usize,
    pub markdown: Option<String>,
    pub plain_text: Option<String>,
    pub svg: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OutputAsset {
    pub relative_path: String,
    pub mime_type: String,
    pub base64: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ParseMetadata {
    pub engine_id: String,
    pub duration_ms: u64,
    pub page_count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ParseResult {
    pub version: String,
    pub markdown: String,
    pub pages: Vec<ParsedPage>,
    pub assets: Vec<OutputAsset>,
    pub metadata: ParseMetadata,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum ParseProgress {
    Queued,
    ConvertingPages { current: u32, total: u32 },
    Parsing { current: u32, total: u32 },
    Packaging,
    Completed,
    Failed { message: String },
}
