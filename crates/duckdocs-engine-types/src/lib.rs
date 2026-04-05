use serde::{Deserialize, Serialize};

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
            ParseEvent::ConvertingPages { current, total } => Self::ConvertingPages { current, total },
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
    fn request_round_trip() {
        let request = ParseRequest {
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
        };

        let json = serde_json::to_string(&request).unwrap();
        let decoded: ParseRequest = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded, request);
    }

    #[test]
    fn result_round_trip() {
        let result = ParseResult {
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
        };

        let json = serde_json::to_string(&result).unwrap();
        let decoded: ParseResult = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded, result);
    }

    #[test]
    fn event_round_trip() {
        let event = ParseEvent::Parsing { current: 1, total: 3 };
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
