use std::fs;
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Instant;

use anyhow::{anyhow, bail, Context, Result};
use base64::Engine;
use duckdocs_engine_types::{
    DocumentFormat, EngineCommand, EngineConfigPayload, EngineFailure, EngineRequest,
    EngineSuccess, LoadConfigRequest, OutputAsset, ParseEvent, ParseInput, ParseMetadata,
    ParseOptions, ParseRequest, ParseResponseData, ParseResult, ParsedPage, ProviderOption,
    SaveConfigRequest, SaveConfigResponseData, ValidateProviderRequest,
    ValidateProviderResponseData, ValidationIssue,
};
use reqwest::blocking::Client;
use serde::{Deserialize, Serialize};
use tempfile::tempdir;

fn main() {
    if let Err(error) = run() {
        eprintln!("{error:?}");
        std::process::exit(1);
    }
}

fn run() -> Result<()> {
    let mut payload = String::new();
    io::stdin()
        .read_to_string(&mut payload)
        .context("failed to read engine request")?;
    let request = decode_request(&payload)?;
    let config_store = EngineConfigStore::default()?;

    match request {
        EngineRequest::Parse(request) => {
            maybe_write_debug(&request.options.debug_request_path, &payload)?;
            let debug_result_path = request.options.debug_result_path.clone();
            let response = handle_parse(request, &config_store)
                .map(|data| {
                    serde_json::to_string_pretty(&EngineSuccess::new(EngineCommand::Parse, data))
                })
                .unwrap_or_else(|error| {
                    let _ = emit_event(&ParseEvent::Failed {
                        message: error.to_string(),
                    });
                    serde_json::to_string_pretty(&engine_failure(EngineCommand::Parse, &error))
                })
                .context("failed to encode parse response")?;
            maybe_write_debug(&debug_result_path, &response)?;
            io::stdout()
                .write_all(response.as_bytes())
                .context("failed to write parse response")?;
        }
        EngineRequest::LoadConfig(LoadConfigRequest {}) => {
            let config = config_store.load()?;
            let payload = EngineSuccess::new(EngineCommand::LoadConfig, config.to_payload());
            write_response(&payload)?;
        }
        EngineRequest::SaveConfig(SaveConfigRequest { config }) => {
            let config = EngineConfig::from_payload(config);
            config_store.save(&config)?;
            let payload = EngineSuccess::new(
                EngineCommand::SaveConfig,
                SaveConfigResponseData {
                    config: config.to_payload(),
                    persisted: true,
                },
            );
            write_response(&payload)?;
        }
        EngineRequest::ValidateProvider(ValidateProviderRequest { config }) => {
            let config = config
                .map(EngineConfig::from_payload)
                .unwrap_or(config_store.load()?);
            let payload =
                EngineSuccess::new(EngineCommand::ValidateProvider, validate_provider(&config));
            write_response(&payload)?;
        }
    }
    Ok(())
}

fn decode_request(payload: &str) -> Result<EngineRequest> {
    serde_json::from_str(payload)
        .or_else(|_| serde_json::from_str::<ParseRequest>(payload).map(EngineRequest::Parse))
        .context("failed to decode engine request JSON")
}

fn write_response<T: Serialize>(payload: &T) -> Result<()> {
    let output =
        serde_json::to_string_pretty(payload).context("failed to encode engine response")?;
    io::stdout()
        .write_all(output.as_bytes())
        .context("failed to write engine response")
}

