use std::collections::BTreeSet;
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{anyhow, bail, Context, Result};
use futures::StreamExt;
use hyprduck_engine_types::{
    AgentChatAnswerMode, AgentChatAskRequest, AgentChatAskResponseData, AgentChatMessage,
    AgentChatMessageRole, AgentChatProviderSummary, AgentChatScopeMode, AgentChatStreamEvent,
    AgentChatStreamStatus, AnswerResponse, AnswerStatus, ContextPackEvidenceTypeTraceV1,
    ContextPackEvidenceV1, ContextPackFindingV0, ContextPackRetrievalTraceV1, ContextPackSourceV0,
    ContextPackV1, ContextPackWarningV0, EvidenceRef, GetContextPackRequest, SuggestedAction,
    SuggestedActionKind, AGENT_CHAT_SCHEMA_VERSION,
};
use rig_core::agent::{MultiTurnStreamItem, StreamingResult};
use rig_core::client::{CompletionClient, Nothing};
use rig_core::completion::Prompt;
use rig_core::providers::{ollama, openrouter};
use rig_core::streaming::{StreamedAssistantContent, StreamingPrompt};

use crate::application::services::context_pack_service;
use crate::domains::retrieval::brain_search::db_search_terms;
use crate::provider::{EngineConfig, ProviderKind};

const MAX_QUESTION_CHARS: usize = 4_000;
const MAX_HISTORY_MESSAGES: usize = 24;
const MAX_HISTORY_CHARS: usize = 16_000;
const DEFAULT_CONTEXT_BUDGET: usize = 8_000;
const DEFAULT_MAX_TOKENS: u64 = 1_200;
const EVIDENCE_AGENT_PREAMBLE: &str = r#"You are HyprDuck's local document evidence agent.
Answer only from the supplied context pack.
When you use evidence, cite the exact evidenceRef in square brackets, for example [ev_abc123].
If the context is insufficient, say what is missing instead of guessing.
Do not expose local filesystem paths."#;
const GENERAL_AGENT_PREAMBLE: &str = r#"You are HyprDuck's agent chat assistant.
Answer the user's general question directly.
No citation-ready HyprDuck document context was supplied for this turn, so do not invent citations or evidence references.
Do not expose local filesystem paths."#;

type AgentChatEventSink<'a> = &'a mut dyn FnMut(AgentChatStreamEvent) -> Result<()>;

pub(crate) fn handle_agent_chat_ask(
    request: AgentChatAskRequest,
    config: &EngineConfig,
) -> Result<AgentChatAskResponseData> {
    execute_agent_chat(request, config, None)
}

pub(crate) fn handle_agent_chat_stream(
    request: AgentChatAskRequest,
    config: &EngineConfig,
    emit: AgentChatEventSink<'_>,
) -> Result<AgentChatAskResponseData> {
    execute_agent_chat(request, config, Some(emit))
}

