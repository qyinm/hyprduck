use anyhow::{anyhow, bail, Context, Result};
use futures::StreamExt;
use etyma_engine_types::AgentChatStreamEvent;
use rig_core::agent::{MultiTurnStreamItem, StreamingResult};
use rig_core::client::{CompletionClient, Nothing};
use rig_core::completion::Prompt;
use rig_core::providers::{ollama, openrouter};
use rig_core::streaming::{StreamedAssistantContent, StreamingPrompt};

use super::state::AgentChatEventSink;
use crate::provider::{EngineConfig, ProviderKind};

pub(super) const DEFAULT_MAX_TOKENS: u64 = 1_200;

pub(super) fn run_rig_agent(
    config: &EngineConfig,
    preamble: &str,
    context: &str,
    prompt: &str,
) -> Result<String> {
    match &config.provider {
        ProviderKind::OpenRouter => run_openrouter_agent(config, preamble, context, prompt),
        ProviderKind::Ollama => run_ollama_agent(config, preamble, context, prompt),
        ProviderKind::Unknown(slug) => bail!(
            "provider_config: provider `{slug}` is not supported for Agent chat. Select OpenRouter or Ollama."
        ),
    }
}

pub(super) fn run_rig_agent_stream(
    config: &EngineConfig,
    preamble: &str,
    context: &str,
    prompt: &str,
    emit: AgentChatEventSink<'_>,
) -> Result<String> {
    match &config.provider {
        ProviderKind::OpenRouter => run_openrouter_agent_stream(config, preamble, context, prompt, emit),
        ProviderKind::Ollama => run_ollama_agent_stream(config, preamble, context, prompt, emit),
        ProviderKind::Unknown(slug) => bail!(
            "provider_config: provider `{slug}` is not supported for Agent chat. Select OpenRouter or Ollama."
        ),
    }
}

pub(super) fn run_openrouter_agent(
    config: &EngineConfig,
    preamble: &str,
    context: &str,
    prompt: &str,
) -> Result<String> {
    if config.api_key.trim().is_empty() {
        bail!("provider_config: OpenRouter requires an API key before Agent chat can run.");
    }
    let client = openrouter::Client::builder()
        .with_app_identity("Etyma", "https://etyma.local")
        .api_key(config.api_key.trim())
        .build()
        .context("provider_config: failed to create Rig OpenRouter client")?;
    let agent = client
        .agent(config.model_id.as_str())
        .preamble(preamble)
        .context(context)
        .temperature(0.2)
        .max_tokens(DEFAULT_MAX_TOKENS)
        .build();
    block_on_prompt(agent.prompt(prompt).max_turns(2).with_tool_concurrency(2))
}

pub(super) fn run_openrouter_agent_stream(
    config: &EngineConfig,
    preamble: &str,
    context: &str,
    prompt: &str,
    emit: AgentChatEventSink<'_>,
) -> Result<String> {
    if config.api_key.trim().is_empty() {
        bail!("provider_config: OpenRouter requires an API key before Agent chat can run.");
    }
    let client = openrouter::Client::builder()
        .with_app_identity("Etyma", "https://etyma.local")
        .api_key(config.api_key.trim())
        .build()
        .context("provider_config: failed to create Rig OpenRouter client")?;
    let agent = client
        .agent(config.model_id.as_str())
        .preamble(preamble)
        .context(context)
        .temperature(0.2)
        .max_tokens(DEFAULT_MAX_TOKENS)
        .build();
    block_on_stream(agent.stream_prompt(prompt.to_string()).multi_turn(2), emit)
}

pub(super) fn run_ollama_agent(
    config: &EngineConfig,
    preamble: &str,
    context: &str,
    prompt: &str,
) -> Result<String> {
    let mut builder = ollama::Client::builder().api_key(Nothing);
    if let Some(base_url) = normalized_ollama_base_url(config.base_url.as_deref()) {
        builder = builder.base_url(base_url);
    }
    let client = builder
        .build()
        .context("provider_config: failed to create Rig Ollama client")?;
    let agent = client
        .agent(config.model_id.as_str())
        .preamble(preamble)
        .context(context)
        .temperature(0.2)
        .max_tokens(DEFAULT_MAX_TOKENS)
        .build();
    block_on_prompt(
        agent
            .prompt(prompt)
            .max_turns(2)
            .with_tool_concurrency(2),
    )
    .map_err(|error| {
        anyhow!(
            "provider_unavailable: Ollama is not available for Agent chat. Confirm Ollama is running and model `{}` is pulled. {error}",
            config.model_id
        )
    })
}

