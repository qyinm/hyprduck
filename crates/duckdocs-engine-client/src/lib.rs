use anyhow::Result;
use duckdocs_engine_types::{ParseProgress, ParseRequest, ParseResult};

pub trait EngineClient {
    fn parse(
        &self,
        request: ParseRequest,
        on_progress: &mut dyn FnMut(ParseProgress),
    ) -> Result<ParseResult>;
}

#[derive(Debug, Default)]
pub struct StubEngineClient;

impl EngineClient for StubEngineClient {
    fn parse(
        &self,
        request: ParseRequest,
        on_progress: &mut dyn FnMut(ParseProgress),
    ) -> Result<ParseResult> {
        on_progress(ParseProgress::Queued);
        on_progress(ParseProgress::ConvertingPages { current: 1, total: 1 });
        on_progress(ParseProgress::Parsing { current: 1, total: 1 });
        on_progress(ParseProgress::Packaging);
        on_progress(ParseProgress::Completed);

        Ok(ParseResult {
            version: "1".to_string(),
            markdown: format!(
                "# {}\n\nStub parse result for `{}`.\n",
                request
                    .output
                    .as_ref()
                    .and_then(|output| output.name.clone())
                    .unwrap_or_else(|| "document".to_string()),
                request.input.path
            ),
            pages: vec![],
            assets: vec![],
            metadata: duckdocs_engine_types::ParseMetadata {
                engine_id: "stub".to_string(),
                duration_ms: 10,
                page_count: 1,
            },
        })
    }
}
