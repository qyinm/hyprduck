use anyhow::{bail, Context, Result};
use hyprduck_engine_types::{DocumentFormat, ParseEvent, ParseInput, ParsedPage};
use markitdown::{model::ConversionOptions, MarkItDown};

use crate::domains::ingest::pipeline::{EventSink, ParsedDocument};

pub(crate) fn parse_markitdown_document(
    input: &ParseInput,
    event_sink: &mut impl EventSink,
) -> Result<ParsedDocument> {
    event_sink.emit(ParseEvent::ConvertingPages {
        current: 1,
        total: 1,
    })?;
    event_sink.emit(ParseEvent::Parsing {
        current: 1,
        total: 1,
    })?;

    let converter = MarkItDown::new();
    let options = ConversionOptions {
        file_extension: Some(markitdown_extension_for_format(&input.format).into()),
        url: None,
        llm_client: None,
        llm_model: None,
    };
    let result = converter
        .convert(&input.path, Some(options))
        .with_context(|| format!("markitdown-rs failed for {}", input.path))?
        .with_context(|| format!("markitdown-rs did not support {}", input.path))?;

    build_markitdown_parsed_document(result.text_content)
}

fn markitdown_extension_for_format(format: &DocumentFormat) -> &'static str {
    match format {
        DocumentFormat::Pdf => ".pdf",
        DocumentFormat::Docx => ".docx",
        DocumentFormat::Doc => ".doc",
        DocumentFormat::Markdown => ".md",
        DocumentFormat::Image => ".png",
    }
}

fn build_single_page_parsed_document(
    markdown: String,
    parser_name: &str,
) -> Result<ParsedDocument> {
    if markdown.trim().is_empty() {
        bail!("{parser_name} produced empty markdown");
    }
    Ok(ParsedDocument {
        pages: vec![ParsedPage {
            index: 0,
            markdown: Some(markdown.clone()),
            plain_text: Some(markdown),
            svg: None,
            image_asset_path: None,
            error_message: None,
        }],
        assets: Vec::new(),
        page_count: 1,
        success_count: 1,
        failed_count: 0,
    })
}

fn build_markitdown_parsed_document(markdown: String) -> Result<ParsedDocument> {
    build_single_page_parsed_document(markdown, "markitdown-rs")
}