fn execute_agent_chat(
    request: AgentChatAskRequest,
    config: &EngineConfig,
    mut emit: Option<AgentChatEventSink<'_>>,
) -> Result<AgentChatAskResponseData> {
    emit_status(
        &mut emit,
        AgentChatStreamStatus::ResolvingScope,
        "Resolving scope...",
    )?;
    validate_agent_chat_request(&request)?;
    let provider = provider_summary(config);

    emit_status(
        &mut emit,
        AgentChatStreamStatus::ClassifyingQuestion,
        "Classifying question...",
    )?;
    let general_intent = should_answer_as_general_chat(&request);
    let should_retrieve_context = should_retrieve_context(&request, general_intent);
    let context_query = build_context_query(&request);

    let (context_pack, persisted_context_pack_path) = if should_retrieve_context {
        emit_status(
            &mut emit,
            AgentChatStreamStatus::RetrievingContext,
            "Retrieving context...",
        )?;
        let selected_node_id = match request.mode {
            AgentChatScopeMode::GraphContext => request.selected_node_id.clone(),
            AgentChatScopeMode::Auto
            | AgentChatScopeMode::AllDocs
            | AgentChatScopeMode::SelectedSource => None,
        };
        let context_response =
            context_pack_service::handle_get_context_pack(GetContextPackRequest {
                scope: request.scope.clone(),
                query: context_query.clone(),
                selected_node_id,
                budget: request.budget.or(Some(DEFAULT_CONTEXT_BUDGET)),
                persist: false,
            })?;
        let filtered_context_pack =
            filter_context_pack_for_scope(&context_response.context_pack_v1, &request);
        let persisted_context_pack_path = if request.persist_context_pack {
            Some(context_pack_service::persist_context_pack_v1(
                &request.scope,
                &filtered_context_pack,
            )?)
        } else {
            None
        };
        (filtered_context_pack, persisted_context_pack_path)
    } else {
        (empty_context_pack(&request), None)
    };

    let has_context = has_citation_ready_context(&context_pack);
    let answer_mode = classify_answer_mode(&request, general_intent, has_context);
    emit_agent_event(
        &mut emit,
        AgentChatStreamEvent::Started {
            conversation_id: request.conversation_id.clone(),
            assistant_message_id: assistant_message_id(&request),
            provider: provider.clone(),
            answer_mode: Some(answer_mode),
        },
    )?;

    let mut warnings = context_pack
        .warnings
        .iter()
        .map(|warning| warning.message.clone())
        .collect::<Vec<_>>();

    if matches!(answer_mode, AgentChatAnswerMode::Blocked) {
        let text =
            "I could not find citation-ready document context for this question.".to_string();
        let response = build_response(
            &request,
            &context_pack,
            AgentChatAnswerMode::Blocked,
            provider,
            persisted_context_pack_path,
            text,
            AnswerStatus::Blocked,
            Vec::new(),
            warnings,
            "The question asks for document or graph-grounded facts, but no citation-ready context matched.",
        );
        emit_status(&mut emit, AgentChatStreamStatus::Complete, "Complete.")?;
        emit_agent_event(
            &mut emit,
            AgentChatStreamEvent::Final {
                result: response.clone(),
            },
        )?;
        return Ok(response);
    }

    let (preamble, context, prompt) = match answer_mode {
        AgentChatAnswerMode::Evidence => (
            EVIDENCE_AGENT_PREAMBLE,
            build_context_document(&context_pack),
            build_evidence_user_prompt(&request, &context_pack),
        ),
        AgentChatAnswerMode::General => {
            warnings
                .push("Answered as general chat without citation-ready document context.".into());
            (
                GENERAL_AGENT_PREAMBLE,
                "No citation-ready HyprDuck document context was retrieved for this turn."
                    .to_string(),
                build_general_user_prompt(&request),
            )
        }
        AgentChatAnswerMode::Blocked => unreachable!("blocked mode is returned above"),
    };

    emit_status(
        &mut emit,
        AgentChatStreamStatus::ConnectingProvider,
        "Connecting provider...",
    )?;
    emit_status(
        &mut emit,
        AgentChatStreamStatus::Generating,
        "Generating...",
    )?;

    let model_text = if let Some(sink) = emit.as_deref_mut() {
        run_rig_agent_stream(config, preamble, &context, &prompt, sink)?
    } else {
        run_rig_agent(config, preamble, &context, &prompt)?
    };

    emit_status(
        &mut emit,
        AgentChatStreamStatus::ValidatingCitations,
        "Validating citations...",
    )?;
    let mut citations = if matches!(answer_mode, AgentChatAnswerMode::Evidence) {
        validate_model_citations(&model_text, &context_pack)
    } else {
        Vec::new()
    };
    let mut answer_status = if matches!(answer_mode, AgentChatAnswerMode::General) {
        AnswerStatus::LowConfidence
    } else if citations.is_empty() {
        AnswerStatus::LowConfidence
    } else {
        AnswerStatus::Grounded
    };

    if citations.is_empty()
        && matches!(answer_mode, AgentChatAnswerMode::Evidence)
        && !context_pack.selected_evidence.is_empty()
    {
        citations = context_pack
            .selected_evidence
            .iter()
            .take(3)
            .cloned()
            .collect();
        warnings.push(
            "The model did not cite evidenceRefs; HyprDuck attached top context evidence as fallback."
                .into(),
        );
        answer_status = AnswerStatus::LowConfidence;
    }
    if !citations.is_empty() {
        emit_agent_event(
            &mut emit,
            AgentChatStreamEvent::CitationUpdate {
                citations: citations.clone(),
            },
        )?;
    }

    let response = build_response(
        &request,
        &context_pack,
        answer_mode,
        provider,
        persisted_context_pack_path,
        model_text,
        answer_status,
        citations,
        warnings,
        if matches!(answer_mode, AgentChatAnswerMode::General) {
            "Answered as general chat without document citations."
        } else {
            "Answered with the selected context pack evidence."
        },
    );
    emit_status(&mut emit, AgentChatStreamStatus::Complete, "Complete.")?;
    emit_agent_event(
        &mut emit,
        AgentChatStreamEvent::Final {
            result: response.clone(),
        },
    )?;
    Ok(response)
}