pub(super) fn run_ollama_agent_stream(
    config: &EngineConfig,
    preamble: &str,
    context: &str,
    prompt: &str,
    emit: AgentChatEventSink<'_>,
) -> Result<String> {
    let mut builder = ollama::Client::builder().api_key(Nothing);
    if let Some(base_url) = normalized_ollama_base_url(config.base_url.as_deref()) {
        builder = builder.base_url(base_url);
    }
    let client = builder
        .build()
        .context("provider_config: failed to create Rig Ollama client")?;
    let agent = client
        .agent(config.model_id.as_str())
        .preamble(preamble)
        .context(context)
        .temperature(0.2)
        .max_tokens(DEFAULT_MAX_TOKENS)
        .build();
    block_on_stream(agent.stream_prompt(prompt.to_string()).multi_turn(2), emit).map_err(|error| {
        anyhow!(
            "provider_unavailable: Ollama is not available for Agent chat. Confirm Ollama is running and model `{}` is pulled. {error}",
            config.model_id
        )
    })
}

pub(super) fn block_on_prompt<F>(request: F) -> Result<String>
where
    F: std::future::IntoFuture<
        Output = std::result::Result<String, rig_core::completion::PromptError>,
    >,
{
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .context("provider_config: failed to start Agent chat runtime")?;
    runtime.block_on(request.into_future()).map_err(|error| {
        anyhow!("provider_unavailable: provider completion failed for Agent chat. {error}")
    })
}

pub(super) fn block_on_stream<F, R>(request: F, emit: AgentChatEventSink<'_>) -> Result<String>
where
    F: std::future::IntoFuture<Output = StreamingResult<R>>,
    R: Clone + Unpin + rig_core::completion::GetTokenUsage + 'static,
{
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .context("provider_config: failed to start Agent chat runtime")?;
    runtime.block_on(async move {
        let mut stream = request.into_future().await;
        let mut output = String::new();
        while let Some(item) = stream.next().await {
            match item.map_err(|error| {
                anyhow!("provider_unavailable: provider streaming failed for Agent chat. {error}")
            })? {
                MultiTurnStreamItem::StreamAssistantItem(StreamedAssistantContent::Text(text)) => {
                    if !text.text.is_empty() {
                        output.push_str(&text.text);
                        emit(AgentChatStreamEvent::Delta { text: text.text })?;
                    }
                }
                MultiTurnStreamItem::FinalResponse(response) if output.is_empty() => {
                    output = response.response().to_string();
                }
                MultiTurnStreamItem::StreamAssistantItem(StreamedAssistantContent::Final(_))
                | MultiTurnStreamItem::StreamAssistantItem(StreamedAssistantContent::Reasoning(
                    _,
                ))
                | MultiTurnStreamItem::StreamAssistantItem(
                    StreamedAssistantContent::ReasoningDelta { .. },
                )
                | MultiTurnStreamItem::StreamAssistantItem(StreamedAssistantContent::ToolCall {
                    ..
                })
                | MultiTurnStreamItem::StreamAssistantItem(
                    StreamedAssistantContent::ToolCallDelta { .. },
                )
                | MultiTurnStreamItem::StreamUserItem(_)
                | MultiTurnStreamItem::CompletionCall(_)
                | MultiTurnStreamItem::FinalResponse(_) => {}
                _ => {}
            }
        }
        Ok(output)
    })
}

pub(super) fn normalized_ollama_base_url(value: Option<&str>) -> Option<String> {
    let value = value?.trim();
    if value.is_empty() {
        return None;
    }
    Some(
        value
            .trim_end_matches("/v1/chat/completions")
            .trim_end_matches("/v1")
            .trim_end_matches('/')
            .to_string(),
    )
}
