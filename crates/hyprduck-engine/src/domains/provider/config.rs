use super::*;
use serde::{Deserialize, Serialize};

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

    pub(crate) fn default_base_url(&self) -> &'static str {
        match self {
            Self::OpenRouter => "https://openrouter.ai/api/v1/chat/completions",
            Self::Ollama => "http://127.0.0.1:11434/v1/chat/completions",
        }
    }

    pub(crate) fn label(&self) -> &'static str {
        match self {
            Self::OpenRouter => "OpenRouter",
            Self::Ollama => "Ollama",
        }
    }

    pub(crate) fn requires_api_key(&self) -> bool {
        !matches!(self, Self::Ollama)
    }

    pub(crate) fn supports_base_url(&self) -> bool {
        true
    }

    pub(crate) fn all() -> [ProviderKind; 2] {
        [Self::OpenRouter, Self::Ollama]
    }

    pub(crate) fn from_slug(value: &str) -> Option<Self> {
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
        if let Some(explicit_dir) = std::env::var_os("HYPRDUCK_CONFIG_DIR") {
            return Ok(Self {
                path: PathBuf::from(explicit_dir).join("engine-config.json"),
            });
        }

        let home =
            dirs::home_dir().ok_or_else(|| anyhow!("failed to resolve user home directory"))?;
        Ok(Self {
            path: home.join(".hyprduck/engine-config.json"),
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

pub(crate) fn model_options_for(provider: &ProviderKind) -> Vec<&'static str> {
    hyprduck_engine_types::model_options_for(provider.id_slug())
}