fn validate_agent_chat_request(request: &AgentChatAskRequest) -> Result<()> {
    if request.schema_version != AGENT_CHAT_SCHEMA_VERSION {
        bail!(
            "invalid_request: unsupported agent chat schemaVersion {}",
            request.schema_version
        );
    }
    if request.conversation_id.trim().is_empty() {
        bail!("invalid_request: conversationId is required");
    }
    let question = request.question.trim();
    if question.is_empty() {
        bail!("invalid_request: question is required");
    }
    if question.chars().count() > MAX_QUESTION_CHARS {
        bail!("invalid_request: question exceeds {MAX_QUESTION_CHARS} characters");
    }
    if request.history.len() > MAX_HISTORY_MESSAGES {
        bail!("invalid_request: history exceeds {MAX_HISTORY_MESSAGES} messages");
    }
    let history_chars = request
        .history
        .iter()
        .map(|message| message.text.chars().count())
        .sum::<usize>();
    if history_chars > MAX_HISTORY_CHARS {
        bail!("invalid_request: history exceeds {MAX_HISTORY_CHARS} characters");
    }
    if matches!(request.mode, AgentChatScopeMode::SelectedSource) && request.source_ids.is_empty() {
        bail!("invalid_request: selected_source mode requires at least one sourceId");
    }
    if matches!(request.mode, AgentChatScopeMode::GraphContext)
        && request
            .selected_node_id
            .as_deref()
            .unwrap_or_default()
            .is_empty()
    {
        bail!("invalid_request: graph_context mode requires selectedNodeId");
    }
    Ok(())
}

fn filter_context_pack_for_scope(
    context_pack: &ContextPackV1,
    request: &AgentChatAskRequest,
) -> ContextPackV1 {
    if !matches!(request.mode, AgentChatScopeMode::SelectedSource) || request.source_ids.is_empty()
    {
        return context_pack.clone();
    }

    let allowed_sources = request.source_ids.iter().cloned().collect::<BTreeSet<_>>();
    let selected_evidence = context_pack
        .selected_evidence
        .iter()
        .filter(|evidence| allowed_sources.contains(&evidence.source_id))
        .cloned()
        .collect::<Vec<_>>();
    let source_set = context_pack
        .source_set
        .iter()
        .filter(|source| allowed_sources.contains(&source.source_id))
        .cloned()
        .collect::<Vec<_>>();

    ContextPackV1 {
        selected_evidence,
        source_set,
        ..context_pack.clone()
    }
}

fn provider_summary(config: &EngineConfig) -> AgentChatProviderSummary {
    AgentChatProviderSummary {
        id: config.provider.id_slug().into(),
        label: config.provider.label().into(),
        model_id: config.model_id.clone(),
        hosted: matches!(config.provider, ProviderKind::OpenRouter),
    }
}

fn emit_status(
    emit: &mut Option<AgentChatEventSink<'_>>,
    status: AgentChatStreamStatus,
    message: impl Into<String>,
) -> Result<()> {
    emit_agent_event(
        emit,
        AgentChatStreamEvent::Status {
            status,
            message: message.into(),
        },
    )
}

