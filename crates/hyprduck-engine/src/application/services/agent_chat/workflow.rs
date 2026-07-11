use std::collections::BTreeSet;

use anyhow::{bail, Result};
use hyprduck_engine_types::{
    AgentChatAnswerMode, AgentChatAskRequest, AgentChatAskResponseData, AgentChatMessage,
    AgentChatMessageRole, AgentChatProviderSummary, AgentChatScopeMode, AgentChatStreamEvent,
    AgentChatStreamStatus, AnswerResponse, AnswerStatus, ContextPackEvidenceV1, ContextPackV1,
    EvidenceRef, GetContextPackRequest, SuggestedAction, SuggestedActionKind,
    AGENT_CHAT_SCHEMA_VERSION,
};

use super::citations::{
    answer_status_for_run, attach_fallback_citations_if_needed, validate_model_citations,
};
use super::intent::{
    looks_like_evidence_question, should_answer_as_general_chat, should_retrieve_context,
};
use super::prompts::build_agent_turn_prompt;
use super::providers::{run_rig_agent, run_rig_agent_stream};
use super::query_plan::{
    build_context_query_candidates, plan_context_query_candidates,
};
use super::state::{
    unix_timestamp, AgentChatEventSink, AgentChatRunState, DEFAULT_CONTEXT_BUDGET,
};
use crate::application::services::context_pack_service;
use crate::provider::{EngineConfig, ProviderKind};

pub(super) const MAX_QUESTION_CHARS: usize = 4_000;
pub(super) const MAX_HISTORY_MESSAGES: usize = 24;
pub(super) const MAX_HISTORY_CHARS: usize = 16_000;
pub(super) const MAX_GENERATION_ATTEMPTS: usize = 2;

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

pub(super) fn validate_agent_chat_request(request: &AgentChatAskRequest) -> Result<()> {
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

pub(super) fn filter_context_pack_for_scope(
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

pub(super) fn retrieve_context_pack_for_agent(
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

pub(super) fn provider_summary(config: &EngineConfig) -> AgentChatProviderSummary {
    AgentChatProviderSummary {
        id: config.provider.id_slug().into(),
        label: config.provider.label().into(),
        model_id: config.model_id.clone(),
        hosted: matches!(config.provider, ProviderKind::OpenRouter),
    }
}

pub(super) fn emit_status(
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

pub(super) fn emit_agent_event(
    emit: &mut Option<AgentChatEventSink<'_>>,
    event: AgentChatStreamEvent,
) -> Result<()> {
    if let Some(sink) = emit.as_deref_mut() {
        sink(event)?;
    }
    Ok(())
}

pub(super) fn emit_started_once(
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

pub(super) fn has_citation_ready_context(context_pack: &ContextPackV1) -> bool {
    !context_pack.selected_evidence.is_empty()
}

pub(super) fn explicit_evidence_scope(request: &AgentChatAskRequest) -> bool {
    matches!(
        request.mode,
        AgentChatScopeMode::AllDocs
            | AgentChatScopeMode::SelectedSource
            | AgentChatScopeMode::GraphContext
    )
}

pub(super) fn classify_answer_mode(
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

pub(super) fn build_response(
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

pub(super) fn assistant_message_id(request: &AgentChatAskRequest) -> String {
    request
        .assistant_message_id
        .clone()
        .filter(|id| !id.trim().is_empty())
        .unwrap_or_else(|| format!("msg_{}", uuid::Uuid::now_v7().simple()))
}

pub(super) fn evidence_to_answer_citation(evidence: &ContextPackEvidenceV1) -> EvidenceRef {
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

