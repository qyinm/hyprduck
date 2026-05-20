use std::time::Duration;

use anyhow::{anyhow, Context, Result};
use async_openai::types::chat::{
    ChatCompletionRequestMessage, ChatCompletionRequestMessageContentPartImage,
    ChatCompletionRequestMessageContentPartText, ChatCompletionRequestUserMessage,
    ChatCompletionRequestUserMessageContent, ChatCompletionRequestUserMessageContentPart,
    CreateChatCompletionRequest, CreateChatCompletionRequestArgs, ImageUrl, ResponseFormat,
    ResponseFormatJsonSchema,
};
use reqwest::blocking::Client;
use serde_json::Value;

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
    let mut client_builder = Client::builder();
    if let Some(timeout) = timeout {
        client_builder = client_builder.timeout(timeout);
    }
    let client = client_builder.build().map_err(|error| {
        provider_failure(
            ProviderFailureKind::ProviderConfig,
            format!("failed to build provider HTTP client: {error:#}"),
        )
    })?;
    let mut http_request = client
        .post(format!(
            "{}/chat/completions",
            openai_compatible_api_base(config)
        ))
        .json(&request);
    if !config.api_key.trim().is_empty() {
        http_request = http_request.bearer_auth(config.api_key.trim());
    }
    let response = http_request.send().map_err(|error| {
        let kind = if error.is_timeout() {
            ProviderFailureKind::ProviderTimeout
        } else {
            ProviderFailureKind::ProviderConfig
        };
        provider_failure(
            kind,
            format!("failed to complete provider request: {error:#}"),
        )
    })?;
    let status = response.status();
    let response_json = response.text().map_err(|error| {
        provider_failure(
            ProviderFailureKind::ProviderResponseInvalid,
            format!("failed to read provider response body: {error:#}"),
        )
    })?;
    if !status.is_success() {
        return Err(provider_failure(
            ProviderFailureKind::ProviderConfig,
            format!("provider returned HTTP {status}: {response_json}"),
        ));
    }
    let value = serde_json::from_str::<Value>(&response_json).map_err(|error| {
        provider_failure(
            ProviderFailureKind::ProviderResponseInvalid,
            format!("failed to decode provider response JSON: {error:#}"),
        )
    })?;
    extract_chat_completion_content(&value).ok_or_else(|| {
        provider_failure(
            ProviderFailureKind::ProviderResponseInvalid,
            "provider response did not include markdown text",
        )
    })
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

fn extract_chat_completion_content(value: &Value) -> Option<String> {
    value
        .get("choices")?
        .as_array()?
        .first()?
        .get("message")?
        .get("content")?
        .as_str()
        .map(str::to_string)
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
    fn chat_completion_content_extraction_ignores_extra_provider_fields() {
        let response = serde_json::json!({
            "id": "gen-test",
            "object": "chat.completion",
            "service_tier": "standard",
            "choices": [
                {
                    "index": 0,
                    "message": {
                        "role": "assistant",
                        "content": "{\"ok\":true}"
                    }
                }
            ]
        });

        assert_eq!(
            extract_chat_completion_content(&response).as_deref(),
            Some("{\"ok\":true}")
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