fn emit_agent_event(
    emit: &mut Option<AgentChatEventSink<'_>>,
    event: AgentChatStreamEvent,
) -> Result<()> {
    if let Some(sink) = emit.as_deref_mut() {
        sink(event)?;
    }
    Ok(())
}

fn has_citation_ready_context(context_pack: &ContextPackV1) -> bool {
    !context_pack.selected_evidence.is_empty() || !context_pack.source_set.is_empty()
}

fn should_answer_as_general_chat(request: &AgentChatAskRequest) -> bool {
    is_general_chat_question(&request.question)
}

fn should_retrieve_context(request: &AgentChatAskRequest, general_intent: bool) -> bool {
    !general_intent
        && (matches!(
            request.mode,
            AgentChatScopeMode::Auto
                | AgentChatScopeMode::AllDocs
                | AgentChatScopeMode::SelectedSource
                | AgentChatScopeMode::GraphContext
        ) || looks_like_evidence_question(&request.question))
}

fn explicit_evidence_scope(request: &AgentChatAskRequest) -> bool {
    matches!(
        request.mode,
        AgentChatScopeMode::AllDocs
            | AgentChatScopeMode::SelectedSource
            | AgentChatScopeMode::GraphContext
    )
}

fn classify_answer_mode(
    request: &AgentChatAskRequest,
    general_intent: bool,
    has_context: bool,
) -> AgentChatAnswerMode {
    if general_intent {
        return AgentChatAnswerMode::General;
    }
    if has_context {
        return AgentChatAnswerMode::Evidence;
    }
    if explicit_evidence_scope(request) || looks_like_evidence_question(&request.question) {
        AgentChatAnswerMode::Blocked
    } else {
        AgentChatAnswerMode::General
    }
}

fn build_context_query(request: &AgentChatAskRequest) -> String {
    let question = request.question.trim();
    if !db_search_terms(question).is_empty() {
        return question.into();
    }

    request
        .history
        .iter()
        .rev()
        .filter(|message| matches!(message.role, AgentChatMessageRole::User))
        .map(|message| message.text.trim())
        .find(|text| !text.is_empty() && !db_search_terms(text).is_empty())
        .map(|previous_question| format!("{previous_question} {question}"))
        .unwrap_or_else(|| question.into())
}

fn looks_like_evidence_question(question: &str) -> bool {
    let normalized = question.trim().to_lowercase();
    let keywords = [
        "document",
        "documents",
        "doc",
        "docs",
        "source",
        "sources",
        "citation",
        "citations",
        "evidence",
        "graph",
        "node",
        "context",
        "pdf",
        "docx",
        "file",
        "files",
        "page",
        "pages",
        "summarize",
        "summary",
        "문서",
        "자료",
        "파일",
        "출처",
        "근거",
        "인용",
        "그래프",
        "노드",
        "페이지",
        "요약",
        "정리",
    ];
    keywords.iter().any(|keyword| normalized.contains(keyword))
}

fn is_general_chat_question(question: &str) -> bool {
    let normalized = question
        .trim()
        .trim_matches(|ch: char| {
            ch.is_ascii_punctuation()
                || matches!(
                    ch,
                    '。' | '，' | '、' | '！' | '？' | '…' | '·' | 'ㅋ' | 'ㅎ'
                )
        })
        .to_lowercase();
    let compact = normalized.split_whitespace().collect::<Vec<_>>().join(" ");
    matches!(
        compact.as_str(),
        "hi" | "hello"
            | "hey"
            | "yo"
            | "good morning"
            | "good afternoon"
            | "good evening"
            | "thanks"
            | "thank you"
            | "안녕"
            | "안녕하세요"
            | "하이"
            | "고마워"
            | "고맙습니다"
            | "감사합니다"
            | "반가워"
            | "반갑습니다"
            | "뭐 할 수 있어"
            | "무엇을 할 수 있어"
            | "what can you do"
    )
}

