use std::fs;
use std::path::PathBuf;
use std::time::Duration;

use anyhow::{anyhow, Context, Result};
use base64::Engine;
use duckdocs_engine_types::{
    EngineConfigPayload, ProviderModelCatalogResponseData, ProviderOption, ReadinessCheck,
    RuntimeReadinessResponseData, ValidateProviderResponseData, ValidationIssue,
};
use reqwest::{blocking::Client, Url};
use serde::{Deserialize, Serialize};

use crate::resolve_binary;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct EngineConfig {
    #[serde(deserialize_with = "ProviderKind::deserialize_unknown")]
    pub(crate) provider: ProviderKind,
    pub(crate) model_id: String,
    pub(crate) api_key: String,
    pub(crate) base_url: Option<String>,
    #[serde(default = "default_prompt_template")]
    pub(crate) prompt_template: String,
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
pub(crate) enum ProviderKind {
    OpenRouter,
    Ollama,
}

impl ProviderKind {
    /// Deserializes a provider slug, falling back to `OpenRouter` for unknown values.
    /// This handles legacy config files that may contain removed providers like `open_ai` or `anthropic`.
    fn deserialize_unknown<'de, D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let slug = String::deserialize(deserializer)?;
        Ok(Self::from_slug(&slug).unwrap_or(Self::OpenRouter))
    }

    pub(crate) fn id_slug(&self) -> &'static str {
        match self {
            Self::OpenRouter => "open_router",
            Self::Ollama => "ollama",
        }
    }

    fn default_base_url(&self) -> &'static str {
        match self {
            Self::OpenRouter => "https://openrouter.ai/api/v1/chat/completions",
            Self::Ollama => "http://127.0.0.1:11434/v1/chat/completions",
        }
    }

    fn label(&self) -> &'static str {
        match self {
            Self::OpenRouter => "OpenRouter",
            Self::Ollama => "Ollama",
        }
    }

    fn requires_api_key(&self) -> bool {
        !matches!(self, Self::Ollama)
    }

    fn supports_base_url(&self) -> bool {
        true
    }

    fn all() -> [ProviderKind; 2] {
        [Self::OpenRouter, Self::Ollama]
    }

    fn from_slug(value: &str) -> Option<Self> {
        match value {
            "open_router" => Some(Self::OpenRouter),
            "ollama" => Some(Self::Ollama),
            _ => None,
        }
    }
}

pub(crate) struct EngineConfigStore {
    pub(crate) path: PathBuf,
}