fn maybe_write_debug(path: &Option<String>, contents: &str) -> Result<()> {
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

fn emit_event(event: &ParseEvent) -> Result<()> {
    let line = serde_json::to_string(event).context("failed to encode parse event")?;
    eprintln!("{line}");
    Ok(())
}

#[derive(Debug)]
struct ParsedDocument {
    pages: Vec<ParsedPage>,
    assets: Vec<OutputAsset>,
    page_count: usize,
    success_count: usize,
    failed_count: usize,
}

fn handle_parse(
    request: ParseRequest,
    config_store: &EngineConfigStore,
) -> Result<ParseResponseData> {
    let started = Instant::now();
    let config = config_store.load()?;

    emit_event(&ParseEvent::Queued)?;
    emit_event(&ParseEvent::DocumentOpened {
        format: request.input.format.clone(),
    })?;

    let parse = parse_document(&request.input, &request.template, &request.options, &config)?;
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

    emit_event(&ParseEvent::Packaging)?;
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

    let saved_output_path = export_output_package(&request, &result)?;
    emit_event(&ParseEvent::Completed)?;
    Ok(ParseResponseData {
        result,
        saved_output_path,
    })
}

fn engine_failure(command: EngineCommand, error: &anyhow::Error) -> EngineFailure {
    let code = if format!("{error:?}").contains("decode") {
        "invalid_request"
    } else if format!("{error:?}").contains("config") {
        "config_error"
    } else {
        "runtime_error"
    };
    EngineFailure::new(command, code, error.to_string())
}

fn parse_document(
    input: &ParseInput,
    template: &str,
    _options: &ParseOptions,
    config: &EngineConfig,
) -> Result<ParsedDocument> {
    match input.format {
        DocumentFormat::Pdf | DocumentFormat::Image => {
            parse_visual_document(input, template, config)
        }
        DocumentFormat::Docx | DocumentFormat::Doc => parse_text_document(input, template, config),
    }
}

fn parse_visual_document(
    input: &ParseInput,
    template: &str,
    config: &EngineConfig,
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
        emit_event(&ParseEvent::ConvertingPages {
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

        emit_event(&ParseEvent::Parsing {
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
) -> Result<ParsedDocument> {
    let text = extract_text_via_textutil(Path::new(&input.path))?;
    emit_event(&ParseEvent::ConvertingPages {
        current: 1,
        total: 1,
    })?;
    emit_event(&ParseEvent::Parsing {
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
    let status = Command::new("/opt/homebrew/bin/pdftoppm")
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
    let output = Command::new("/usr/bin/textutil")
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

fn build_markdown(title: String, pages: &[ParsedPage]) -> String {
    let mut markdown = format!("# {title}\n\n");
    for (idx, page) in pages.iter().enumerate() {
        markdown.push_str(&format!("## Page {}\n\n", idx + 1));
        if let Some(image_path) = &page.image_asset_path {
            markdown.push_str(&format!("![Page {}]({image_path})\n\n", idx + 1));
        }
        if let Some(body) = page
            .markdown
            .as_ref()
            .or(page.plain_text.as_ref())
            .filter(|value| !value.trim().is_empty())
        {
            markdown.push_str(body);
            markdown.push_str("\n\n");
        } else if let Some(error_message) = &page.error_message {
            markdown.push_str(&format!("_AI analysis unavailable: {error_message}_\n\n"));
        } else {
            markdown.push_str("_AI analysis unavailable._\n\n");
        }
    }
    markdown
}

fn export_output_package(request: &ParseRequest, result: &ParseResult) -> Result<Option<String>> {
    let Some(output) = &request.output else {
        return Ok(None);
    };

    let output_root = match &output.root_dir {
        Some(root) => PathBuf::from(root),
        None => dirs::document_dir()
            .ok_or_else(|| anyhow!("failed to resolve documents directory"))?
            .join("DuckDocs"),
    };
    fs::create_dir_all(&output_root)
        .with_context(|| format!("failed creating output root {}", output_root.display()))?;

    let base_name = output
        .name
        .clone()
        .or_else(|| {
            Path::new(&request.input.path)
                .file_stem()
                .map(|value| value.to_string_lossy().into_owned())
        })
        .unwrap_or_else(|| "document".to_string());
    let safe_name = sanitize_name(&base_name);
    let timestamp = chrono_like_timestamp();
    let output_dir = output_root.join(format!("{safe_name}_{timestamp}"));
    let images_dir = output_dir.join("images");
    fs::create_dir_all(&images_dir).with_context(|| {
        format!(
            "failed creating image output directory {}",
            images_dir.display()
        )
    })?;

    for asset in &result.assets {
        let target = output_dir.join(&asset.relative_path);
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("failed creating asset directory {}", parent.display()))?;
        }
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(&asset.base64)
            .with_context(|| format!("failed decoding asset {}", asset.relative_path))?;
        fs::write(&target, bytes)
            .with_context(|| format!("failed writing asset {}", target.display()))?;
    }

    let markdown_path = output_dir.join(format!("{safe_name}.md"));
    fs::write(&markdown_path, &result.markdown)
        .with_context(|| format!("failed writing markdown {}", markdown_path.display()))?;
    Ok(Some(markdown_path.display().to_string()))
}

fn sanitize_name(value: &str) -> String {
    let sanitized = value
        .replace('/', "-")
        .replace('\\', "-")
        .replace(':', "-")
        .replace("..", "-")
        .trim()
        .chars()
        .take(100)
        .collect::<String>();
    if sanitized.is_empty() {
        "output".into()
    } else {
        sanitized
    }
}

fn chrono_like_timestamp() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};

    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    now.to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct EngineConfig {
    provider: ProviderKind,
    model_id: String,
    api_key: String,
    base_url: Option<String>,
    #[serde(default = "default_prompt_template")]
    prompt_template: String,
}

impl Default for EngineConfig {
    fn default() -> Self {
        Self {
            provider: ProviderKind::OpenRouter,
            model_id: "openai/gpt-4.1-mini".into(),
            api_key: std::env::var("OPENROUTER_API_KEY").unwrap_or_default(),
            base_url: None,
            prompt_template: default_prompt_template(),
        }
    }
}

fn default_prompt_template() -> String {
    "General".into()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum ProviderKind {
    OpenRouter,
    OpenAi,
    Anthropic,
    Ollama,
}

impl ProviderKind {
    fn id_slug(&self) -> &'static str {
        match self {
            Self::OpenRouter => "open_router",
            Self::OpenAi => "open_ai",
            Self::Anthropic => "anthropic",
            Self::Ollama => "ollama",
        }
    }

    fn default_base_url(&self) -> &'static str {
        match self {
            Self::OpenRouter => "https://openrouter.ai/api/v1/chat/completions",
            Self::OpenAi => "https://api.openai.com/v1/chat/completions",
            Self::Anthropic => "https://api.anthropic.com/v1/messages",
            Self::Ollama => "http://127.0.0.1:11434/api/generate",
        }
    }

    fn label(&self) -> &'static str {
        match self {
            Self::OpenRouter => "OpenRouter",
            Self::OpenAi => "OpenAI",
            Self::Anthropic => "Anthropic",
            Self::Ollama => "Ollama",
        }
    }

    fn requires_api_key(&self) -> bool {
        !matches!(self, Self::Ollama)
    }

    fn supports_base_url(&self) -> bool {
        true
    }
}

struct EngineConfigStore {
    path: PathBuf,
}

impl EngineConfigStore {
    fn default() -> Result<Self> {
        if let Some(explicit_dir) = std::env::var_os("DUCKDOCS_CONFIG_DIR") {
            return Ok(Self {
                path: PathBuf::from(explicit_dir).join("engine-config.json"),
            });
        }

        let home =
            dirs::home_dir().ok_or_else(|| anyhow!("failed to resolve user home directory"))?;
        Ok(Self {
            path: home.join(".duckdocs/engine-config.json"),
        })
    }

    fn load(&self) -> Result<EngineConfig> {
        if !self.path.exists() {
            let config = EngineConfig::default();
            self.save(&config)?;
            return Ok(config);
        }

        let contents = fs::read_to_string(&self.path)
            .with_context(|| format!("failed reading {}", self.path.display()))?;
        serde_json::from_str(&contents)
            .with_context(|| format!("failed decoding {}", self.path.display()))
    }

    fn save(&self, config: &EngineConfig) -> Result<()> {
        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent).with_context(|| {
                format!("failed creating config directory {}", parent.display())
            })?;
        }
        let payload =
            serde_json::to_string_pretty(config).context("failed encoding engine config")?;
        fs::write(&self.path, payload)
            .with_context(|| format!("failed writing {}", self.path.display()))
    }
}

impl EngineConfig {
    fn to_payload(&self) -> EngineConfigPayload {
        let provider_options = ProviderKind::all()
            .into_iter()
            .map(|provider| ProviderOption {
                id: provider.id_slug().to_string(),
                label: provider.label().to_string(),
                requires_api_key: provider.requires_api_key(),
                supports_base_url: provider.supports_base_url(),
            })
            .collect();

        EngineConfigPayload {
            provider: self.provider.id_slug().to_string(),
            model_id: self.model_id.clone(),
            api_key: self.api_key.clone(),
            base_url: self.base_url.clone(),
            prompt_template: self.prompt_template.clone(),
            provider_options,
            model_options: model_options_for(&self.provider)
                .into_iter()
                .map(str::to_string)
                .collect(),
            prompt_template_options: prompt_template_options()
                .into_iter()
                .map(str::to_string)
                .collect(),
        }
    }

    fn from_payload(payload: EngineConfigPayload) -> Self {
        Self {
            provider: ProviderKind::from_slug(&payload.provider)
                .unwrap_or(ProviderKind::OpenRouter),
            model_id: payload.model_id,
            api_key: payload.api_key,
            base_url: payload.base_url,
            prompt_template: payload.prompt_template,
        }
    }
}

impl ProviderKind {
    fn all() -> [ProviderKind; 4] {
        [
            Self::OpenRouter,
            Self::OpenAi,
            Self::Anthropic,
            Self::Ollama,
        ]
    }

    fn from_slug(value: &str) -> Option<Self> {
        match value {
            "open_router" => Some(Self::OpenRouter),
            "open_ai" => Some(Self::OpenAi),
            "anthropic" => Some(Self::Anthropic),
            "ollama" => Some(Self::Ollama),
            _ => None,
        }
    }
}

fn prompt_template_options() -> [&'static str; 6] {
    [
        "General",
        "API Documentation",
        "UI Flow",
        "Tutorial",
        "Code Snippets",
        "Data Tables",
    ]
}

fn model_options_for(provider: &ProviderKind) -> Vec<&'static str> {
    match provider {
        ProviderKind::OpenRouter => vec![
            "openai/gpt-4.1-nano",
            "openai/gpt-4o",
            "openai/gpt-4o-mini",
            "openai/gpt-4-turbo",
        ],
        ProviderKind::OpenAi => vec![
            "gpt-4o",
            "gpt-4o-mini",
            "gpt-4-turbo",
            "gpt-4.1",
            "gpt-4.1-mini",
            "gpt-4.1-nano",
        ],
        ProviderKind::Anthropic => vec![
            "claude-sonnet-4-20250514",
            "claude-3-5-sonnet-20241022",
            "claude-3-opus-20240229",
        ],
        ProviderKind::Ollama => vec![
            "qwen3-vl:8b",
            "qwen3-vl:latest",
            "qwen2.5vl:7b",
            "gemma3:12b",
            "llama3.2-vision:11b",
        ],
    }
}

