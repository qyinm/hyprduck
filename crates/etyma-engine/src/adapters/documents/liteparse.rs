use std::collections::HashMap;
use std::future::Future;

use anyhow::{bail, Context, Result};
use base64::Engine;
use etyma_engine_types::{OutputAsset, ParseEvent, ParseInput, ParsedPage};
use liteparse::config::{ImageMode, LiteParseConfig, OutputFormat};
use liteparse::{LiteParse, LiteParseError, ParseResult, ScreenshotResult};

use crate::domains::ingest::pipeline::{EventSink, ParsedDocument};

pub(crate) fn parse_liteparse_document(
    input: &ParseInput,
    event_sink: &mut impl EventSink,
) -> Result<ParsedDocument> {
    event_sink.emit(ParseEvent::ConvertingPages {
        current: 1,
        total: 1,
    })?;

    let parser = LiteParse::new(liteparse_config());
    let result = block_on_liteparse(parser.parse(&input.path))
        .with_context(|| format!("LiteParse failed for {}", input.path))?;
    let total = parsed_page_total(&result);
    event_sink.emit(ParseEvent::Parsing {
        current: total,
        total,
    })?;

    build_liteparse_parsed_document(result)
}

pub(crate) fn parse_liteparse_pdf_document(
    input: &ParseInput,
    event_sink: &mut impl EventSink,
) -> Result<ParsedDocument> {
    event_sink.emit(ParseEvent::ConvertingPages {
        current: 1,
        total: 1,
    })?;

    let parser = LiteParse::new(liteparse_config());
    let result = block_on_liteparse(parser.parse(&input.path))
        .with_context(|| format!("LiteParse failed for {}", input.path))?;
    let total = parsed_page_total(&result);
    event_sink.emit(ParseEvent::Parsing {
        current: total,
        total,
    })?;

    let mut document = build_liteparse_parsed_document(result)?;
    if let Ok(screenshots) = block_on_liteparse(parser.screenshot(&input.path, None)) {
        attach_screenshot_assets(&mut document, screenshots, event_sink)?;
    }

    Ok(document)
}

fn liteparse_config() -> LiteParseConfig {
    LiteParseConfig {
        output_format: OutputFormat::Markdown,
        image_mode: ImageMode::Off,
        ocr_enabled: false,
        quiet: true,
        max_pages: 1000,
        extract_links: true,
        ..LiteParseConfig::default()
    }
}

fn block_on_liteparse<F, T>(future: F) -> Result<T>
where
    F: Future<Output = Result<T, LiteParseError>>,
{
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .context("failed to create LiteParse runtime")?;
    runtime.block_on(future).context("LiteParse runtime failed")
}

fn parsed_page_total(result: &ParseResult) -> u32 {
    result.pages.len().max(1) as u32
}

fn build_liteparse_parsed_document(result: ParseResult) -> Result<ParsedDocument> {
    let page_texts = result
        .pages
        .into_iter()
        .map(|page| LiteParsePageText {
            page_number: page.page_number,
            text: page.text,
        })
        .collect::<Vec<_>>();
    build_parsed_document_from_page_texts(page_texts, result.text)
}

#[derive(Debug)]
struct LiteParsePageText {
    page_number: usize,
    text: String,
}

fn build_parsed_document_from_page_texts(
    page_texts: Vec<LiteParsePageText>,
    full_text: String,
) -> Result<ParsedDocument> {
    let fallback_text = full_text.trim().to_string();
    let pages = if page_texts.is_empty() {
        if fallback_text.is_empty() {
            bail!("LiteParse produced empty markdown");
        }
        vec![ParsedPage {
            index: 0,
            markdown: Some(fallback_text.clone()),
            plain_text: Some(fallback_text.clone()),
            svg: None,
            image_asset_path: None,
            error_message: None,
        }]
    } else {
        page_texts
            .into_iter()
            .enumerate()
            .map(|(fallback_idx, page)| {
                let index = page.page_number.checked_sub(1).unwrap_or(fallback_idx);
                let text = page.text.trim().to_string();
                let has_text = !text.is_empty();
                ParsedPage {
                    index,
                    markdown: has_text.then(|| text.clone()),
                    plain_text: has_text.then_some(text),
                    svg: None,
                    image_asset_path: None,
                    error_message: (!has_text)
                        .then(|| "page text was not available from LiteParse".into()),
                }
            })
            .collect::<Vec<_>>()
    };

    let success_count = pages
        .iter()
        .filter(|page| {
            page.markdown
                .as_deref()
                .is_some_and(|markdown| !markdown.trim().is_empty())
        })
        .count();
    if success_count == 0 {
        if !fallback_text.is_empty() {
            return Ok(ParsedDocument {
                pages: vec![ParsedPage {
                    index: 0,
                    markdown: Some(fallback_text.clone()),
                    plain_text: Some(fallback_text),
                    svg: None,
                    image_asset_path: None,
                    error_message: None,
                }],
                assets: Vec::new(),
                page_count: 1,
                success_count: 1,
                failed_count: 0,
            });
        }
        bail!("LiteParse produced empty markdown");
    }
    let failed_count = pages.len().saturating_sub(success_count);

    Ok(ParsedDocument {
        page_count: pages.len(),
        pages,
        assets: Vec::new(),
        success_count,
        failed_count,
    })
}

