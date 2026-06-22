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
use serde_json::Value;

use crate::application::services::context_pack_service;
use crate::domains::retrieval::brain_search::db_search_terms;
use crate::provider::{EngineConfig, ProviderKind};

const MAX_QUESTION_CHARS: usize = 4_000;
const MAX_HISTORY_MESSAGES: usize = 24;
const MAX_HISTORY_CHARS: usize = 16_000;
const DEFAULT_CONTEXT_BUDGET: usize = 8_000;
const DEFAULT_MAX_TOKENS: u64 = 1_200;
const MAX_CONTEXT_RETRIEVAL_ATTEMPTS: usize = 4;
const MAX_GENERATION_ATTEMPTS: usize = 2;
const EVIDENCE_AGENT_PREAMBLE: &str = r#"You are HyprDuck's local document evidence agent.
Answer only from the supplied context pack.
When you use evidence, cite the exact evidenceRef in square brackets, for example [ev_abc123].
If the context is insufficient, say what is missing instead of guessing.
Do not expose local filesystem paths."#;
const GENERAL_AGENT_PREAMBLE: &str = r#"You are HyprDuck's agent chat assistant.
Answer the user's general question directly.
No citation-ready HyprDuck document context was supplied for this turn, so do not invent citations or evidence references.
Do not expose local filesystem paths."#;
const CONTEXT_QUERY_PLANNER_PREAMBLE: &str = r#"You are HyprDuck's retrieval query planner.
You do not answer the user.
Convert the latest user request into concise search queries for local document evidence.
Use the user's language. Preserve concrete entities, filenames, graph/source hints, and topic nouns.
Drop conversational or command wording. If a topic and request wording are glued together, infer the likely searchable topic phrase.
Return JSON only as {"queries":["..."]}. Do not include markdown."#;
const MAX_PLANNED_CONTEXT_QUERIES: usize = 5;
const MAX_PLANNED_CONTEXT_QUERY_CHARS: usize = 160;

type AgentChatEventSink<'a> = &'a mut dyn FnMut(AgentChatStreamEvent) -> Result<()>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AgentChatWorkflowStep {
    ClassifyQuestion,
    RetrieveContext,
    AssessContext,
    PlanContextQueries,
    ConnectProvider,
    Generate,
    ValidateCitations,
    RepairCitations,
    Block,
    Finalize,
}

struct AgentChatRunState {
    provider: AgentChatProviderSummary,
    general_intent: bool,
    should_retrieve_context: bool,
    context_query: String,
    context_query_candidates: Vec<String>,
    next_context_query_index: usize,
    context_query_planning_attempted: bool,
    retrieval_attempts: usize,
    generation_attempts: usize,
    started: bool,
    context_pack: ContextPackV1,
    persisted_context_pack_path: Option<String>,
    answer_mode: AgentChatAnswerMode,
    model_text: String,
    citations: Vec<ContextPackEvidenceV1>,
    answer_status: AnswerStatus,
    warnings: Vec<String>,
    explanation: String,
}

impl AgentChatRunState {
    fn new(request: &AgentChatAskRequest, provider: AgentChatProviderSummary) -> Self {
        Self {
            provider,
            general_intent: false,
            should_retrieve_context: false,
            context_query: request.question.trim().into(),
            context_query_candidates: Vec::new(),
            next_context_query_index: 0,
            context_query_planning_attempted: false,
            retrieval_attempts: 0,
            generation_attempts: 0,
            started: false,
            context_pack: empty_context_pack(request),
            persisted_context_pack_path: None,
            answer_mode: AgentChatAnswerMode::General,
            model_text: String::new(),
            citations: Vec::new(),
            answer_status: AnswerStatus::LowConfidence,
            warnings: Vec::new(),
            explanation: String::new(),
        }
    }

    fn set_context_query_candidates(&mut self, candidates: Vec<String>) {
        self.context_query_candidates = candidates;
        self.context_query = self
            .context_query_candidates
            .first()
            .cloned()
            .unwrap_or_else(String::new);
        self.next_context_query_index = usize::from(!self.context_query_candidates.is_empty());
    }