fn validate_provider(config: &EngineConfig) -> ValidateProviderResponseData {
    let mut issues = Vec::new();
    if config.provider.requires_api_key() && config.api_key.trim().is_empty() {
        issues.push(ValidationIssue {
            code: "missing_api_key".into(),
            message: format!("{} requires an API key.", config.provider.label()),
        });
    }

    if config.model_id.trim().is_empty() {
        issues.push(ValidationIssue {
            code: "missing_model_id".into(),
            message: "A model ID is required.".into(),
        });
    }

    if let Some(base_url) = &config.base_url {
        if !base_url.trim().is_empty()
            && !(base_url.starts_with("http://") || base_url.starts_with("https://"))
        {
            issues.push(ValidationIssue {
                code: "invalid_base_url".into(),
                message: "Base URL must start with http:// or https://".into(),
            });
        }
    }

    ValidateProviderResponseData {
        ready: issues.is_empty(),
        issues,
    }
}

fn parse_image_with_provider(
    config: &EngineConfig,
    image_bytes: &[u8],
    template: &str,
) -> Result<String> {
    if provider_unavailable(config) {
        return Ok(format!(
            "_DuckDocs fallback parse._\n\nProvider `{}` is not configured or reachable, so this page was packaged as an image-only placeholder.\n\n- Template: {}\n- Image bytes: {}\n",
            config.provider.id_slug(),
            template,
            image_bytes.len()
        ));
    }

    let image_base64 = base64::engine::general_purpose::STANDARD.encode(image_bytes);
    let prompt = format!(
        "Convert this document page into clean markdown. Template: {template}. Preserve headings, lists, tables, and code blocks where possible."
    );
    match config.provider {
        ProviderKind::OpenRouter | ProviderKind::OpenAi => {
            parse_openai_compatible(config, &prompt, Some(image_base64), None)
        }
        ProviderKind::Anthropic => parse_anthropic(config, &prompt, Some(image_base64), None),
        ProviderKind::Ollama => parse_ollama(config, &prompt, Some(image_base64), None),
    }
}

