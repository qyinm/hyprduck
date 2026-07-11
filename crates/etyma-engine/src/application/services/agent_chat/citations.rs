use std::collections::BTreeSet;

use etyma_engine_types::{AgentChatAnswerMode, AnswerStatus, ContextPackEvidenceV1, ContextPackV1};

use super::state::AgentChatRunState;

pub(super) fn validate_model_citations(
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

pub(super) fn answer_status_for_run(run: &AgentChatRunState) -> AnswerStatus {
    if matches!(run.answer_mode, AgentChatAnswerMode::General) {
        AnswerStatus::LowConfidence
    } else if run.citations.is_empty() {
        AnswerStatus::LowConfidence
    } else {
        AnswerStatus::Grounded
    }
}

pub(super) fn attach_fallback_citations_if_needed(run: &mut AgentChatRunState) {
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
        "The model did not cite evidenceRefs; Etyma attached top context evidence as fallback."
            .into(),
    );
    run.answer_status = AnswerStatus::LowConfidence;
}