fn empty_context_pack(request: &AgentChatAskRequest) -> ContextPackV1 {
    let budget = request.budget.unwrap_or(DEFAULT_CONTEXT_BUDGET);
    ContextPackV1 {
        schema_version: "hyprduck.context_pack.v1".into(),
        pack_id: format!("ctx_empty_{}", uuid::Uuid::now_v7().simple()),
        workspace_id: request.scope.workspace_id.clone(),
        query: request.question.clone(),
        generated_at: unix_timestamp().to_string(),
        source_set: Vec::<ContextPackSourceV0>::new(),
        selected_evidence: Vec::<ContextPackEvidenceV1>::new(),
        findings: Vec::<ContextPackFindingV0>::new(),
        warnings: Vec::<ContextPackWarningV0>::new(),
        retrieval_trace: ContextPackRetrievalTraceV1 {
            strategy: "general_chat".into(),
            chunks_considered: 0,
            chunks_selected: 0,
            budget_requested: budget,
            budget_used: 0,
            evidence_type_trace: ContextPackEvidenceTypeTraceV1::default(),
        },
        suggested_next_reads: Vec::new(),
    }
}

fn run_rig_agent(
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

fn run_rig_agent_stream(
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

fn run_openrouter_agent(
    config: &EngineConfig,
    preamble: &str,
    context: &str,
    prompt: &str,
) -> Result<String> {
    if config.api_key.trim().is_empty() {
        bail!("provider_config: OpenRouter requires an API key before Agent chat can run.");
    }
    let client = openrouter::Client::builder()
        .with_app_identity("HyprDuck", "https://hyprduck.local")
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
    block_on_prompt(agent.prompt(prompt))
}

fn run_openrouter_agent_stream(
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
        .with_app_identity("HyprDuck", "https://hyprduck.local")
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
    block_on_stream(agent.stream_prompt(prompt.to_string()), emit)
}

fn run_ollama_agent(
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
    block_on_prompt(agent.prompt(prompt)).map_err(|error| {
        anyhow!(
            "provider_unavailable: Ollama is not available for Agent chat. Confirm Ollama is running and model `{}` is pulled. {error}",
            config.model_id
        )
    })
}

fn run_ollama_agent_stream(
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
    block_on_stream(agent.stream_prompt(prompt.to_string()), emit).map_err(|error| {
        anyhow!(
            "provider_unavailable: Ollama is not available for Agent chat. Confirm Ollama is running and model `{}` is pulled. {error}",
            config.model_id
        )
    })
}

fn block_on_prompt<F>(request: F) -> Result<String>
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

fn block_on_stream<F, R>(request: F, emit: AgentChatEventSink<'_>) -> Result<String>
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

fn normalized_ollama_base_url(value: Option<&str>) -> Option<String> {
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

fn build_evidence_user_prompt(
    request: &AgentChatAskRequest,
    context_pack: &ContextPackV1,
) -> String {
    let history = format_recent_history(&request.history);
    let scope_line = match request.mode {
        AgentChatScopeMode::Auto => {
            "Scope: automatically selected indexed document evidence".to_string()
        }
        AgentChatScopeMode::AllDocs => "Scope: all indexed documents".to_string(),
        AgentChatScopeMode::SelectedSource => {
            format!("Scope: selected sources {}", request.source_ids.join(", "))
        }
        AgentChatScopeMode::GraphContext => format!(
            "Scope: graph context for node {}",
            request.selected_node_id.as_deref().unwrap_or("unknown")
        ),
    };
    format!(
        "{scope_line}\nContext pack: {}\nConversation history:\n{}\n\nQuestion:\n{}",
        context_pack.pack_id,
        if history.is_empty() {
            "(no prior messages)"
        } else {
            history.as_str()
        },
        request.question.trim()
    )
}

fn build_general_user_prompt(request: &AgentChatAskRequest) -> String {
    let history = format_recent_history(&request.history);
    format!(
        "Conversation history:\n{}\n\nQuestion:\n{}",
        if history.is_empty() {
            "(no prior messages)"
        } else {
            history.as_str()
        },
        request.question.trim()
    )
}

fn format_recent_history(history: &[AgentChatMessage]) -> String {
    history
        .iter()
        .rev()
        .take(8)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .map(|message| format!("{:?}: {}", message.role, message.text))
        .collect::<Vec<_>>()
        .join("\n")
}

fn build_context_document(context_pack: &ContextPackV1) -> String {
    let sources = context_pack
        .source_set
        .iter()
        .map(|source| {
            format!(
                "- sourceId={} filename={} pages={} status={} localOnly={}",
                source.source_id,
                source.original_filename,
                source.page_count,
                source.ingestion_status,
                source.local_only
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    let evidence = context_pack
        .selected_evidence
        .iter()
        .map(|evidence| {
            format!(
                "[{}] sourceId={} page={} confidence={:?}\n{}",
                evidence.evidence_ref,
                evidence.source_id,
                evidence.page,
                evidence.parse_confidence,
                evidence.quoted_text
            )
        })
        .collect::<Vec<_>>()
        .join("\n\n");
    format!(
        "Context pack id: {}\nWorkspace id: {}\nSources:\n{}\n\nEvidence:\n{}",
        context_pack.pack_id, context_pack.workspace_id, sources, evidence
    )
}

fn validate_model_citations(
    model_text: &str,
    context_pack: &ContextPackV1,
) -> Vec<ContextPackEvidenceV1> {
    let valid_refs = context_pack
        .selected_evidence
        .iter()
        .map(|evidence| evidence.evidence_ref.as_str())
        .collect::<BTreeSet<_>>();
    let mut seen = BTreeSet::new();
    let cited_refs = model_text
        .split(|ch: char| !(ch.is_ascii_alphanumeric() || ch == '_' || ch == '-' || ch == ':'))
        .filter(|token| valid_refs.contains(*token))
        .filter(|token| seen.insert((*token).to_string()))
        .collect::<Vec<_>>();

    cited_refs
        .iter()
        .filter_map(|evidence_ref| {
            context_pack
                .selected_evidence
                .iter()
                .find(|evidence| evidence.evidence_ref == *evidence_ref)
                .cloned()
        })
        .collect()
}

fn build_response(
    request: &AgentChatAskRequest,
    context_pack: &ContextPackV1,
    answer_mode: AgentChatAnswerMode,
    provider: AgentChatProviderSummary,
    persisted_context_pack_path: Option<String>,
    text: String,
    status: AnswerStatus,
    citations: Vec<ContextPackEvidenceV1>,
    warnings: Vec<String>,
    explanation: &str,
) -> AgentChatAskResponseData {
    let message = AgentChatMessage {
        id: assistant_message_id(request),
        role: AgentChatMessageRole::Assistant,
        text: text.clone(),
        created_at: unix_timestamp(),
    };
    let answer = AnswerResponse {
        status,
        text: Some(text),
        explanation: explanation.into(),
        citations: citations.iter().map(evidence_to_answer_citation).collect(),
        related_node_ids: Vec::new(),
        suggested_actions: if matches!(status, AnswerStatus::Blocked) {
            vec![SuggestedAction {
                kind: SuggestedActionKind::ReimportProject,
                label: "Add Docs".into(),
                description: "Add or refresh documents before asking the agent.".into(),
            }]
        } else {
            Vec::new()
        },
    };

    AgentChatAskResponseData {
        schema_version: AGENT_CHAT_SCHEMA_VERSION.into(),
        conversation_id: request.conversation_id.clone(),
        answer_mode,
        assistant_message: message,
        answer,
        context_pack_id: context_pack.pack_id.clone(),
        persisted_context_pack_path,
        citations,
        retrieval_trace: context_pack.retrieval_trace.clone(),
        provider,
        warnings,
    }
}

fn assistant_message_id(request: &AgentChatAskRequest) -> String {
    request
        .assistant_message_id
        .clone()
        .filter(|id| !id.trim().is_empty())
        .unwrap_or_else(|| format!("msg_{}", uuid::Uuid::now_v7().simple()))
}

fn evidence_to_answer_citation(evidence: &ContextPackEvidenceV1) -> EvidenceRef {
    EvidenceRef {
        id: evidence.evidence_ref.clone(),
        page_label: format!("Page {}", evidence.page),
        page_index: Some(evidence.page),
        snippet: evidence.quoted_text.clone(),
        source_path: None,
        source_id: Some(evidence.source_id.clone()),
        markdown_path: None,
        image_path: None,
        provenance: Some(evidence.selection_reason.clone()),
    }
}

fn unix_timestamp() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use hyprduck_engine_types::{
        BrainReadScope, ContextPackEvidenceTypeTraceV1, ContextPackFindingV0,
        ContextPackGraphRecordKindV1, ContextPackGraphRecordV1, ContextPackGraphTrailV1,
        ContextPackParseConfidence, ContextPackRetrievalTraceV1, ContextPackSourceV0,
        ContextPackStaleness, ContextPackWarningSeverity, ContextPackWarningV0, EvidenceType,
    };

    fn request(mode: AgentChatScopeMode) -> AgentChatAskRequest {
        AgentChatAskRequest {
            schema_version: AGENT_CHAT_SCHEMA_VERSION.into(),
            conversation_id: "conversation_1".into(),
            assistant_message_id: None,
            scope: BrainReadScope {
                workspace_id: "default".into(),
                root_dir: None,
            },
            mode,
            selected_node_id: Some("node_a".into()),
            source_ids: vec!["source_a".into()],
            question: "What changed?".into(),
            history: Vec::new(),
            budget: Some(1024),
            persist_context_pack: false,
        }
    }

    fn context_pack() -> ContextPackV1 {
        ContextPackV1 {
            schema_version: "hyprduck.context_pack.v1".into(),
            pack_id: "ctx_test".into(),
            workspace_id: "default".into(),
            query: "What changed?".into(),
            generated_at: "2026-06-18T00:00:00Z".into(),
            source_set: vec![ContextPackSourceV0 {
                source_id: "source_a".into(),
                original_filename: "a.pdf".into(),
                content_hash: "hash".into(),
                page_count: 1,
                ingestion_status: "ingested".into(),
                staleness: ContextPackStaleness::Current,
                provider_route: "local".into(),
                local_only: true,
            }],
            selected_evidence: vec![ContextPackEvidenceV1 {
                evidence_ref: "ev_a".into(),
                source_id: "source_a".into(),
                page: 1,
                region: None,
                span: None,
                quoted_text: "quoted evidence".into(),
                parse_confidence: ContextPackParseConfidence::High,
                selection_reason: "top match".into(),
                content_hash: "hash".into(),
                evidence_type: EvidenceType::Text,
                graph_trail: Some(ContextPackGraphTrailV1 {
                    direct: vec![ContextPackGraphRecordV1 {
                        record_type: ContextPackGraphRecordKindV1::Evidence,
                        id: "ev_a".into(),
                        reason: "selected".into(),
                    }],
                    adjacent: Vec::new(),
                    follow_up: Vec::new(),
                    unavailable_reason: None,
                }),
            }],
            findings: Vec::<ContextPackFindingV0>::new(),
            warnings: vec![ContextPackWarningV0 {
                warning_type: "low_parse_confidence".into(),
                severity: ContextPackWarningSeverity::Low,
                message: "warning".into(),
                page_refs: Vec::new(),
            }],
            retrieval_trace: ContextPackRetrievalTraceV1 {
                strategy: "test".into(),
                chunks_considered: 1,
                chunks_selected: 1,
                budget_requested: 1024,
                budget_used: 10,
                evidence_type_trace: ContextPackEvidenceTypeTraceV1::default(),
            },
            suggested_next_reads: Vec::new(),
        }
    }

    #[test]
    fn citation_validation_drops_hallucinated_refs() {
        let pack = context_pack();
        let citations = validate_model_citations("Use [ev_a] but not [ev_fake].", &pack);
        assert_eq!(citations.len(), 1);
        assert_eq!(citations[0].evidence_ref, "ev_a");
    }

    #[test]
    fn selected_source_filters_context_pack() {
        let mut pack = context_pack();
        pack.selected_evidence.push(ContextPackEvidenceV1 {
            evidence_ref: "ev_b".into(),
            source_id: "source_b".into(),
            page: 1,
            region: None,
            span: None,
            quoted_text: "other evidence".into(),
            parse_confidence: ContextPackParseConfidence::High,
            selection_reason: "other".into(),
            content_hash: "hash_b".into(),
            evidence_type: EvidenceType::Text,
            graph_trail: None,
        });
        let filtered =
            filter_context_pack_for_scope(&pack, &request(AgentChatScopeMode::SelectedSource));
        assert_eq!(filtered.selected_evidence.len(), 1);
        assert_eq!(filtered.selected_evidence[0].source_id, "source_a");
    }

    #[test]
    fn general_greeting_uses_general_answer_mode_without_context() {
        let mut request = request(AgentChatScopeMode::Auto);
        request.question = "HI".into();
        assert!(should_answer_as_general_chat(&request));
        assert!(!should_retrieve_context(&request, true));
        assert_eq!(
            classify_answer_mode(&request, true, false),
            AgentChatAnswerMode::General
        );
    }

    #[test]
    fn auto_topic_question_retrieves_context_before_general_fallback() {
        let mut request = request(AgentChatScopeMode::Auto);
        request.question = "dynamic hashing 내용 설명해".into();
        assert!(!should_answer_as_general_chat(&request));
        assert!(should_retrieve_context(&request, false));
        assert_eq!(
            classify_answer_mode(&request, false, false),
            AgentChatAnswerMode::General
        );
        assert_eq!(build_context_query(&request), "dynamic hashing 내용 설명해");
    }

    #[test]
    fn follow_up_context_query_reuses_previous_user_topic() {
        let mut request = request(AgentChatScopeMode::Auto);
        request.history = vec![
            AgentChatMessage {
                id: "msg_user_1".into(),
                role: AgentChatMessageRole::User,
                text: "dynamic hashing 알려줘".into(),
                created_at: 1,
            },
            AgentChatMessage {
                id: "msg_assistant_1".into(),
                role: AgentChatMessageRole::Assistant,
                text: "It is in section 8.3.".into(),
                created_at: 2,
            },
        ];
        request.question = "내용 없어?".into();

        assert_eq!(
            build_context_query(&request),
            "dynamic hashing 알려줘 내용 없어?"
        );
    }

    #[test]
    fn evidence_question_without_context_is_blocked() {
        let mut request = request(AgentChatScopeMode::AllDocs);
        request.question = "Summarize the source evidence".into();
        assert!(!should_answer_as_general_chat(&request));
        assert_eq!(
            classify_answer_mode(&request, false, false),
            AgentChatAnswerMode::Blocked
        );
    }

    #[test]
    fn build_response_preserves_requested_assistant_message_id() {
        let mut request = request(AgentChatScopeMode::Auto);
        request.assistant_message_id = Some("assistant-1".into());
        let response = build_response(
            &request,
            &empty_context_pack(&request),
            AgentChatAnswerMode::General,
            provider_summary(&EngineConfig {
                provider: ProviderKind::Ollama,
                model_id: "llama3.1".into(),
                api_key: "".into(),
                base_url: None,
                prompt_template: "General".into(),
            }),
            None,
            "Hello".into(),
            AnswerStatus::LowConfidence,
            Vec::new(),
            Vec::new(),
            "General response.",
        );
        assert_eq!(response.assistant_message.id, "assistant-1");
        assert_eq!(response.answer_mode, AgentChatAnswerMode::General);
    }

    #[test]
    fn provider_config_error_is_specific_for_missing_openrouter_key() {
        let config = EngineConfig {
            provider: ProviderKind::OpenRouter,
            model_id: "openai/gpt-4.1-mini".into(),
            api_key: "".into(),
            base_url: None,
            prompt_template: "General".into(),
        };
        let error = run_rig_agent(&config, "preamble", "context", "prompt")
            .unwrap_err()
            .to_string();
        assert!(error.contains("OpenRouter requires an API key"));
    }
}
