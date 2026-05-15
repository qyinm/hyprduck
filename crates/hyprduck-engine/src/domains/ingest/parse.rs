use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{bail, Context, Result};
use base64::Engine;
use hyprduck_engine_types::{
    DocumentFormat, OutputAsset, ParseEvent, ParseInput, ParseOptions, ParsedPage,
};
use markitdown::{model::ConversionOptions, MarkItDown};
use tempfile::tempdir;

use crate::infra::process::resolve_binary;
use crate::provider::{parse_image_with_provider, parse_text_with_provider, EngineConfig};

#[derive(Debug)]
pub(crate) struct ParsedDocument {
    pub(crate) pages: Vec<ParsedPage>,
    pub(crate) assets: Vec<OutputAsset>,
    pub(crate) page_count: usize,
    pub(crate) success_count: usize,
    pub(crate) failed_count: usize,
}

pub(crate) trait EventSink {
    fn emit(&mut self, event: ParseEvent) -> Result<()>;
}

pub(crate) fn parse_document(
    input: &ParseInput,
    template: &str,
    _options: &ParseOptions,
    config: &EngineConfig,
    event_sink: &mut impl EventSink,
) -> Result<ParsedDocument> {
    match input.format {
        DocumentFormat::Pdf => parse_markitdown_document(input, event_sink)
            .or_else(|_| parse_visual_document(input, template, config, event_sink)),
        DocumentFormat::Image => parse_visual_document(input, template, config, event_sink),
        DocumentFormat::Docx => parse_markitdown_document(input, event_sink)
            .or_else(|_| parse_text_document(input, template, config, event_sink)),
        DocumentFormat::Doc | DocumentFormat::Markdown => {
            parse_text_document(input, template, config, event_sink)
        }
    }
}

fn parse_markitdown_document(
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

fn parse_visual_document(
    input: &ParseInput,
    template: &str,
    config: &EngineConfig,
    event_sink: &mut impl EventSink,
) -> Result<ParsedDocument> {
    let page_images = match input.format {
        DocumentFormat::Image => vec![PathBuf::from(&input.path)],
        DocumentFormat::Pdf => convert_pdf_to_pngs(Path::new(&input.path))?,
        _ => unreachable!(),
    };

    let total = page_images.len() as u32;
    let mut pages = Vec::new();
    let mut assets = Vec::new();
    let mut success_count = 0usize;
    let mut failed_count = 0usize;

    for (idx, image_path) in page_images.iter().enumerate() {
        event_sink.emit(ParseEvent::ConvertingPages {
            current: (idx + 1) as u32,
            total,
        })?;

        let image_bytes = fs::read(image_path)
            .with_context(|| format!("failed to read rendered image {}", image_path.display()))?;
        let relative_path = format!("images/page_{}.png", idx + 1);
        assets.push(OutputAsset {
            relative_path: relative_path.clone(),
            mime_type: "image/png".into(),
            base64: base64::engine::general_purpose::STANDARD.encode(&image_bytes),
        });

        event_sink.emit(ParseEvent::Parsing {
            current: (idx + 1) as u32,
            total,
        })?;

        match parse_image_with_provider(config, &image_bytes, template) {
            Ok(markdown) => {
                success_count += 1;
                pages.push(ParsedPage {
                    index: idx,
                    markdown: Some(markdown.clone()),
                    plain_text: Some(markdown),
                    svg: None,
                    image_asset_path: Some(relative_path),
                    error_message: None,
                });
            }
            Err(error) => {
                failed_count += 1;
                pages.push(ParsedPage {
                    index: idx,
                    markdown: None,
                    plain_text: None,
                    svg: None,
                    image_asset_path: Some(relative_path),
                    error_message: Some(error.to_string()),
                });
            }
        }
    }

    Ok(ParsedDocument {
        page_count: pages.len(),
        pages,
        assets,
        success_count,
        failed_count,
    })
}

fn parse_text_document(
    input: &ParseInput,
    template: &str,
    config: &EngineConfig,
    event_sink: &mut impl EventSink,
) -> Result<ParsedDocument> {
    let text = if input.format == DocumentFormat::Markdown {
        fs::read_to_string(&input.path)
            .with_context(|| format!("failed reading markdown {}", input.path))?
    } else {
        extract_text_via_textutil(Path::new(&input.path))?
    };
    event_sink.emit(ParseEvent::ConvertingPages {
        current: 1,
        total: 1,
    })?;
    event_sink.emit(ParseEvent::Parsing {
        current: 1,
        total: 1,
    })?;

    let page = match parse_text_with_provider(config, &text, template) {
        Ok(markdown) => ParsedPage {
            index: 0,
            markdown: Some(markdown.clone()),
            plain_text: Some(markdown),
            svg: None,
            image_asset_path: None,
            error_message: None,
        },
        Err(error) => ParsedPage {
            index: 0,
            markdown: None,
            plain_text: Some(text.clone()),
            svg: None,
            image_asset_path: None,
            error_message: Some(error.to_string()),
        },
    };

    let failed_count = usize::from(page.markdown.is_none());
    Ok(ParsedDocument {
        pages: vec![page],
        assets: Vec::new(),
        page_count: 1,
        success_count: 1usize.saturating_sub(failed_count),
        failed_count,
    })
}

fn convert_pdf_to_pngs(path: &Path) -> Result<Vec<PathBuf>> {
    let temp = tempdir().context("failed to create temp directory for pdf conversion")?;
    let prefix = temp.path().join("page");
    let status = Command::new(resolve_binary(
        "pdftoppm",
        &["/opt/homebrew/bin/pdftoppm", "/usr/local/bin/pdftoppm"],
    ))
    .arg("-png")
    .arg(path)
    .arg(&prefix)
    .status()
    .context("failed to launch pdftoppm")?;
    if !status.success() {
        bail!("pdftoppm failed for {}", path.display());
    }

    let mut outputs = fs::read_dir(temp.path())
        .with_context(|| {
            format!(
                "failed listing converted PDF pages in {}",
                temp.path().display()
            )
        })?
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.extension().and_then(|ext| ext.to_str()) == Some("png"))
        .collect::<Vec<_>>();
    outputs.sort();

    if outputs.is_empty() {
        bail!("pdf conversion produced no pages for {}", path.display());
    }

    let persisted_root = temp.keep();
    Ok(outputs
        .into_iter()
        .map(|path| persisted_root.join(path.file_name().unwrap()))
        .collect())
}

fn extract_text_via_textutil(path: &Path) -> Result<String> {
    let output = Command::new(resolve_binary("textutil", &["/usr/bin/textutil"]))
        .arg("-convert")
        .arg("txt")
        .arg("-stdout")
        .arg(path)
        .output()
        .context("failed to launch textutil")?;

    if !output.status.success() {
        bail!("textutil failed for {}", path.display());
    }

    let text = String::from_utf8(output.stdout).context("textutil output was not valid UTF-8")?;
    if text.trim().is_empty() {
        bail!(
            "text extraction produced empty output for {}",
            path.display()
        );
    }
    Ok(text)
}
