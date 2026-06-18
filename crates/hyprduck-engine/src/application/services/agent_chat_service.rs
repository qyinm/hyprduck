use std::collections::BTreeSet;
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{anyhow, bail, Context, Result};
use hyprduck_engine_types::{
    AgentChatAskRequest, AgentChatAskResponseData, AgentChatMessage, AgentChatMessageRole,
    AgentChatProviderSummary, AgentChatScopeMode, AnswerResponse, AnswerStatus,
    ContextPackEvidenceV1, ContextPackV1, EvidenceRef, GetContextPackRequest, SuggestedAction,
    SuggestedActionKind, AGENT_CHAT_SCHEMA_VERSION,
};
use rig_core::client::{CompletionClient, Nothing};
use rig_core::completion::Prompt;
use rig_core::providers::{ollama, openrouter};

use crate::application::services::context_pack_service;
use crate::provider::{EngineConfig, ProviderKind};

const MAX_QUESTION_CHARS: usize = 4_000;
const MAX_HISTORY_MESSAGES: usize = 24;
const MAX_HISTORY_CHARS: usize = 16_000;
const DEFAULT_CONTEXT_BUDGET: usize = 8_000;
const DEFAULT_MAX_TOKENS: u64 = 1_200;
const AGENT_PREAMBLE: &str = r#"You are HyprDuck's local document evidence agent.
Answer only from the supplied context pack.
When you use evidence, cite the exact evidenceRef in square brackets, for example [ev_abc123].
If the context is insufficient, say what is missing instead of guessing.
Do not expose local filesystem paths."#;

pub(crate) fn handle_agent_chat_ask(
    request: AgentChatAskRequest,
    config: &EngineConfig,
) -> Result<AgentChatAskResponseData> {
    validate_agent_chat_request(&request)?;

    let selected_node_id = match request.mode {
        AgentChatScopeMode::GraphContext => request.selected_node_id.clone(),
        AgentChatScopeMode::AllDocs | AgentChatScopeMode::SelectedSource => None,
    };
    let context_response = context_pack_service::handle_get_context_pack(GetContextPackRequest {
        scope: request.scope.clone(),
        query: request.question.clone(),
        selected_node_id,
        budget: request.budget.or(Some(DEFAULT_CONTEXT_BUDGET)),
        persist: false,
    })?;
    let context_pack = context_response.context_pack_v1;
    let filtered_context_pack = filter_context_pack_for_scope(&context_pack, &request);
    let persisted_context_pack_path = if request.persist_context_pack {
        Some(context_pack_service::persist_context_pack_v1(
            &request.scope,
            &filtered_context_pack,
        )?)
    } else {
        None
    };
    let provider = provider_summary(config);
    let mut warnings = context_pack
        .warnings
        .iter()
        .map(|warning| warning.message.clone())
        .collect::<Vec<_>>();

    if filtered_context_pack.selected_evidence.is_empty()
        && filtered_context_pack.source_set.is_empty()
    {
        let message = "I could not find citation-ready document context for this question.";
        return Ok(build_response(
            &request,
            &filtered_context_pack,
            provider,
            persisted_context_pack_path,
            message.to_string(),
            AnswerStatus::Blocked,
            Vec::new(),
            warnings,
            "No matching context pack evidence was available.",
        ));
    }

    let prompt = build_user_prompt(&request, &filtered_context_pack);
    let context = build_context_document(&filtered_context_pack);
    let model_text = run_rig_agent(config, &context, &prompt)?;
    let mut citations = validate_model_citations(&model_text, &filtered_context_pack);
    let mut answer_status = if citations.is_empty() {
        AnswerStatus::LowConfidence
    } else {
        AnswerStatus::Grounded
    };

    if citations.is_empty() && !filtered_context_pack.selected_evidence.is_empty() {
        citations = filtered_context_pack
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

    Ok(build_response(
        &request,
        &filtered_context_pack,
        provider,
        persisted_context_pack_path,
        model_text,
        answer_status,
        citations,
        warnings,
        "Answered with the selected context pack evidence.",
    ))
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

fn run_rig_agent(config: &EngineConfig, context: &str, prompt: &str) -> Result<String> {
    match &config.provider {
        ProviderKind::OpenRouter => run_openrouter_agent(config, context, prompt),
        ProviderKind::Ollama => run_ollama_agent(config, context, prompt),
        ProviderKind::Unknown(slug) => bail!(
            "provider_config: provider `{slug}` is not supported for Agent chat. Select OpenRouter or Ollama."
        ),
    }
}

fn run_openrouter_agent(config: &EngineConfig, context: &str, prompt: &str) -> Result<String> {
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
        .preamble(AGENT_PREAMBLE)
        .context(context)
        .temperature(0.2)
        .max_tokens(DEFAULT_MAX_TOKENS)
        .build();
    block_on_prompt(agent.prompt(prompt))
}

fn run_ollama_agent(config: &EngineConfig, context: &str, prompt: &str) -> Result<String> {
    let mut builder = ollama::Client::builder().api_key(Nothing);
    if let Some(base_url) = normalized_ollama_base_url(config.base_url.as_deref()) {
        builder = builder.base_url(base_url);
    }
    let client = builder
        .build()
        .context("provider_config: failed to create Rig Ollama client")?;
    let agent = client
        .agent(config.model_id.as_str())
        .preamble(AGENT_PREAMBLE)
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

fn build_user_prompt(request: &AgentChatAskRequest, context_pack: &ContextPackV1) -> String {
    let history = request
        .history
        .iter()
        .rev()
        .take(8)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .map(|message| format!("{:?}: {}", message.role, message.text))
        .collect::<Vec<_>>()
        .join("\n");
    let scope_line = match request.mode {
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
    provider: AgentChatProviderSummary,
    persisted_context_pack_path: Option<String>,
    text: String,
    status: AnswerStatus,
    citations: Vec<ContextPackEvidenceV1>,
    warnings: Vec<String>,
    explanation: &str,
) -> AgentChatAskResponseData {
    let message = AgentChatMessage {
        id: format!("msg_{}", uuid::Uuid::now_v7().simple()),
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
    fn provider_config_error_is_specific_for_missing_openrouter_key() {
        let config = EngineConfig {
            provider: ProviderKind::OpenRouter,
            model_id: "openai/gpt-4.1-mini".into(),
            api_key: "".into(),
            base_url: None,
            prompt_template: "General".into(),
        };
        let error = run_rig_agent(&config, "context", "prompt")
            .unwrap_err()
            .to_string();
        assert!(error.contains("OpenRouter requires an API key"));
    }
}
