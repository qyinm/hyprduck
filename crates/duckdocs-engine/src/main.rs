use std::fs;
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Instant;

use anyhow::{anyhow, bail, Context, Result};
use base64::Engine;
use duckdocs_engine_types::{
    DocumentFormat, OutputAsset, ParseEvent, ParseInput, ParseMetadata, ParseOptions,
    ParseRequest, ParseResult, ParsedPage,
};
use reqwest::blocking::Client;
use serde::{Deserialize, Serialize};
use tempfile::tempdir;

fn main() {
    if let Err(error) = run() {
        let _ = emit_event(&ParseEvent::Failed {
            message: error.to_string(),
        });
        eprintln!("{error:?}");
        std::process::exit(1);
    }
}

fn run() -> Result<()> {
    let mut payload = String::new();
    io::stdin().read_to_string(&mut payload).context("failed to read parse request")?;
    let request: ParseRequest = serde_json::from_str(&payload).context("failed to decode parse request JSON")?;
    maybe_write_debug(&request.options.debug_request_path, &payload)?;

    let started = Instant::now();
    let config_store = EngineConfigStore::default()?;
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

    let output = serde_json::to_string_pretty(&result).context("failed to encode parse result")?;
    maybe_write_debug(&request.options.debug_result_path, &output)?;
    emit_event(&ParseEvent::Completed)?;
    io::stdout().write_all(output.as_bytes()).context("failed to write parse result")?;
    Ok(())
}

fn maybe_write_debug(path: &Option<String>, contents: &str) -> Result<()> {
    if let Some(path) = path {
        if let Some(parent) = Path::new(path).parent() {
            fs::create_dir_all(parent).with_context(|| format!("failed creating debug directory {}", parent.display()))?;
        }
        fs::write(path, contents).with_context(|| format!("failed writing debug artifact {}", path))?;
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

fn parse_document(
    input: &ParseInput,
    template: &str,
    _options: &ParseOptions,
    config: &EngineConfig,
) -> Result<ParsedDocument> {
    match input.format {
        DocumentFormat::Pdf | DocumentFormat::Image => parse_visual_document(input, template, config),
        DocumentFormat::Docx | DocumentFormat::Doc => parse_text_document(input, template, config),
    }
}

fn parse_visual_document(input: &ParseInput, template: &str, config: &EngineConfig) -> Result<ParsedDocument> {
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

fn parse_text_document(input: &ParseInput, template: &str, config: &EngineConfig) -> Result<ParsedDocument> {
    let text = extract_text_via_textutil(Path::new(&input.path))?;
    emit_event(&ParseEvent::ConvertingPages { current: 1, total: 1 })?;
    emit_event(&ParseEvent::Parsing { current: 1, total: 1 })?;

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
        .with_context(|| format!("failed listing converted PDF pages in {}", temp.path().display()))?
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
        bail!("text extraction produced empty output for {}", path.display());
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

#[derive(Debug, Clone, Serialize, Deserialize)]
struct EngineConfig {
    provider: ProviderKind,
    model_id: String,
    api_key: String,
    base_url: Option<String>,
}

impl Default for EngineConfig {
    fn default() -> Self {
        Self {
            provider: ProviderKind::OpenRouter,
            model_id: "openai/gpt-4.1-mini".into(),
            api_key: std::env::var("OPENROUTER_API_KEY").unwrap_or_default(),
            base_url: None,
        }
    }
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
            Self::OpenRouter => "openrouter",
            Self::OpenAi => "openai",
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

        let home = dirs::home_dir().ok_or_else(|| anyhow!("failed to resolve user home directory"))?;
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

        let contents =
            fs::read_to_string(&self.path).with_context(|| format!("failed reading {}", self.path.display()))?;
        serde_json::from_str(&contents).with_context(|| format!("failed decoding {}", self.path.display()))
    }

    fn save(&self, config: &EngineConfig) -> Result<()> {
        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("failed creating config directory {}", parent.display()))?;
        }
        let payload = serde_json::to_string_pretty(config).context("failed encoding engine config")?;
        fs::write(&self.path, payload).with_context(|| format!("failed writing {}", self.path.display()))
    }
}

fn parse_image_with_provider(config: &EngineConfig, image_bytes: &[u8], template: &str) -> Result<String> {
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
        ProviderKind::OpenRouter | ProviderKind::OpenAi => parse_openai_compatible(config, &prompt, None, None),
        ProviderKind::Anthropic => parse_anthropic(config, &prompt, None, None),
        ProviderKind::Ollama => parse_ollama(config, &prompt, None, Some(text)),
    }
}

fn provider_unavailable(config: &EngineConfig) -> bool {
    match config.provider {
        ProviderKind::OpenRouter | ProviderKind::OpenAi | ProviderKind::Anthropic => config.api_key.trim().is_empty(),
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
        .post(config.base_url.clone().unwrap_or_else(|| config.provider.default_base_url().to_string()))
        .bearer_auth(config.api_key.clone())
        .json(&body)
        .send()
        .context("failed to send provider request")?;
    let response = response.error_for_status().context("provider returned error status")?;
    let json: serde_json::Value = response.json().context("failed to decode provider response")?;
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
        .post(config.base_url.clone().unwrap_or_else(|| config.provider.default_base_url().to_string()))
        .header("x-api-key", &config.api_key)
        .header("anthropic-version", "2023-06-01")
        .json(&body)
        .send()
        .context("failed to send anthropic request")?;
    let response = response.error_for_status().context("anthropic returned error status")?;
    let json: serde_json::Value = response.json().context("failed to decode anthropic response")?;
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
    let mut request = client.post(config.base_url.clone().unwrap_or_else(|| config.provider.default_base_url().to_string()));
    if !config.api_key.is_empty() {
        request = request.bearer_auth(config.api_key.clone());
    }
    let response = request.json(&body).send().context("failed to send ollama request")?;
    let response = response.error_for_status().context("ollama returned error status")?;
    let json: serde_json::Value = response.json().context("failed to decode ollama response")?;
    json["response"]
        .as_str()
        .map(|value| value.to_string())
        .ok_or_else(|| anyhow!("ollama response did not include markdown text"))
}
