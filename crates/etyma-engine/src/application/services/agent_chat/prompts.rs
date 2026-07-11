use etyma_engine_types::{
    AgentChatAnswerMode, AgentChatAskRequest, AgentChatMessage, AgentChatMessageRole,
    AgentChatScopeMode, ContextPackV1,
};

use super::state::AgentChatRunState;

pub(super) const MAX_PLANNED_CONTEXT_QUERIES: usize = 5;
pub(super) const MAX_PLANNED_CONTEXT_QUERY_CHARS: usize = 160;

pub(super) const EVIDENCE_AGENT_PREAMBLE: &str = r#"You are Etyma's local document evidence agent.
Answer only from the supplied context pack.
When you use evidence, cite the exact evidenceRef in square brackets, for example [ev_abc123].
If the context is insufficient, say what is missing instead of guessing.
Do not expose local filesystem paths."#;
pub(super) const GENERAL_AGENT_PREAMBLE: &str = r#"You are Etyma's agent chat assistant.
Answer the user's general question directly.
No citation-ready Etyma document context was supplied for this turn, so do not invent citations or evidence references.
Do not expose local filesystem paths."#;
pub(super) const CONTEXT_QUERY_PLANNER_PREAMBLE: &str = r#"You are Etyma's retrieval query planner.
You do not answer the user.
Convert the latest user request into concise search queries for local document evidence.
Use the user's language. Preserve concrete entities, filenames, graph/source hints, and topic nouns.
Drop conversational or command wording. If a topic and request wording are glued together, infer the likely searchable topic phrase.
Return JSON only as {"queries":["..."]}. Do not include markdown."#;

pub(super) fn build_context_query_planner_prompt(
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

pub(super) fn format_recent_user_messages(history: &[AgentChatMessage]) -> String {
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

pub(super) fn build_agent_turn_prompt(
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
            "No citation-ready Etyma document context was retrieved for this turn.".to_string(),
            build_general_user_prompt(request),
        ),
        AgentChatAnswerMode::Blocked => (
            GENERAL_AGENT_PREAMBLE,
            String::new(),
            build_general_user_prompt(request),
        ),
    }
}

pub(super) fn build_evidence_user_prompt(
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

pub(super) fn build_citation_repair_user_prompt(
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

pub(super) fn build_general_user_prompt(request: &AgentChatAskRequest) -> String {
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

pub(super) fn format_recent_history(history: &[AgentChatMessage]) -> String {
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

pub(super) fn build_context_document(context_pack: &ContextPackV1) -> String {
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
