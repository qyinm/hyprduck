use std::fs;
use std::path::{Path, PathBuf};
use std::time::Instant;

use anyhow::{Context, Result};
use hyprduck_engine_types::{
    ParseEvent, ParseMetadata, ParseRequest, ParseResponseData, ParseResult,
    RetryFailedPagesRequest, RetryFailedPagesResponseData,
};

use crate::adapters::process::binary_locator::resolve_binary;
use crate::domains::ingest::output_package::{
    build_markdown, export_output_package, retry_failed_page_artifacts,
};
use crate::domains::ingest::pipeline::{parse_document, EventSink, ProcessLocator};
use crate::provider::{EngineConfig, EngineConfigStore};
use crate::runtime::emit_event;

pub(crate) fn maybe_write_debug(path: &Option<String>, contents: &str) -> Result<()> {
    if let Some(path) = path {
        if let Some(parent) = Path::new(path).parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("failed creating debug directory {}", parent.display()))?;
        }
        fs::write(path, contents)
            .with_context(|| format!("failed writing debug artifact {}", path))?;
    }
    Ok(())
}

pub(crate) fn handle_parse(
    request: ParseRequest,
    config_store: &EngineConfigStore,
) -> Result<ParseResponseData> {
    let started = Instant::now();
    let config = config_store.load()?;

    let mut event_sink = RuntimeParseEventSink;

    event_sink.emit(ParseEvent::Queued)?;
    event_sink.emit(ParseEvent::DocumentOpened {
        format: request.input.format.clone(),
    })?;

    let process_locator = RuntimeProcessLocator;
    let parse = parse_document(
        &request.input,
        &request.template,
        &request.options,
        &config,
        &mut event_sink,
        &process_locator,
    )?;
    let markdown = build_markdown(
        request
            .output
            .as_ref()
            .and_then(|target| target.name.clone())
            .unwrap_or_else(|| {
                Path::new(&request.input.path)
                    .file_stem()
                    .map(|value| value.to_string_lossy().into_owned())
                    .unwrap_or_else(|| "document".to_string())
            }),
        &parse.pages,
    );

    event_sink.emit(ParseEvent::Packaging)?;
    let result = ParseResult {
        version: request.version.clone(),
        markdown,
        pages: parse.pages,
        assets: parse.assets,
        metadata: ParseMetadata {
            engine_id: format!("{}/{}", config.provider.id_slug(), config.model_id),
            duration_ms: started.elapsed().as_millis() as u64,
            page_count: parse.page_count,
        },
        success_count: parse.success_count,
        failed_count: parse.failed_count,
    };

    let source_manifest = export_output_package(&request, &result, &config)?;
    let saved_output_path = source_manifest
        .as_ref()
        .map(|manifest| manifest.markdown_path.clone());
    event_sink.emit(ParseEvent::Completed)?;
    Ok(ParseResponseData {
        result,
        saved_output_path,
        source_manifest,
    })
}

pub(crate) fn handle_retry_failed_pages(
    request: RetryFailedPagesRequest,
    config: &EngineConfig,
) -> Result<RetryFailedPagesResponseData> {
    retry_failed_page_artifacts(&request, config)
}

struct RuntimeParseEventSink;

impl EventSink for RuntimeParseEventSink {
    fn emit(&mut self, event: ParseEvent) -> Result<()> {
        emit_event(&event)
    }
}

struct RuntimeProcessLocator;

impl ProcessLocator for RuntimeProcessLocator {
    fn resolve_binary(&self, name: &str, common_paths: &[&str]) -> PathBuf {
        resolve_binary(name, common_paths)
    }
}
