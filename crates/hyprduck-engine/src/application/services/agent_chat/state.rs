use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::Result;
use hyprduck_engine_types::{
    AgentChatAnswerMode, AgentChatAskRequest, AgentChatProviderSummary, AgentChatStreamEvent,
    AnswerStatus, ContextPackEvidenceTypeTraceV1, ContextPackEvidenceV1, ContextPackFindingV0,
    ContextPackRetrievalTraceV1, ContextPackSourceV0, ContextPackV1, ContextPackWarningV0,
};

use super::query_plan::push_unique_context_query;

pub(super) const DEFAULT_CONTEXT_BUDGET: usize = 8_000;
pub(super) const MAX_CONTEXT_RETRIEVAL_ATTEMPTS: usize = 4;

pub(super) type AgentChatEventSink<'a> = &'a mut dyn FnMut(AgentChatStreamEvent) -> Result<()>;

pub(super) struct AgentChatRunState {
    pub(super) provider: AgentChatProviderSummary,
    pub(super) general_intent: bool,
    pub(super) should_retrieve_context: bool,
    pub(super) context_query: String,
    pub(super) context_query_candidates: Vec<String>,
    pub(super) next_context_query_index: usize,
    pub(super) context_query_planning_attempted: bool,
    pub(super) retrieval_attempts: usize,
    pub(super) generation_attempts: usize,
    pub(super) started: bool,
    pub(super) context_pack: ContextPackV1,
    pub(super) persisted_context_pack_path: Option<String>,
    pub(super) answer_mode: AgentChatAnswerMode,
    pub(super) model_text: String,
    pub(super) citations: Vec<ContextPackEvidenceV1>,
    pub(super) answer_status: AnswerStatus,
    pub(super) warnings: Vec<String>,
    pub(super) explanation: String,
}

impl AgentChatRunState {
    pub(super) fn new(request: &AgentChatAskRequest, provider: AgentChatProviderSummary) -> Self {
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

    pub(super) fn set_context_query_candidates(&mut self, candidates: Vec<String>) {
        self.context_query_candidates = candidates;
        self.context_query = self
            .context_query_candidates
            .first()
            .cloned()
            .unwrap_or_else(String::new);
        self.next_context_query_index = usize::from(!self.context_query_candidates.is_empty());
    }

    pub(super) fn advance_context_query(&mut self) -> bool {
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

    pub(super) fn extend_context_query_candidates(&mut self, candidates: Vec<String>) -> bool {
        let before = self.context_query_candidates.len();
        for candidate in candidates {
            push_unique_context_query(&mut self.context_query_candidates, candidate);
        }
        self.context_query_candidates.len() > before
    }
}

pub(super) fn empty_context_pack(request: &AgentChatAskRequest) -> ContextPackV1 {
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

pub(super) fn unix_timestamp() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0)
}
