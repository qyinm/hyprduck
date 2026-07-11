use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProviderOption {
    pub id: String,
    pub label: String,
    pub requires_api_key: bool,
    pub supports_base_url: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EngineConfigPayload {
    pub provider: String,
    pub model_id: String,
    pub api_key: String,
    #[serde(default)]
    pub base_url: Option<String>,
    pub prompt_template: String,
    #[serde(default)]
    pub provider_options: Vec<ProviderOption>,
    #[serde(default)]
    pub model_options: Vec<String>,
    #[serde(default)]
    pub prompt_template_options: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SaveConfigResponseData {
    pub config: EngineConfigPayload,
    pub persisted: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ValidationIssue {
    pub code: String,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ValidateProviderResponseData {
    pub ready: bool,
    #[serde(default)]
    pub issues: Vec<ValidationIssue>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct ListProviderModelsRequest {}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProviderModelCatalogResponseData {
    pub provider_models: BTreeMap<String, Vec<String>>,
    pub ollama_vision_prefixes: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct CheckReadinessRequest {}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReadinessCheck {
    pub id: String,
    pub label: String,
    pub ready: bool,
    #[serde(default = "default_readiness_required")]
    pub required: bool,
    pub message: String,
}

fn default_readiness_required() -> bool {
    true
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RuntimeReadinessResponseData {
    pub ready: bool,
    pub provider: String,
    pub model_id: String,
    #[serde(default)]
    pub checks: Vec<ReadinessCheck>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LoadConfigRequest {}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SaveConfigRequest {
    pub config: EngineConfigPayload,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ValidateProviderRequest {
    #[serde(default)]
    pub config: Option<EngineConfigPayload>,
}

/// Returns the list of supported model IDs for a given provider slug.
/// Single source of truth — used by both the engine and the desktop UI.
pub fn model_options_for(provider_slug: &str) -> Vec<&'static str> {
    match provider_slug {
        "open_router" => vec![
            "google/gemma-4-31b-it",
            "z-ai/glm-5v-turbo",
            "anthropic/claude-sonnet-4.6",
            "anthropic/claude-opus-4.6",
            "google/gemini-3-flash-preview",
            "qwen/qwen3.6-plus:free",
            "x-ai/grok-4.1-fast",
            "google/gemini-2.5-flash-lite",
            "google/gemini-2.5-flash",
            "moonshotai/kimi-k2.5",
        ],
        "ollama" => vec![
            "gemma4:latest",
            "qwen3.5:latest",
            "qwen3-vl:8b",
            "qwen3-vl:72b",
            "kimi-k2.5:latest",
            "glm-ocr:latest",
            "deepseek-ocr:latest",
        ],
        _ => Vec::new(),
    }
}

/// Prefixes used to identify local Ollama models that can process page images.
pub fn ollama_vision_prefixes() -> Vec<&'static str> {
    vec![
        "gemma4",
        "qwen3.5",
        "qwen3-vl",
        "kimi-k2.5",
        "glm-ocr",
        "deepseek-ocr",
    ]
}