    fn advance_context_query(&mut self) -> bool {
        if self.retrieval_attempts >= MAX_CONTEXT_RETRIEVAL_ATTEMPTS {
            return false;
        }
        while self.next_context_query_index < self.context_query_candidates.len() {
            let query = self.context_query_candidates[self.next_context_query_index].clone();
            self.next_context_query_index += 1;
            if query != self.context_query {
                self.context_query = query;
                return true;
            }
        }
        false
    }

    fn extend_context_query_candidates(&mut self, candidates: Vec<String>) -> bool {
        let before = self.context_query_candidates.len();
        for candidate in candidates {
            push_unique_context_query(&mut self.context_query_candidates, candidate);
        }
        self.context_query_candidates.len() > before
    }
}

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
    let streaming = emit.is_some();
    let mut run = AgentChatRunState::new(&request, provider);
    let mut step = AgentChatWorkflowStep::ClassifyQuestion;

    loop {
        step = match step {
            AgentChatWorkflowStep::ClassifyQuestion => {
                emit_status(
                    &mut emit,
                    AgentChatStreamStatus::ClassifyingQuestion,
                    "Classifying question...",
                )?;
                run.general_intent = should_answer_as_general_chat(&request);
                run.should_retrieve_context = should_retrieve_context(&request, run.general_intent);
                run.set_context_query_candidates(build_context_query_candidates(&request));
                if run.should_retrieve_context {
                    AgentChatWorkflowStep::RetrieveContext
                } else {
                    AgentChatWorkflowStep::AssessContext
                }
            }
            AgentChatWorkflowStep::RetrieveContext => {
                run.retrieval_attempts += 1;
                emit_status(
                    &mut emit,
                    AgentChatStreamStatus::RetrievingContext,
                    if run.retrieval_attempts == 1 {
                        "Retrieving context..."
                    } else {
                        "Retrieving more context..."
                    },
                )?;
                run.context_pack = retrieve_context_pack_for_agent(&request, &run.context_query)?;
                run.warnings = run
                    .context_pack
                    .warnings
                    .iter()
                    .map(|warning| warning.message.clone())
                    .collect::<Vec<_>>();
                AgentChatWorkflowStep::AssessContext
            }
            AgentChatWorkflowStep::AssessContext => {
                let has_context = has_citation_ready_context(&run.context_pack);
                run.answer_mode = classify_answer_mode(&request, run.general_intent, has_context);
                if matches!(run.answer_mode, AgentChatAnswerMode::Blocked)
                    && run.advance_context_query()
                {
                    AgentChatWorkflowStep::RetrieveContext
                } else if matches!(run.answer_mode, AgentChatAnswerMode::Blocked)
                    && !run.context_query_planning_attempted
                {
                    AgentChatWorkflowStep::PlanContextQueries
                } else if matches!(run.answer_mode, AgentChatAnswerMode::Blocked) {
                    AgentChatWorkflowStep::Block
                } else {
                    emit_started_once(&request, &mut emit, &mut run)?;
                    AgentChatWorkflowStep::ConnectProvider
                }
            }
            AgentChatWorkflowStep::PlanContextQueries => {
                run.context_query_planning_attempted = true;
                emit_status(
                    &mut emit,
                    AgentChatStreamStatus::RetrievingContext,
                    "Planning retrieval queries...",
                )?;
                match plan_context_query_candidates(config, &request, &run.context_query_candidates)
                {
                    Ok(candidates) => {
                        let added = run.extend_context_query_candidates(candidates);
                        if added && run.advance_context_query() {
                            AgentChatWorkflowStep::RetrieveContext
                        } else {
                            AgentChatWorkflowStep::Block
                        }
                    }
                    Err(error) => {
                        run.warnings.push(format!(
                            "Context query planning failed before retrieval retry: {error:#}"
                        ));
                        AgentChatWorkflowStep::Block
                    }
                }
            }
            AgentChatWorkflowStep::ConnectProvider => {
                emit_status(
                    &mut emit,
                    AgentChatStreamStatus::ConnectingProvider,
                    "Connecting provider...",
                )?;
                AgentChatWorkflowStep::Generate
            }
            AgentChatWorkflowStep::Generate => {
                run.generation_attempts += 1;
                emit_status(
                    &mut emit,
                    AgentChatStreamStatus::Generating,
                    if run.generation_attempts == 1 {
                        "Generating..."
                    } else {
                        "Regenerating with citation checks..."
                    },
                )?;
                let (preamble, context, prompt) = build_agent_turn_prompt(&request, &run);
                run.model_text = if let Some(sink) = emit.as_deref_mut() {
                    run_rig_agent_stream(config, preamble, &context, &prompt, sink)?
                } else {
                    run_rig_agent(config, preamble, &context, &prompt)?
                };
                AgentChatWorkflowStep::ValidateCitations
            }
            AgentChatWorkflowStep::ValidateCitations => {
                emit_status(
                    &mut emit,
                    AgentChatStreamStatus::ValidatingCitations,
                    "Validating citations...",
                )?;
                run.citations = if matches!(run.answer_mode, AgentChatAnswerMode::Evidence) {
                    validate_model_citations(&run.model_text, &run.context_pack)
                } else {
                    Vec::new()
                };
                run.answer_status = answer_status_for_run(&run);
                if matches!(run.answer_mode, AgentChatAnswerMode::Evidence)
                    && run.citations.is_empty()
                    && !streaming
                    && run.generation_attempts < MAX_GENERATION_ATTEMPTS
                {
                    AgentChatWorkflowStep::RepairCitations
                } else {
                    attach_fallback_citations_if_needed(&mut run);
                    if !run.citations.is_empty() {
                        emit_agent_event(
                            &mut emit,
                            AgentChatStreamEvent::CitationUpdate {
                                citations: run.citations.clone(),
                            },
                        )?;
                    }
                    AgentChatWorkflowStep::Finalize
                }
            }
            AgentChatWorkflowStep::RepairCitations => {
                run.warnings.push(
                    "The first draft omitted valid evidenceRefs; HyprDuck asked the agent to rewrite with citations."
                        .into(),
                );
                AgentChatWorkflowStep::Generate
            }
            AgentChatWorkflowStep::Block => {
                emit_started_once(&request, &mut emit, &mut run)?;
                run.model_text =
                    "I could not find citation-ready document context for this question."
                        .to_string();
                run.answer_status = AnswerStatus::Blocked;
                run.explanation = "The question asks for document or graph-grounded facts, but no citation-ready context matched.".into();
                AgentChatWorkflowStep::Finalize
            }
            AgentChatWorkflowStep::Finalize => {
                if request.persist_context_pack && run.persisted_context_pack_path.is_none() {
                    run.persisted_context_pack_path =
                        Some(context_pack_service::persist_context_pack_v1(
                            &request.scope,
                            &run.context_pack,
                        )?);
                }
                let response = build_response(
                    &request,
                    &run.context_pack,
                    run.answer_mode,
                    run.provider,
                    run.persisted_context_pack_path,
                    run.model_text,
                    run.answer_status,
                    run.citations,
                    run.warnings,
                    if run.explanation.is_empty() {
                        if matches!(run.answer_mode, AgentChatAnswerMode::General) {
                            "Answered as general chat without document citations."
                        } else {
                            "Answered with the selected context pack evidence."
                        }
                    } else {
                        run.explanation.as_str()
                    },
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
        };
    }
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

fn retrieve_context_pack_for_agent(
    request: &AgentChatAskRequest,
    query: &str,
) -> Result<ContextPackV1> {
    let selected_node_id = match request.mode {
        AgentChatScopeMode::GraphContext => request.selected_node_id.clone(),
        AgentChatScopeMode::Auto
        | AgentChatScopeMode::AllDocs
        | AgentChatScopeMode::SelectedSource => None,
    };
    let context_response = context_pack_service::handle_get_context_pack(GetContextPackRequest {
        scope: request.scope.clone(),
        query: query.into(),
        selected_node_id,
        budget: request.budget.or(Some(DEFAULT_CONTEXT_BUDGET)),
        persist: false,
    })?;
    Ok(filter_context_pack_for_scope(
        &context_response.context_pack_v1,
        request,
    ))
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

fn emit_started_once(
    request: &AgentChatAskRequest,
    emit: &mut Option<AgentChatEventSink<'_>>,
    run: &mut AgentChatRunState,
) -> Result<()> {
    if run.started {
        return Ok(());
    }
    run.started = true;
    emit_agent_event(
        emit,
        AgentChatStreamEvent::Started {
            conversation_id: request.conversation_id.clone(),
            assistant_message_id: assistant_message_id(request),
            provider: run.provider.clone(),
            answer_mode: Some(run.answer_mode),
        },
    )
}

fn has_citation_ready_context(context_pack: &ContextPackV1) -> bool {
    !context_pack.selected_evidence.is_empty()
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
    if should_reuse_previous_topic_for_context(question) {
        if let Some(query) = build_history_augmented_context_query(request) {
            return query;
        }
    }
    question.into()
}

fn build_context_query_candidates(request: &AgentChatAskRequest) -> Vec<String> {
    let mut candidates = Vec::new();
    push_unique_context_query(&mut candidates, build_context_query(request));
    if let Some(query) = build_history_augmented_context_query(request) {
        push_unique_context_query(&mut candidates, query);
    }
    if let Some(query) = build_cleaned_context_query(request.question.trim()) {
        push_unique_context_query(&mut candidates, query);
    }
    if let Some(query) = build_history_augmented_clean_context_query(request) {
        push_unique_context_query(&mut candidates, query);
    }
    if candidates.is_empty() {
        push_unique_context_query(&mut candidates, request.question.trim().into());
    }
    candidates
}

fn push_unique_context_query(candidates: &mut Vec<String>, query: String) {
    let query = query.trim();
    if query.is_empty() || candidates.iter().any(|candidate| candidate == query) {
        return;
    }
    candidates.push(query.into());
}

fn build_history_augmented_context_query(request: &AgentChatAskRequest) -> Option<String> {
    let question = request.question.trim();
    request
        .history
        .iter()
        .rev()
        .filter(|message| matches!(message.role, AgentChatMessageRole::User))
        .map(|message| message.text.trim())
        .find(|text| !text.is_empty() && !db_search_terms(text).is_empty())
        .map(|previous_question| format!("{previous_question} {question}"))
}

fn build_cleaned_context_query(text: &str) -> Option<String> {
    let query = db_search_terms(text).join(" ");
    (!query.is_empty()).then_some(query)
}

fn build_history_augmented_clean_context_query(request: &AgentChatAskRequest) -> Option<String> {
    let question = build_cleaned_context_query(request.question.trim());
    request
        .history
        .iter()
        .rev()
        .filter(|message| matches!(message.role, AgentChatMessageRole::User))
        .filter_map(|message| build_cleaned_context_query(message.text.trim()))
        .next()
        .map(|previous_question| match &question {
            Some(question) if !question.is_empty() => format!("{previous_question} {question}"),
            _ => previous_question,
        })
}

fn plan_context_query_candidates(
    config: &EngineConfig,
    request: &AgentChatAskRequest,
    attempted_queries: &[String],
) -> Result<Vec<String>> {
    let prompt = build_context_query_planner_prompt(request, attempted_queries);
    let output = run_rig_agent(config, CONTEXT_QUERY_PLANNER_PREAMBLE, "", &prompt)?;
    Ok(parse_context_query_plan(&output))
}

fn build_context_query_planner_prompt(
    request: &AgentChatAskRequest,
    attempted_queries: &[String],
) -> String {
    let recent_user_messages = format_recent_user_messages(&request.history);
    let attempted = if attempted_queries.is_empty() {
        "(none)".to_string()
    } else {
        attempted_queries
            .iter()
            .map(|query| format!("- {query}"))
            .collect::<Vec<_>>()
            .join("\n")
    };
    format!(
        "Latest user question:\n{}\n\nRecent user messages:\n{}\n\nAlready attempted retrieval queries:\n{}\n\nReturn 1 to {} concise search queries that should be tried against indexed document text, filenames, graph labels, and source metadata. Prefer evidence-bearing topic/entity phrases over full chat commands. JSON only.",
        request.question.trim(),
        if recent_user_messages.is_empty() {
            "(none)"
        } else {
            recent_user_messages.as_str()
        },
        attempted,
        MAX_PLANNED_CONTEXT_QUERIES,
    )
}

fn format_recent_user_messages(history: &[AgentChatMessage]) -> String {
    history
        .iter()
        .rev()
        .filter(|message| matches!(message.role, AgentChatMessageRole::User))
        .take(6)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .map(|message| message.text.trim())
        .filter(|text| !text.is_empty())
        .collect::<Vec<_>>()
        .join("\n")
}

fn parse_context_query_plan(output: &str) -> Vec<String> {
    let Some(value) = parse_json_value_from_text(output) else {
        return Vec::new();
    };
    let queries = match value {
        Value::Object(map) => map
            .get("queries")
            .and_then(Value::as_array)
            .map(|values| query_strings_from_json_array(values))
            .unwrap_or_default(),
        Value::Array(values) => query_strings_from_json_array(&values),
        _ => Vec::new(),
    };
    sanitize_planned_context_queries(queries)
}

fn query_strings_from_json_array(values: &[Value]) -> Vec<String> {
    values
        .iter()
        .filter_map(Value::as_str)
        .map(str::to_string)
        .collect()
}

fn sanitize_planned_context_queries(queries: Vec<String>) -> Vec<String> {
    let mut sanitized = Vec::new();
    for query in queries {
        let query = query.split_whitespace().collect::<Vec<_>>().join(" ");
        let query = query
            .trim()
            .chars()
            .take(MAX_PLANNED_CONTEXT_QUERY_CHARS)
            .collect::<String>();
        if query.is_empty() || sanitized.iter().any(|candidate| candidate == &query) {
            continue;
        }
        sanitized.push(query);
        if sanitized.len() >= MAX_PLANNED_CONTEXT_QUERIES {
            break;
        }
    }
    sanitized
}

fn parse_json_value_from_text(output: &str) -> Option<Value> {
    let trimmed = output.trim();
    if let Ok(value) = serde_json::from_str::<Value>(trimmed) {
        return Some(value);
    }
    for (start, _) in output
        .char_indices()
        .filter(|(_, ch)| matches!(ch, '{' | '['))
    {
        let Some(end) = json_value_end(&output[start..]) else {
            continue;
        };
        if let Ok(value) = serde_json::from_str::<Value>(&output[start..start + end]) {
            return Some(value);
        }
    }
    None
}

fn json_value_end(text: &str) -> Option<usize> {
    let mut stack = Vec::new();
    let mut in_string = false;
    let mut escaped = false;
    for (index, ch) in text.char_indices() {
        if in_string {
            if escaped {
                escaped = false;
            } else if ch == '\\' {
                escaped = true;
            } else if ch == '"' {
                in_string = false;
            }
            continue;
        }
        match ch {
            '"' => in_string = true,
            '{' => stack.push('}'),
            '[' => stack.push(']'),
            '}' | ']' => {
                if stack.pop() != Some(ch) {
                    return None;
                }
                if stack.is_empty() {
                    return Some(index + ch.len_utf8());
                }
            }
            _ => {}
        }
    }
    None
}

fn should_reuse_previous_topic_for_context(question: &str) -> bool {
    let terms = db_search_terms(question);
    terms.is_empty()
        || (terms.len() <= 1 && looks_like_evidence_question(question))
        || terms.iter().all(|term| is_generic_evidence_term(term))
}

fn is_generic_evidence_term(term: &str) -> bool {
    matches!(
        term,
        "document"
            | "documents"
            | "doc"
            | "docs"
            | "source"
            | "sources"
            | "citation"
            | "citations"
            | "evidence"
            | "graph"
            | "node"
            | "context"
            | "pdf"
            | "docx"
            | "file"
            | "files"
            | "paper"
            | "papers"
            | "article"
            | "articles"
            | "research"
            | "page"
            | "pages"
            | "summarize"
            | "summary"
            | "문서"
            | "자료"
            | "파일"
            | "논문"
            | "연구"
            | "출처"
            | "근거"
            | "인용"
            | "그래프"
            | "노드"
            | "페이지"
            | "요약"
            | "정리"
    )
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
        "paper",
        "papers",
        "article",
        "articles",
        "research",
        "page",
        "pages",
        "summarize",
        "summary",
        "문서",
        "자료",
        "파일",
        "논문",
        "연구",
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
    block_on_prompt(agent.prompt(prompt).max_turns(2).with_tool_concurrency(2))
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
    block_on_stream(agent.stream_prompt(prompt.to_string()).multi_turn(2), emit)
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
    block_on_stream(agent.stream_prompt(prompt.to_string()).multi_turn(2), emit).map_err(|error| {
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

fn build_agent_turn_prompt(
    request: &AgentChatAskRequest,
    run: &AgentChatRunState,
) -> (&'static str, String, String) {
    match run.answer_mode {
        AgentChatAnswerMode::Evidence => {
            let prompt = if run.generation_attempts > 1 {
                build_citation_repair_user_prompt(
                    request,
                    &run.context_pack,
                    run.model_text.as_str(),
                )
            } else {
                build_evidence_user_prompt(request, &run.context_pack)
            };
            (
                EVIDENCE_AGENT_PREAMBLE,
                build_context_document(&run.context_pack),
                prompt,
            )
        }
        AgentChatAnswerMode::General => (
            GENERAL_AGENT_PREAMBLE,
            "No citation-ready HyprDuck document context was retrieved for this turn.".to_string(),
            build_general_user_prompt(request),
        ),
        AgentChatAnswerMode::Blocked => (
            GENERAL_AGENT_PREAMBLE,
            String::new(),
            build_general_user_prompt(request),
        ),
    }
}

fn build_evidence_user_prompt(
    request: &AgentChatAskRequest,
    context_pack: &ContextPackV1,
) -> String {
    let recent_user_requests = format_recent_user_messages(&request.history);
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
        "{scope_line}\nContext pack: {}\nRecent user requests for topic continuity only:\n{}\n\nCurrent context pack overrides prior chat text. Do not treat previous assistant messages as evidence.\n\nQuestion:\n{}",
        context_pack.pack_id,
        if recent_user_requests.is_empty() {
            "(no prior messages)"
        } else {
            recent_user_requests.as_str()
        },
        request.question.trim()
    )
}

fn build_citation_repair_user_prompt(
    request: &AgentChatAskRequest,
    context_pack: &ContextPackV1,
    previous_draft: &str,
) -> String {
    let valid_refs = context_pack
        .selected_evidence
        .iter()
        .map(|evidence| evidence.evidence_ref.as_str())
        .collect::<Vec<_>>()
        .join(", ");
    format!(
        "{}\n\nThe previous draft did not cite valid evidenceRefs from the context pack. Rewrite the answer using only the supplied evidence. Include at least one valid evidenceRef in square brackets for every factual paragraph. Valid evidenceRefs: {}.\n\nPrevious draft:\n{}",
        build_evidence_user_prompt(request, context_pack),
        valid_refs,
        previous_draft.trim()
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

fn answer_status_for_run(run: &AgentChatRunState) -> AnswerStatus {
    if matches!(run.answer_mode, AgentChatAnswerMode::General) {
        AnswerStatus::LowConfidence
    } else if run.citations.is_empty() {
        AnswerStatus::LowConfidence
    } else {
        AnswerStatus::Grounded
    }
}

fn attach_fallback_citations_if_needed(run: &mut AgentChatRunState) {
    if !run.citations.is_empty()
        || !matches!(run.answer_mode, AgentChatAnswerMode::Evidence)
        || run.context_pack.selected_evidence.is_empty()
    {
        return;
    }
    run.citations = run
        .context_pack
        .selected_evidence
        .iter()
        .take(3)
        .cloned()
        .collect();
    run.warnings.push(
        "The model did not cite evidenceRefs; HyprDuck attached top context evidence as fallback."
            .into(),
    );
    run.answer_status = AnswerStatus::LowConfidence;
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
    fn source_set_without_selected_evidence_is_not_citation_ready() {
        let mut pack = context_pack();
        pack.selected_evidence.clear();

        assert!(!has_citation_ready_context(&pack));
        assert_eq!(
            classify_answer_mode(&request(AgentChatScopeMode::Auto), false, false),
            AgentChatAnswerMode::General
        );
        assert_eq!(
            classify_answer_mode(&request(AgentChatScopeMode::AllDocs), false, false),
            AgentChatAnswerMode::Blocked
        );
    }

    #[test]
    fn citation_repair_prompt_names_valid_evidence_refs() {
        let request = request(AgentChatScopeMode::Auto);
        let pack = context_pack();
        let prompt = build_citation_repair_user_prompt(&request, &pack, "draft without refs");

        assert!(prompt.contains("Valid evidenceRefs: ev_a"));
        assert!(prompt.contains("Previous draft:"));
        assert!(prompt.contains("draft without refs"));
    }

    #[test]
    fn evidence_prompt_excludes_stale_assistant_history() {
        let mut request = request(AgentChatScopeMode::Auto);
        request.history = vec![
            AgentChatMessage {
                id: "msg_user_1".into(),
                role: AgentChatMessageRole::User,
                text: "summarize the parser fixture".into(),
                created_at: 1,
            },
            AgentChatMessage {
                id: "msg_assistant_1".into(),
                role: AgentChatMessageRole::Assistant,
                text: "The context pack only contains unrelated queue content.".into(),
                created_at: 2,
            },
        ];
        let prompt = build_evidence_user_prompt(&request, &context_pack());

        assert!(prompt.contains("summarize the parser fixture"));
        assert!(prompt.contains("Do not treat previous assistant messages as evidence."));
        assert!(!prompt.contains("unrelated queue content"));
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
    fn generic_evidence_follow_up_context_query_reuses_previous_user_topic() {
        let mut request = request(AgentChatScopeMode::Auto);
        request.history = vec![AgentChatMessage {
            id: "msg_user_1".into(),
            role: AgentChatMessageRole::User,
            text: "graph contains parser fixture evidence".into(),
            created_at: 1,
        }];
        request.question = "source summary".into();

        let candidates = build_context_query_candidates(&request);

        assert!(
            candidates
                .iter()
                .any(|candidate| candidate
                    == "graph contains parser fixture evidence source summary"),
            "{candidates:#?}"
        );
    }

    #[test]
    fn context_query_candidates_keep_raw_question_first() {
        let mut request = request(AgentChatScopeMode::Auto);
        request.history.clear();
        request.question = "source summary".into();

        let candidates = build_context_query_candidates(&request);

        assert_eq!(
            candidates.first().map(String::as_str),
            Some("source summary")
        );
    }

    #[test]
    fn blocked_retrieval_can_advance_to_cleaned_query_candidate() {
        let mut request = request(AgentChatScopeMode::Auto);
        request.history.clear();
        request.question = "dynamic hashing 내용".into();
        let mut run = AgentChatRunState::new(
            &request,
            AgentChatProviderSummary {
                id: "test".into(),
                label: "Test".into(),
                model_id: "test-model".into(),
                hosted: false,
            },
        );
        run.set_context_query_candidates(build_context_query_candidates(&request));
        run.retrieval_attempts = 1;

        assert_eq!(run.context_query, "dynamic hashing 내용");
        assert!(run.advance_context_query());
        assert_eq!(run.context_query, "dynamic hashing");
    }

    #[test]
    fn planned_context_queries_are_appended_for_retrieval_retry() {
        let mut request = request(AgentChatScopeMode::Auto);
        request.history.clear();
        request.question = "source summary".into();
        let mut run = AgentChatRunState::new(
            &request,
            AgentChatProviderSummary {
                id: "test".into(),
                label: "Test".into(),
                model_id: "test-model".into(),
                hosted: false,
            },
        );
        run.set_context_query_candidates(build_context_query_candidates(&request));
        run.retrieval_attempts = 1;

        assert_eq!(run.context_query, "source summary");
        assert!(!run.advance_context_query());
        assert!(run.extend_context_query_candidates(vec![
            "parser fixture".into(),
            "source summary".into()
        ]));
        assert!(run.advance_context_query());
        assert_eq!(run.context_query, "parser fixture");
    }

    #[test]
    fn context_query_plan_parser_accepts_json_object() {
        let queries =
            parse_context_query_plan(r#"{"queries":["parser fixture","indexed source"]}"#);

        assert_eq!(queries, vec!["parser fixture", "indexed source"]);
    }

    #[test]
    fn context_query_plan_parser_accepts_embedded_json_array() {
        let queries = parse_context_query_plan(
            "Plan:\n```json\n[\"fixture parser\", \"metadata source\"]\n```",
        );

        assert_eq!(queries, vec!["fixture parser", "metadata source"]);
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
    fn evidence_question_without_context_is_blocked_instead_of_general_chat() {
        let mut request = request(AgentChatScopeMode::Auto);
        request.question = "source summary".into();

        assert!(!should_answer_as_general_chat(&request));
        assert!(looks_like_evidence_question(&request.question));
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