fn parse_text_with_provider(config: &EngineConfig, text: &str, template: &str) -> Result<String> {
    if provider_unavailable(config) {
        return Ok(format!(
            "_DuckDocs fallback parse._\n\nProvider `{}` is not configured or reachable, so this document was returned from extracted text.\n\n- Template: {}\n\n{}",
            config.provider.id_slug(),
            template,
            text
        ));
    }

    let prompt = format!(
        "Convert the following extracted document text into clean markdown. Template: {template}.\n\n{text}"
    );
    match config.provider {
        ProviderKind::OpenRouter | ProviderKind::OpenAi => {
            parse_openai_compatible(config, &prompt, None, None)
        }
        ProviderKind::Anthropic => parse_anthropic(config, &prompt, None, None),
        ProviderKind::Ollama => parse_ollama(config, &prompt, None, Some(text)),
    }
}

fn provider_unavailable(config: &EngineConfig) -> bool {
    match config.provider {
        ProviderKind::OpenRouter | ProviderKind::OpenAi | ProviderKind::Anthropic => {
            config.api_key.trim().is_empty()
        }
        ProviderKind::Ollama => false,
    }
}

fn parse_openai_compatible(
    config: &EngineConfig,
    prompt: &str,
    image_base64: Option<String>,
    text_override: Option<&str>,
) -> Result<String> {
    let client = Client::new();
    let mut content = vec![serde_json::json!({ "type": "text", "text": prompt })];
    if let Some(image_base64) = image_base64 {
        content.push(serde_json::json!({
            "type": "image_url",
            "image_url": { "url": format!("data:image/png;base64,{image_base64}") }
        }));
    }
    if let Some(text) = text_override {
        content.push(serde_json::json!({ "type": "text", "text": text }));
    }

    let body = serde_json::json!({
        "model": config.model_id,
        "messages": [{ "role": "user", "content": content }],
    });
    let response = client
        .post(
            config
                .base_url
                .clone()
                .unwrap_or_else(|| config.provider.default_base_url().to_string()),
        )
        .bearer_auth(config.api_key.clone())
        .json(&body)
        .send()
        .context("failed to send provider request")?;
    let response = response
        .error_for_status()
        .context("provider returned error status")?;
    let json: serde_json::Value = response
        .json()
        .context("failed to decode provider response")?;
    json["choices"][0]["message"]["content"]
        .as_str()
        .map(|value| value.to_string())
        .ok_or_else(|| anyhow!("provider response did not include markdown text"))
}

