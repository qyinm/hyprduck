use std::time::Duration;

use anyhow::{anyhow, Context, Result};
use async_openai::{
    config::OpenAIConfig,
    types::chat::{
        ChatCompletionRequestMessage, ChatCompletionRequestMessageContentPartImage,
        ChatCompletionRequestMessageContentPartText, ChatCompletionRequestUserMessage,
        ChatCompletionRequestUserMessageContent, ChatCompletionRequestUserMessageContentPart,
        CreateChatCompletionRequest, CreateChatCompletionRequestArgs, ImageUrl,
    },
    Client,
};

use crate::provider::{EngineConfig, ProviderKind};

pub(crate) fn parse_openai_compatible(
    config: &EngineConfig,
    prompt: &str,
    image_base64: Option<String>,
) -> Result<String> {
    parse_openai_compatible_with_timeout(config, prompt, image_base64, None)
}

pub(crate) fn parse_openai_compatible_with_timeout(
    config: &EngineConfig,
    prompt: &str,
    image_base64: Option<String>,
    timeout: Option<Duration>,
) -> Result<String> {
    let request = build_chat_completion_request(config, prompt, image_base64)?;
    let client = openai_compatible_client(config);
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .context("failed to build provider API runtime")?;

    runtime.block_on(async {
        let chat = client.chat();
        let request_future = chat.create(request);
        let response = match timeout {
            Some(timeout) => tokio::time::timeout(timeout, request_future)
                .await
                .map_err(|_| anyhow!("provider request timed out after {timeout:?}"))?,
            None => request_future.await,
        }
        .map_err(|error| anyhow!("failed to complete provider request: {error:#}"))?;

        response
            .choices
            .first()
            .and_then(|choice| choice.message.content.as_deref())
            .map(str::to_string)
            .ok_or_else(|| anyhow!("provider response did not include markdown text"))
    })
}

fn openai_compatible_client(config: &EngineConfig) -> Client<OpenAIConfig> {
    let openai_config = OpenAIConfig::new()
        .with_api_base(openai_compatible_api_base(config))
        .with_api_key(config.api_key.clone());

    Client::with_config(openai_config)
}

fn build_chat_completion_request(
    config: &EngineConfig,
    prompt: &str,
    image_base64: Option<String>,
) -> Result<CreateChatCompletionRequest> {
    let mut parts = vec![ChatCompletionRequestUserMessageContentPart::Text(
        ChatCompletionRequestMessageContentPartText {
            text: prompt.to_string(),
        },
    )];

    if let Some(image_base64) = image_base64 {
        parts.push(ChatCompletionRequestUserMessageContentPart::ImageUrl(
            ChatCompletionRequestMessageContentPartImage {
                image_url: ImageUrl {
                    url: format!("data:image/png;base64,{image_base64}"),
                    detail: None,
                },
            },
        ));
    }

    CreateChatCompletionRequestArgs::default()
        .model(config.model_id.clone())
        .messages(vec![ChatCompletionRequestMessage::User(
            ChatCompletionRequestUserMessage {
                content: ChatCompletionRequestUserMessageContent::Array(parts),
                name: None,
            },
        )])
        .build()
        .context("failed to build OpenAI-compatible chat completion request")
}

fn openai_compatible_api_base(config: &EngineConfig) -> String {
    let base_url = config
        .base_url
        .clone()
        .filter(|url| !url.trim().is_empty())
        .unwrap_or_else(|| config.provider.default_base_url().to_string());

    normalize_openai_compatible_api_base(&base_url)
}

fn normalize_openai_compatible_api_base(raw: &str) -> String {
    let trimmed = raw.trim().trim_end_matches('/');
    trimmed
        .strip_suffix("/chat/completions")
        .unwrap_or(trimmed)
        .to_string()
}

pub(crate) fn provider_unavailable(config: &EngineConfig) -> bool {
    match config.provider {
        ProviderKind::OpenRouter => config.api_key.trim().is_empty(),
        ProviderKind::Ollama => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_config(provider: ProviderKind, base_url: Option<&str>) -> EngineConfig {
        EngineConfig {
            provider,
            model_id: "test-model".into(),
            api_key: "test-key".into(),
            base_url: base_url.map(str::to_string),
            prompt_template: "General".into(),
        }
    }

    #[test]
    fn openai_compatible_base_strips_chat_completion_endpoint() {
        let config = test_config(
            ProviderKind::OpenRouter,
            Some("https://openrouter.ai/api/v1/chat/completions"),
        );

        assert_eq!(
            openai_compatible_api_base(&config),
            "https://openrouter.ai/api/v1"
        );
    }

    #[test]
    fn openai_compatible_base_accepts_api_root() {
        let config = test_config(ProviderKind::Ollama, Some("http://127.0.0.1:11434/v1"));

        assert_eq!(
            openai_compatible_api_base(&config),
            "http://127.0.0.1:11434/v1"
        );
    }

    #[test]
    fn provider_availability_keeps_ollama_keyless() {
        let mut config = test_config(ProviderKind::Ollama, None);
        config.api_key.clear();

        assert!(!provider_unavailable(&config));
    }

    #[test]
    fn chat_completion_request_uses_openai_content_parts_for_images() {
        let config = test_config(ProviderKind::OpenRouter, None);
        let request =
            build_chat_completion_request(&config, "Parse this page.", Some("abc".into()))
                .expect("request");
        let encoded = serde_json::to_value(request).expect("encode request");

        assert_eq!(encoded["model"], "test-model");
        assert_eq!(encoded["messages"][0]["role"], "user");
        assert_eq!(encoded["messages"][0]["content"][0]["type"], "text");
        assert_eq!(
            encoded["messages"][0]["content"][1]["image_url"]["url"],
            "data:image/png;base64,abc"
        );
    }
}