fn attach_screenshot_assets(
    document: &mut ParsedDocument,
    screenshots: Vec<ScreenshotResult>,
    event_sink: &mut impl EventSink,
) -> Result<()> {
    if screenshots.is_empty() {
        return Ok(());
    }

    let total = screenshots.len() as u32;
    let mut assets_by_index = HashMap::new();
    for (idx, screenshot) in screenshots.into_iter().enumerate() {
        event_sink.emit(ParseEvent::ConvertingPages {
            current: (idx + 1) as u32,
            total,
        })?;

        let page_index = screenshot.page_num.saturating_sub(1) as usize;
        let relative_path = format!("images/page_{}.png", page_index + 1);
        document.assets.push(OutputAsset {
            relative_path: relative_path.clone(),
            mime_type: "image/png".into(),
            base64: base64::engine::general_purpose::STANDARD.encode(&screenshot.image_bytes),
        });
        assets_by_index.insert(page_index, relative_path);
    }

    for page in &mut document.pages {
        if let Some(relative_path) = assets_by_index.get(&page.index) {
            page.image_asset_path = Some(relative_path.clone());
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use anyhow::Result;
    use etyma_engine_types::{ParseEvent, ParsedPage};
    use liteparse::ScreenshotResult;

    use super::{
        attach_screenshot_assets, build_parsed_document_from_page_texts, LiteParsePageText,
    };
    use crate::domains::ingest::pipeline::{EventSink, ParsedDocument};

    #[test]
    fn liteparse_mapping_preserves_page_indexes_and_counts() {
        let document = build_parsed_document_from_page_texts(
            vec![
                LiteParsePageText {
                    page_number: 1,
                    text: "First page".into(),
                },
                LiteParsePageText {
                    page_number: 2,
                    text: "Second page".into(),
                },
            ],
            "First page\n\nSecond page".into(),
        )
        .expect("document");

        assert_eq!(document.page_count, 2);
        assert_eq!(document.success_count, 2);
        assert_eq!(document.failed_count, 0);
        assert_eq!(document.pages[0].index, 0);
        assert_eq!(document.pages[1].index, 1);
        assert_eq!(document.pages[0].markdown.as_deref(), Some("First page"));
        assert_eq!(document.pages[1].plain_text.as_deref(), Some("Second page"));
    }

    #[test]
    fn liteparse_mapping_tracks_empty_pages_as_failures() {
        let document = build_parsed_document_from_page_texts(
            vec![
                LiteParsePageText {
                    page_number: 1,
                    text: "Visible page".into(),
                },
                LiteParsePageText {
                    page_number: 2,
                    text: "  ".into(),
                },
            ],
            "Visible page".into(),
        )
        .expect("document");

        assert_eq!(document.success_count, 1);
        assert_eq!(document.failed_count, 1);
        assert!(document.pages[1].markdown.is_none());
        assert_eq!(
            document.pages[1].error_message.as_deref(),
            Some("page text was not available from LiteParse")
        );
    }

    #[test]
    fn liteparse_mapping_rejects_all_empty_output() {
        let error = build_parsed_document_from_page_texts(Vec::new(), "  ".into())
            .expect_err("empty output should fail");

        assert!(error
            .to_string()
            .contains("LiteParse produced empty markdown"));
    }

    #[test]
    fn liteparse_mapping_falls_back_to_full_text_when_pages_are_empty() {
        let document = build_parsed_document_from_page_texts(
            vec![LiteParsePageText {
                page_number: 1,
                text: " ".into(),
            }],
            "# Full markdown".into(),
        )
        .expect("document");

        assert_eq!(document.page_count, 1);
        assert_eq!(document.success_count, 1);
        assert_eq!(document.failed_count, 0);
        assert_eq!(
            document.pages[0].markdown.as_deref(),
            Some("# Full markdown")
        );
    }

    #[test]
    fn liteparse_screenshots_attach_page_assets() {
        let mut document = ParsedDocument {
            pages: vec![ParsedPage {
                index: 0,
                markdown: Some("Page text".into()),
                plain_text: Some("Page text".into()),
                svg: None,
                image_asset_path: None,
                error_message: None,
            }],
            assets: Vec::new(),
            page_count: 1,
            success_count: 1,
            failed_count: 0,
        };
        let mut sink = TestSink::default();

        attach_screenshot_assets(
            &mut document,
            vec![ScreenshotResult {
                page_num: 1,
                width: 10,
                height: 10,
                image_bytes: vec![1, 2, 3],
            }],
            &mut sink,
        )
        .expect("attach assets");

        assert_eq!(document.assets.len(), 1);
        assert_eq!(document.assets[0].relative_path, "images/page_1.png");
        assert_eq!(document.assets[0].mime_type, "image/png");
        assert_eq!(
            document.pages[0].image_asset_path.as_deref(),
            Some("images/page_1.png")
        );
        assert!(matches!(
            sink.events.as_slice(),
            [ParseEvent::ConvertingPages {
                current: 1,
                total: 1
            }]
        ));
    }

    #[derive(Default)]
    struct TestSink {
        events: Vec<ParseEvent>,
    }

    impl EventSink for TestSink {
        fn emit(&mut self, event: ParseEvent) -> Result<()> {
            self.events.push(event);
            Ok(())
        }
    }
}
