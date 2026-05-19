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

pub(crate) trait ProcessLocator {
    fn resolve_binary(&self, name: &str, common_paths: &[&str]) -> PathBuf;
}

pub(crate) fn parse_document(
    input: &ParseInput,
    template: &str,
    _options: &ParseOptions,
    config: &EngineConfig,
    event_sink: &mut impl EventSink,
    process_locator: &impl ProcessLocator,
) -> Result<ParsedDocument> {
    match input.format {
        DocumentFormat::Pdf => {
            parse_pdf_document(input, event_sink, process_locator).or_else(|_| {
                parse_visual_document(input, template, config, event_sink, process_locator)
            })
        }
        DocumentFormat::Image => {
            parse_visual_document(input, template, config, event_sink, process_locator)
        }
        DocumentFormat::Docx => parse_markitdown_document(input, event_sink)
            .map(attach_text_preview_assets)
            .or_else(|_| parse_text_document(input, template, config, event_sink, process_locator)),
        DocumentFormat::Doc | DocumentFormat::Markdown => {
            parse_text_document(input, template, config, event_sink, process_locator)
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

fn parse_pdf_document(
    input: &ParseInput,
    event_sink: &mut impl EventSink,
    process_locator: &impl ProcessLocator,
) -> Result<ParsedDocument> {
    event_sink.emit(ParseEvent::ConvertingPages {
        current: 1,
        total: 1,
    })?;
    let page_images = convert_pdf_to_pngs(Path::new(&input.path), process_locator)?;
    let total = page_images.len() as u32;
    let mut assets = Vec::new();
    for (idx, image_path) in page_images.iter().enumerate() {
        event_sink.emit(ParseEvent::ConvertingPages {
            current: (idx + 1) as u32,
            total,
        })?;
        let image_bytes = fs::read(image_path)
            .with_context(|| format!("failed to read rendered image {}", image_path.display()))?;
        assets.push(OutputAsset {
            relative_path: format!("images/page_{}.png", idx + 1),
            mime_type: "image/png".into(),
            base64: base64::engine::general_purpose::STANDARD.encode(&image_bytes),
        });
    }

    event_sink.emit(ParseEvent::Parsing { current: 1, total })?;
    let text_document = parse_markitdown_document(input, event_sink)?;
    let text_pages =
        split_pdf_markdown_pages(&text_document.pages[0].markdown.clone().unwrap_or_default());
    let mut success_count = 0usize;
    let pages = assets
        .iter()
        .enumerate()
        .map(|(idx, asset)| {
            let markdown = markdown_for_rendered_page(&text_pages, idx);
            let plain_text = markdown.clone();
            success_count += usize::from(
                markdown
                    .as_ref()
                    .is_some_and(|value| !value.trim().is_empty()),
            );
            let error_message = markdown
                .as_ref()
                .is_none_or(|value| value.trim().is_empty())
                .then(|| "page text was not available from PDF text extraction".into());
            ParsedPage {
                index: idx,
                markdown,
                plain_text,
                svg: None,
                image_asset_path: Some(asset.relative_path.clone()),
                error_message,
            }
        })
        .collect::<Vec<_>>();
    let failed_count = pages.len().saturating_sub(success_count);

    Ok(ParsedDocument {
        page_count: pages.len(),
        pages,
        assets,
        success_count,
        failed_count,
    })
}

fn split_pdf_markdown_pages(markdown: &str) -> Vec<String> {
    let pages = markdown
        .split('\u{c}')
        .map(str::trim)
        .filter(|page| !page.is_empty())
        .map(str::to_string)
        .collect::<Vec<_>>();
    if pages.is_empty() {
        vec![markdown.to_string()]
    } else {
        pages
    }
}

fn markdown_for_rendered_page(text_pages: &[String], page_index: usize) -> Option<String> {
    if text_pages.is_empty() {
        return None;
    }
    if text_pages.len() == 1 {
        return text_pages.first().cloned();
    }
    text_pages
        .get(page_index)
        .cloned()
        .or_else(|| text_pages.last().cloned())
}

fn attach_text_preview_assets(mut document: ParsedDocument) -> ParsedDocument {
    for page in &mut document.pages {
        if page.image_asset_path.is_some() {
            continue;
        }
        let page_number = page.index + 1;
        let preview_text = page
            .markdown
            .as_deref()
            .or(page.plain_text.as_deref())
            .unwrap_or("Text page");
        let relative_path = format!("images/page_{page_number}.svg");
        document.assets.push(OutputAsset {
            relative_path: relative_path.clone(),
            mime_type: "image/svg+xml".into(),
            base64: base64::engine::general_purpose::STANDARD
                .encode(text_preview_svg(page_number, preview_text)),
        });
        page.image_asset_path = Some(relative_path);
    }
    document
}

fn text_preview_svg(page_number: usize, text: &str) -> String {
    let excerpt = text
        .split_whitespace()
        .take(36)
        .collect::<Vec<_>>()
        .join(" ");
    format!(
        r##"<svg xmlns="http://www.w3.org/2000/svg" width="960" height="1240" viewBox="0 0 960 1240">
<rect width="960" height="1240" fill="#ffffff"/>
<rect x="48" y="48" width="864" height="1144" fill="none" stroke="#d4d4d8" stroke-width="4"/>
<text x="96" y="132" font-family="Arial, sans-serif" font-size="42" fill="#18181b">Page {page_number}</text>
<text x="96" y="210" font-family="Arial, sans-serif" font-size="28" fill="#3f3f46">{}</text>
</svg>"##,
        escape_xml(&excerpt)
    )
}

fn escape_xml(value: &str) -> String {
    value
        .chars()
        .flat_map(|character| match character {
            '&' => "&amp;".chars().collect::<Vec<_>>(),
            '<' => "&lt;".chars().collect::<Vec<_>>(),
            '>' => "&gt;".chars().collect::<Vec<_>>(),
            '"' => "&quot;".chars().collect::<Vec<_>>(),
            '\'' => "&apos;".chars().collect::<Vec<_>>(),
            _ => vec![character],
        })
        .collect()
}

fn parse_visual_document(
    input: &ParseInput,
    template: &str,
    config: &EngineConfig,
    event_sink: &mut impl EventSink,
    process_locator: &impl ProcessLocator,
) -> Result<ParsedDocument> {
    let page_images = match input.format {
        DocumentFormat::Image => vec![PathBuf::from(&input.path)],
        DocumentFormat::Pdf => convert_pdf_to_pngs(Path::new(&input.path), process_locator)?,
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
    process_locator: &impl ProcessLocator,
) -> Result<ParsedDocument> {
    let text = if input.format == DocumentFormat::Markdown {
        fs::read_to_string(&input.path)
            .with_context(|| format!("failed reading markdown {}", input.path))?
    } else {
        extract_text_via_textutil(Path::new(&input.path), process_locator)?
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
        Err(error) => {
            let provider_error = format!("{error:#}");
            let fallback_markdown = provider_fallback_markdown(&text, &provider_error);
            ParsedPage {
                index: 0,
                markdown: Some(fallback_markdown.clone()),
                plain_text: Some(fallback_markdown),
                svg: None,
                image_asset_path: None,
                error_message: Some(provider_error),
            }
        }
    };

    Ok(attach_text_preview_assets(ParsedDocument {
        pages: vec![page],
        assets: Vec::new(),
        page_count: 1,
        success_count: 1,
        failed_count: 0,
    }))
}

fn provider_fallback_markdown(text: &str, provider_error: &str) -> String {
    format!("_HyprDuck provider fallback: {provider_error}_\n\n{text}")
}

fn convert_pdf_to_pngs(path: &Path, process_locator: &impl ProcessLocator) -> Result<Vec<PathBuf>> {
    let temp = tempdir().context("failed to create temp directory for pdf conversion")?;
    let prefix = temp.path().join("page");
    let status = Command::new(process_locator.resolve_binary(
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
    outputs.sort_by_key(|path| rendered_page_sort_key(path));

    if outputs.is_empty() {
        bail!("pdf conversion produced no pages for {}", path.display());
    }

    let persisted_root = temp.keep();
    Ok(outputs
        .into_iter()
        .map(|path| persisted_root.join(path.file_name().unwrap()))
        .collect())
}

fn rendered_page_sort_key(path: &Path) -> usize {
    path.file_stem()
        .and_then(|stem| stem.to_str())
        .and_then(|stem| {
            stem.rsplit_once('-')
                .map(|(_, suffix)| suffix)
                .or(Some(stem))
        })
        .and_then(|suffix| suffix.parse::<usize>().ok())
        .unwrap_or(usize::MAX)
}

#[cfg(test)]
mod tests {
    use super::{provider_fallback_markdown, rendered_page_sort_key};
    use std::path::Path;

    #[test]
    fn rendered_pdf_pages_sort_by_numeric_suffix() {
        let mut pages = [
            "page-10.png",
            "page-2.png",
            "page-1.png",
            "page-11.png",
            "page-3.png",
        ];

        pages.sort_by_key(|page| rendered_page_sort_key(Path::new(page)));

        assert_eq!(
            pages,
            [
                "page-1.png",
                "page-2.png",
                "page-3.png",
                "page-10.png",
                "page-11.png"
            ]
        );
    }

    #[test]
    fn provider_fallback_markdown_preserves_failure_taxonomy() {
        let markdown = provider_fallback_markdown(
            "Extracted text",
            "provider_timeout: provider request timed out after 1s",
        );

        assert!(markdown.contains("provider_timeout: provider request timed out"));
        assert!(markdown.contains("Extracted text"));
    }
}

fn extract_text_via_textutil(path: &Path, process_locator: &impl ProcessLocator) -> Result<String> {
    let output = Command::new(process_locator.resolve_binary("textutil", &["/usr/bin/textutil"]))
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
