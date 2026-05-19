use std::time::Duration;

use anyhow::{anyhow, Context, Result};
use async_openai::{
    config::OpenAIConfig,
    types::chat::{
        ChatCompletionRequestMessage, ChatCompletionRequestMessageContentPartImage,
        ChatCompletionRequestMessageContentPartText, ChatCompletionRequestUserMessage,
        ChatCompletionRequestUserMessageContent, ChatCompletionRequestUserMessageContentPart,
        CreateChatCompletionRequest, CreateChatCompletionRequestArgs, ImageUrl, ResponseFormat,
        ResponseFormatJsonSchema,
    },
    Client,
};

use crate::provider::{EngineConfig, ProviderKind};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ProviderFailureKind {
    ProviderConfig,
    ProviderTimeout,
    ProviderResponseInvalid,
    UnsupportedProvider,
}

impl ProviderFailureKind {
    pub(crate) fn code(self) -> &'static str {
        match self {
            Self::ProviderConfig => "provider_config",
            Self::ProviderTimeout => "provider_timeout",
            Self::ProviderResponseInvalid => "provider_response_invalid",
            Self::UnsupportedProvider => "unsupported_provider",
        }
    }
}

pub(crate) fn provider_failure(
    kind: ProviderFailureKind,
    message: impl Into<String>,
) -> anyhow::Error {
    anyhow!("{}: {}", kind.code(), message.into())
}

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
    parse_openai_compatible_with_response_format_timeout(
        config,
        prompt,
        image_base64,
        timeout,
        None,
    )
}

pub(crate) fn parse_openai_compatible_json_schema_with_timeout(
    config: &EngineConfig,
    prompt: &str,
    schema: ResponseFormatJsonSchema,
    timeout: Option<Duration>,
) -> Result<String> {
    parse_openai_compatible_with_response_format_timeout(
        config,
        prompt,
        None,
        timeout,
        Some(ResponseFormat::JsonSchema {
            json_schema: schema,
        }),
    )
}

fn parse_openai_compatible_with_response_format_timeout(
    config: &EngineConfig,
    prompt: &str,
    image_base64: Option<String>,
    timeout: Option<Duration>,
    response_format: Option<ResponseFormat>,
) -> Result<String> {
    let request = build_chat_completion_request(config, prompt, image_base64, response_format)
        .map_err(|error| {
            provider_failure(
                ProviderFailureKind::ProviderConfig,
                format!("failed to build OpenAI-compatible chat completion request: {error:#}"),
            )
        })?;
    let client = openai_compatible_client(config);
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|error| {
            provider_failure(
                ProviderFailureKind::ProviderConfig,
                format!("failed to build provider API runtime: {error:#}"),
            )
        })?;

    runtime.block_on(async {
        let chat = client.chat();
        let request_future = chat.create(request);
        let response = match timeout {
            Some(timeout) => tokio::time::timeout(timeout, request_future)
                .await
                .map_err(|_| {
                    provider_failure(
                        ProviderFailureKind::ProviderTimeout,
                        format!("provider request timed out after {timeout:?}"),
                    )
                })?,
            None => request_future.await,
        }
        .map_err(|error| {
            provider_failure(
                ProviderFailureKind::ProviderConfig,
                format!("failed to complete provider request: {error:#}"),
            )
        })?;

        response
            .choices
            .first()
            .and_then(|choice| choice.message.content.as_deref())
            .map(str::to_string)
            .ok_or_else(|| {
                provider_failure(
                    ProviderFailureKind::ProviderResponseInvalid,
                    "provider response did not include markdown text",
                )
            })
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
    response_format: Option<ResponseFormat>,
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

    let mut request = CreateChatCompletionRequestArgs::default();
    request
        .model(config.model_id.clone())
        .messages(vec![ChatCompletionRequestMessage::User(
            ChatCompletionRequestUserMessage {
                content: ChatCompletionRequestUserMessageContent::Array(parts),
                name: None,
            },
        )]);
    if let Some(response_format) = response_format {
        request.response_format(response_format);
    }

    request
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
    match &config.provider {
        ProviderKind::OpenRouter => config.api_key.trim().is_empty(),
        ProviderKind::Ollama => false,
        ProviderKind::Unknown(_) => true,
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
    fn provider_failure_messages_include_stable_taxonomy_code() {
        let error = provider_failure(
            ProviderFailureKind::ProviderResponseInvalid,
            "provider response did not include markdown text",
        );

        assert_eq!(
            error.to_string(),
            "provider_response_invalid: provider response did not include markdown text"
        );
    }

    #[test]
    fn chat_completion_request_uses_openai_content_parts_for_images() {
        let config = test_config(ProviderKind::OpenRouter, None);
        let request =
            build_chat_completion_request(&config, "Parse this page.", Some("abc".into()), None)
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

    #[test]
    fn chat_completion_request_can_use_json_schema_response_format() {
        let config = test_config(ProviderKind::OpenRouter, None);
        let request = build_chat_completion_request(
            &config,
            "Return graph records.",
            None,
            Some(ResponseFormat::JsonSchema {
                json_schema: ResponseFormatJsonSchema {
                    name: "graph_records".into(),
                    description: Some("Graph records".into()),
                    schema: Some(serde_json::json!({
                        "type": "object",
                        "properties": {
                            "records": { "type": "array", "items": { "type": "object" } }
                        },
                        "required": ["records"],
                        "additionalProperties": false
                    })),
                    strict: Some(false),
                },
            }),
        )
        .expect("request");
        let encoded = serde_json::to_value(request).expect("encode request");

        assert_eq!(encoded["response_format"]["type"], "json_schema");
        assert_eq!(
            encoded["response_format"]["json_schema"]["name"],
            "graph_records"
        );
    }
}
