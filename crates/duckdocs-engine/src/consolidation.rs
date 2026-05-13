use duckdocs_engine_types::{BrainRepoSnapshot, SourceArtifactManifest};
use serde_json::{json, Value};

use crate::import_context::{import_evidence_context_allowed_refs, ImportEvidenceContext};
use crate::provider_graph_prompt::provider_retrieval_context_for_prompt;

pub fn should_run_post_import_consolidation(
    applied_count: usize,
    context: &ImportEvidenceContext,
) -> bool {
    applied_count > 0 && !context.retrieved_source_evidence.is_empty()
}

pub fn build_post_import_consolidation_prompt(
    manifest: &SourceArtifactManifest,
    snapshot: &BrainRepoSnapshot,
    context: &ImportEvidenceContext,
    parent_run_id: Option<&str>,
) -> String {
    let nodes = snapshot
        .nodes
        .iter()
        .take(100)
        .map(|node| {
            format!(
                "- nodeId: {}, kind: {:?}, label: {}, sources: {}",
                node.node_id,
                node.kind,
                node.label,
                join_or_none(&node.source_ids)
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    let edges = snapshot
        .relations
        .iter()
        .take(100)
        .map(|edge| {
            format!(
                "- edgeId: {}, kind: {:?}, source: {}, target: {}, label: {}",
                edge.relation_id, edge.kind, edge.source_node_id, edge.target_node_id, edge.label
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    let context_evidence_refs = import_evidence_context_allowed_refs(context);
    let retrieval_context = provider_retrieval_context_for_prompt(context);

    format!(
        r#"You are HyprDuck's post-import graph consolidation agent.

Goal:
- A provider graph import just ran for sourceId {source_id}.
- Use retrieved old source evidence to repair underlinked graph state.
- Prefer missing cross-document edges, missing claims, aliases, and evidence refs.
- Do not delete nodes or sources.
- Return JSON only with shape {{"proposals":[AgentGraphProposalPayload...]}}.
- Cite only these evidence refs: {context_evidence_refs}

Parent run: {parent_run_id}
Source: {source_path}

Retrieved context:
{retrieval_context}

Current materialized nodes:
{nodes}

Current materialized edges:
{edges}
"#,
        source_id = manifest.source_id,
        source_path = manifest.source_path,
        context_evidence_refs = join_or_none(&context_evidence_refs),
        parent_run_id = parent_run_id.unwrap_or("(none)"),
        retrieval_context = retrieval_context,
        nodes = if nodes.is_empty() { "(none)" } else { &nodes },
        edges = if edges.is_empty() { "(none)" } else { &edges },
    )
}

pub fn post_import_consolidation_report_value(
    status: &str,
    run_id: &str,
    parent_run_id: Option<&str>,
    source_id: &str,
    proposal_ids: &[String],
    applied_proposal_ids: &[String],
    failed_validation_messages: &[String],
    failed_proposals: Value,
    updated_at: u64,
) -> Value {
    json!({
        "status": status,
        "runId": run_id,
        "parentRunId": parent_run_id,
        "sourceId": source_id,
        "proposalIds": proposal_ids,
        "appliedProposalIds": applied_proposal_ids,
        "failedValidationMessages": failed_validation_messages,
        "failedProposals": failed_proposals,
        "updatedAt": updated_at,
    })
}

pub fn failed_post_import_consolidation_report_value(
    run_id: &str,
    parent_run_id: Option<&str>,
    source_id: &str,
    error_message: String,
    updated_at: u64,
) -> Value {
    json!({
        "status": "failed",
        "runId": run_id,
        "parentRunId": parent_run_id,
        "sourceId": source_id,
        "errorMessage": error_message,
        "updatedAt": updated_at,
    })
}

fn join_or_none(values: &[String]) -> String {
    if values.is_empty() {
        "none".into()
    } else {
        values.join(", ")
    }
}
