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

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ProviderKind {
    OpenRouter,
    Ollama,
    Unknown(String),
}

impl Serialize for ProviderKind {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(self.id_slug())
    }
}

impl ProviderKind {
    /// Deserializes a provider slug while preserving removed or future provider ids.
    /// Legacy config files still load, but unsupported providers stay explicit internally.
    fn deserialize_unknown<'de, D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let slug = String::deserialize(deserializer)?;
        Ok(Self::from_config_slug(slug))
    }

    pub(crate) fn id_slug(&self) -> &str {
        match self {
            Self::OpenRouter => "open_router",
            Self::Ollama => "ollama",
            Self::Unknown(slug) => slug.as_str(),
        }
    }

    pub(crate) fn default_base_url(&self) -> &'static str {
        match self {
            Self::OpenRouter => "https://openrouter.ai/api/v1/chat/completions",
            Self::Ollama => "http://127.0.0.1:11434/v1/chat/completions",
            Self::Unknown(_) => "https://openrouter.ai/api/v1/chat/completions",
        }
    }

    pub(crate) fn label(&self) -> &'static str {
        match self {
            Self::OpenRouter => "OpenRouter",
            Self::Ollama => "Ollama",
            Self::Unknown(_) => "Unknown provider",
        }
    }

    pub(crate) fn requires_api_key(&self) -> bool {
        matches!(self, Self::OpenRouter)
    }

    pub(crate) fn supports_base_url(&self) -> bool {
        true
    }

    pub(crate) fn uses_openai_compatible_chat_api(&self) -> bool {
        matches!(self, Self::OpenRouter | Self::Ollama)
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

    pub(crate) fn from_config_slug(value: String) -> Self {
        Self::from_slug(&value).unwrap_or(Self::Unknown(value))
    }

    pub(crate) fn is_unknown(&self) -> bool {
        matches!(self, Self::Unknown(_))
    }
}

pub(crate) struct EngineConfigStore {
    pub(crate) path: PathBuf,
}

impl EngineConfigStore {
    pub(crate) fn default() -> Result<Self> {
        if let Some(explicit_dir) = std::env::var_os("ETYMA_CONFIG_DIR") {
            return Ok(Self {
                path: PathBuf::from(explicit_dir).join("engine-config.json"),
            });
        }

        let home =
            dirs::home_dir().ok_or_else(|| anyhow!("failed to resolve user home directory"))?;
        Ok(Self {
            path: home.join(".etyma/engine-config.json"),
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
            provider: ProviderKind::from_config_slug(payload.provider),
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
    if provider.is_unknown() {
        return Vec::new();
    }

    etyma_engine_types::model_options_for(provider.id_slug())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn payload_with_provider(provider: &str) -> EngineConfigPayload {
        EngineConfigPayload {
            provider: provider.into(),
            model_id: "test-model".into(),
            api_key: "test-key".into(),
            base_url: None,
            prompt_template: "General".into(),
            provider_options: Vec::new(),
            model_options: Vec::new(),
            prompt_template_options: Vec::new(),
        }
    }

    #[test]
    fn config_decode_preserves_unknown_provider_slug() {
        let decoded: EngineConfig = serde_json::from_value(serde_json::json!({
            "provider": "legacy_ai",
            "model_id": "legacy-model",
            "api_key": "legacy-key",
            "base_url": null,
            "prompt_template": "General"
        }))
        .expect("legacy config should still decode");

        assert!(matches!(
            decoded.provider,
            ProviderKind::Unknown(ref slug) if slug == "legacy_ai"
        ));
    }

    #[test]
    fn payload_decode_preserves_unknown_provider_slug() {
        let config = EngineConfig::from_payload(payload_with_provider("legacy_ai"));

        assert!(matches!(
            config.provider,
            ProviderKind::Unknown(ref slug) if slug == "legacy_ai"
        ));

        let surfaced = config.to_payload();
        assert_eq!(surfaced.provider, "legacy_ai");
        assert!(surfaced.model_options.is_empty());
    }

    #[test]
    fn provider_strategy_surface_stays_launch_scoped() {
        let launch_providers = ProviderKind::all()
            .into_iter()
            .map(|provider| provider.id_slug().to_string())
            .collect::<Vec<_>>();

        assert_eq!(launch_providers, ["open_router", "ollama"]);
        assert!(ProviderKind::OpenRouter.uses_openai_compatible_chat_api());
        assert!(ProviderKind::Ollama.uses_openai_compatible_chat_api());
        assert!(!ProviderKind::Unknown("openai".into()).uses_openai_compatible_chat_api());
        assert!(!ProviderKind::Unknown("anthropic".into()).uses_openai_compatible_chat_api());
    }
}