impl EngineConfigStore {
    pub(crate) fn default() -> Result<Self> {
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

    pub(crate) fn load(&self) -> Result<EngineConfig> {
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

    pub(crate) fn save(&self, config: &EngineConfig) -> Result<()> {
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
    pub(crate) fn to_payload(&self) -> EngineConfigPayload {
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

    pub(crate) fn from_payload(payload: EngineConfigPayload) -> Self {
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
    duckdocs_engine_types::model_options_for(provider.id_slug())
}

pub(crate) fn provider_model_catalog() -> ProviderModelCatalogResponseData {
    let provider_models = ProviderKind::all()
        .into_iter()
        .map(|provider| {
            (
                provider.id_slug().to_string(),
                model_options_for(&provider)
                    .into_iter()
                    .map(str::to_string)
                    .collect(),
            )
        })
        .collect();

    ProviderModelCatalogResponseData {
        provider_models,
        ollama_vision_prefixes: duckdocs_engine_types::ollama_vision_prefixes()
            .into_iter()
            .map(str::to_string)
            .collect(),
    }
}

pub(crate) fn validate_provider(config: &EngineConfig) -> ValidateProviderResponseData {
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

pub(crate) fn check_readiness(config_store: &EngineConfigStore) -> RuntimeReadinessResponseData {
    let mut checks = vec![ReadinessCheck {
        id: "runtime_process".into(),
        label: "Runtime process".into(),
        ready: true,
        required: true,
        message: "Runtime process is accepting commands.".into(),
    }];

    let config = match config_store.load() {
        Ok(config) => {
            checks.push(ReadinessCheck {
                id: "config_file".into(),
                label: "Engine config".into(),
                ready: true,
                required: true,
                message: format!("Loaded {}", config_store.path.display()),
            });
            config
        }
        Err(error) => {
            checks.push(ReadinessCheck {
                id: "config_file".into(),
                label: "Engine config".into(),
                ready: false,
                required: true,
                message: error.to_string(),
            });
            return RuntimeReadinessResponseData {
                ready: false,
                provider: "unknown".into(),
                model_id: String::new(),
                checks,
            };
        }
    };

    let validation = validate_provider(&config);
    checks.push(ReadinessCheck {
        id: "provider_config".into(),
        label: "Provider config".into(),
        ready: validation.ready,
        required: true,
        message: if validation.ready {
            format!(
                "{} is configured with model {}.",
                config.provider.label(),
                config.model_id
            )
        } else {
            validation
                .issues
                .iter()
                .map(|issue| issue.message.as_str())
                .collect::<Vec<_>>()
                .join(" ")
        },
    });

    if matches!(config.provider, ProviderKind::Ollama) {
        checks.push(check_ollama_endpoint(&config));
    }

    checks.push(check_path_exists(
        "pdf_converter",
        "PDF converter",
        "pdftoppm",
        &["/opt/homebrew/bin/pdftoppm", "/usr/local/bin/pdftoppm"],
        false,
    ));
    checks.push(check_path_exists(
        "text_converter",
        "DOC/DOCX text converter",
        "textutil",
        &["/usr/bin/textutil"],
        false,
    ));
    checks.push(check_path_exists(
        "knowledge_store",
        "Knowledge store",
        "sqlite3",
        &["/usr/bin/sqlite3"],
        true,
    ));

    RuntimeReadinessResponseData {
        ready: checks
            .iter()
            .filter(|check| check.required)
            .all(|check| check.ready),
        provider: config.provider.id_slug().into(),
        model_id: config.model_id,
        checks,
    }
}

fn check_path_exists(
    id: &str,
    label: &str,
    binary_name: &str,
    common_paths: &[&str],
    required: bool,
) -> ReadinessCheck {
    let path = resolve_binary(binary_name, common_paths);
    let ready = path.exists();
    ReadinessCheck {
        id: id.into(),
        label: label.into(),
        ready,
        required,
        message: if ready {
            format!("Found {}", path.display())
        } else {
            format!("Missing {binary_name} in PATH or common install locations")
        },
    }
}

fn check_ollama_endpoint(config: &EngineConfig) -> ReadinessCheck {
    let endpoint = ollama_models_endpoint(config);
    let result = Client::builder()
        .timeout(Duration::from_secs(2))
        .connect_timeout(Duration::from_secs(1))
        .build()
        .and_then(|client| client.get(&endpoint).send())
        .and_then(|response| response.error_for_status().map(|_| ()));

    match result {
        Ok(()) => ReadinessCheck {
            id: "ollama_endpoint".into(),
            label: "Ollama endpoint".into(),
            ready: true,
            required: true,
            message: format!("Ollama responded at {endpoint}."),
        },
        Err(error) => ReadinessCheck {
            id: "ollama_endpoint".into(),
            label: "Ollama endpoint".into(),
            ready: false,
            required: true,
            message: format!("Ollama is not reachable at {endpoint}: {error}"),
        },
    }
}

pub(crate) fn ollama_models_endpoint(config: &EngineConfig) -> String {
    let raw = config
        .base_url
        .clone()
        .filter(|url| !url.trim().is_empty())
        .unwrap_or_else(|| config.provider.default_base_url().to_string())
        .replace("/v1/chat/completions", "/v1/models")
        .replace("/api/generate", "/api/tags");

    if let Ok(mut url) = Url::parse(&raw) {
        let path = url.path().trim_end_matches('/');
        if path.is_empty() {
            url.set_path("/v1/models");
            return url.to_string();
        }
        if path == "/v1" {
            url.set_path("/v1/models");
            return url.to_string();
        }
        if path == "/api" {
            url.set_path("/api/tags");
            return url.to_string();
        }
    }

    raw
}

pub(crate) fn parse_image_with_provider(
    config: &EngineConfig,
    image_bytes: &[u8],
    template: &str,
) -> Result<String> {
    if provider_unavailable(config) {
        return Ok(format!(
            "_HyprDuck fallback parse._\n\nProvider `{}` is not configured or reachable, so this page was packaged as an image-only placeholder.\n\n- Template: {}\n- Image bytes: {}\n",
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
        ProviderKind::OpenRouter | ProviderKind::Ollama => {
            parse_openai_compatible(config, &prompt, Some(image_base64))
        }
    }
}

pub(crate) fn parse_text_with_provider(
    config: &EngineConfig,
    text: &str,
    template: &str,
) -> Result<String> {
    if provider_unavailable(config) {
        return Ok(format!(
            "_HyprDuck fallback parse._\n\nProvider `{}` is not configured or reachable, so this document was returned from extracted text.\n\n- Template: {}\n\n{}",
            config.provider.id_slug(),
            template,
            text
        ));
    }

    let prompt = format!(
        "Convert the following extracted document text into clean markdown. Template: {template}.\n\n{text}"
    );
    match config.provider {
        ProviderKind::OpenRouter | ProviderKind::Ollama => {
            parse_openai_compatible(config, &prompt, None)
        }
    }
}

pub(crate) fn provider_unavailable(config: &EngineConfig) -> bool {
    match config.provider {
        ProviderKind::OpenRouter => config.api_key.trim().is_empty(),
        ProviderKind::Ollama => false,
    }
}

pub(crate) fn parse_openai_compatible(
    config: &EngineConfig,
    prompt: &str,
    image_base64: Option<String>,
) -> Result<String> {
    let client = Client::builder()
        .timeout(None)
        .connect_timeout(Duration::from_secs(10))
        .build()
        .context("failed to build provider HTTP client")?;
    let mut content = vec![serde_json::json!({ "type": "text", "text": prompt })];
    if let Some(image_base64) = image_base64 {
        content.push(serde_json::json!({
            "type": "image_url",
            "image_url": { "url": format!("data:image/png;base64,{image_base64}") }
        }));
    }

    let body = serde_json::json!({
        "model": config.model_id,
        "messages": [{ "role": "user", "content": content }],
    });
    let endpoint = config
        .base_url
        .clone()
        .filter(|u| !u.trim().is_empty())
        .unwrap_or_else(|| config.provider.default_base_url().to_string());
    let response = client
        .post(&endpoint)
        .bearer_auth(config.api_key.clone())
        .json(&body)
        .send()
        .map_err(|error| anyhow!("failed to send provider request to {endpoint}: {error:#}"))?;
    let response = response
        .error_for_status()
        .map_err(|error| anyhow!("provider returned error status from {endpoint}: {error:#}"))?;
    let json: serde_json::Value = response.json().map_err(|error| {
        anyhow!("failed to decode provider response from {endpoint}: {error:#}")
    })?;
    json["choices"][0]["message"]["content"]
        .as_str()
        .map(|value| value.to_string())
        .ok_or_else(|| anyhow!("provider response did not include markdown text"))
}