fn parse_anthropic(
    config: &EngineConfig,
    prompt: &str,
    image_base64: Option<String>,
    _text_override: Option<&str>,
) -> Result<String> {
    let client = Client::new();
    let mut content = vec![serde_json::json!({ "type": "text", "text": prompt })];
    if let Some(image_base64) = image_base64 {
        content.push(serde_json::json!({
            "type": "image",
            "source": {
                "type": "base64",
                "media_type": "image/png",
                "data": image_base64
            }
        }));
    }
    let body = serde_json::json!({
        "model": config.model_id,
        "max_tokens": 4096,
        "messages": [{ "role": "user", "content": content }]
    });
    let response = client
        .post(
            config
                .base_url
                .clone()
                .unwrap_or_else(|| config.provider.default_base_url().to_string()),
        )
        .header("x-api-key", &config.api_key)
        .header("anthropic-version", "2023-06-01")
        .json(&body)
        .send()
        .context("failed to send anthropic request")?;
    let response = response
        .error_for_status()
        .context("anthropic returned error status")?;
    let json: serde_json::Value = response
        .json()
        .context("failed to decode anthropic response")?;
    json["content"][0]["text"]
        .as_str()
        .map(|value| value.to_string())
        .ok_or_else(|| anyhow!("anthropic response did not include markdown text"))
}

fn parse_ollama(
    config: &EngineConfig,
    prompt: &str,
    image_base64: Option<String>,
    text_override: Option<&str>,
) -> Result<String> {
    let client = Client::new();
    let prompt = match text_override {
        Some(text) => format!("{prompt}\n\n{text}"),
        None => prompt.to_string(),
    };
    let body = if let Some(image_base64) = image_base64 {
        serde_json::json!({
            "model": config.model_id,
            "prompt": prompt,
            "images": [image_base64],
            "stream": false
        })
    } else {
        serde_json::json!({
            "model": config.model_id,
            "prompt": prompt,
            "stream": false
        })
    };
    let mut request = client.post(
        config
            .base_url
            .clone()
            .unwrap_or_else(|| config.provider.default_base_url().to_string()),
    );
    if !config.api_key.is_empty() {
        request = request.bearer_auth(config.api_key.clone());
    }
    let response = request
        .json(&body)
        .send()
        .context("failed to send ollama request")?;
    let response = response
        .error_for_status()
        .context("ollama returned error status")?;
    let json: serde_json::Value = response
        .json()
        .context("failed to decode ollama response")?;
    json["response"]
        .as_str()
        .map(|value| value.to_string())
        .ok_or_else(|| anyhow!("ollama response did not include markdown text"))
}
