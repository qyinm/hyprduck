use hyprduck_engine_types::{BrainRepoSnapshot, SourceArtifactManifest};

use crate::import_context::{import_evidence_context_allowed_refs, ImportEvidenceContext};
use crate::retrieval::RetrievedEvidenceChunk;

pub fn build_provider_graph_proposal_prompt(
    manifest: &SourceArtifactManifest,
    markdown: &str,
    snapshot: &BrainRepoSnapshot,
    context: &ImportEvidenceContext,
    evidence_refs: &[String],
) -> String {
    let existing_nodes = snapshot
        .nodes
        .iter()
        .take(80)
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
    let existing_edges = snapshot
        .relations
        .iter()
        .take(80)
        .map(|edge| {
            format!(
                "- edgeId: {}, kind: {:?}, source: {}, target: {}, label: {}",
                edge.relation_id, edge.kind, edge.source_node_id, edge.target_node_id, edge.label
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    let existing_claims = snapshot
        .claims
        .iter()
        .take(40)
        .map(|claim| {
            format!(
                "- claimId: {}, statement: {}",
                claim.claim_id, claim.statement
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    let context_evidence_refs = import_evidence_context_allowed_refs(context);
    let retrieval_context = provider_retrieval_context_for_prompt(context);

    format!(
        r#"You are HyprDuck's local brain graph maintenance agent.

Goal:
- Read the new source markdown and the existing brain graph.
- Read the retrieved old source/wiki/memory evidence before proposing graph changes.
- Propose only durable, evidence-backed graph updates.
- Add new nodes, new edges, new claims, and important memories when useful.
- Reuse existing nodeId values when the new source refers to an existing thing.
- Improve old graph state when retrieved old evidence proves a cross-document relationship.
- If you create a new node that an edge or claim will reference, set node.nodeId to a stable id like "node-provider-short-slug" and reuse that exact id.
- Edge contract: if a claim has two or more topicRefs, also emit at least one new_edge connecting those topic nodes.
- Edge contract: if you create a non-source node from this source, also emit a new_edge from the source node to that node using kind "source_of" or "derived_from".
- Do not hide relationships inside claims only. Durable relationships must appear as new_edge payloads.

Hard output rule:
- Return JSON only. No markdown fence, no prose.
- Shape: {{"proposals":[AgentGraphProposalPayload...]}}
- Use camelCase object fields and snake_case enum values.

Allowed payloads:
1. {{"changeType":"new_node","node":{{"label":"...","kind":"concept|topic|person|company|project|product|team|event|decision|task|claim","sourcePath":"...","nodeId":"optional-stable-id","aliases":[],"sourceRefs":["..."],"evidenceRefs":["..."],"reason":"..."}}}}
2. {{"changeType":"new_edge","edge":{{"sourceNodeId":"...","targetNodeId":"...","kind":"mentions|supports|contradicts|supersedes|same_as|works_at|founded|invested_in|advises|attended|owns|responsible_for|decided|blocks|depends_on|source_of|derived_from|related_to","label":"...","sourcePath":"...","edgeId":"optional-stable-id","sourceRefs":["..."],"evidenceRefs":["..."],"reason":"..."}}}}
3. {{"changeType":"new_claim","claim":{{"statement":"...","sourcePath":"...","claimId":"optional-stable-id","topicRefs":["node-id"],"sourceRefs":["..."],"evidenceRefs":["..."],"reason":"..."}}}}
4. {{"changeType":"new_memory","memory":{{"title":"...","body":"...","sourcePath":"...","memoryId":"optional-stable-id","sourceRefs":["..."],"evidenceRefs":["..."],"reason":"..."}}}}

Source:
- sourceId: {source_id}
- sourcePath: {source_path}
- markdownPath: {markdown_path}
- evidenceRefs you may cite: {evidence_refs}
- retrieved context evidenceRefs you may cite: {context_evidence_refs}

Retrieved context:
{retrieval_context}

Existing nodes:
{existing_nodes}

Existing edges:
{existing_edges}

Existing claims:
{existing_claims}

New source markdown:
{markdown}
"#,
        source_id = manifest.source_id,
        source_path = manifest.source_path,
        markdown_path = manifest.markdown_path,
        evidence_refs = join_or_none(evidence_refs),
        context_evidence_refs = join_or_none(&context_evidence_refs),
        retrieval_context = retrieval_context,
        existing_nodes = if existing_nodes.is_empty() {
            "(none)"
        } else {
            &existing_nodes
        },
        existing_edges = if existing_edges.is_empty() {
            "(none)"
        } else {
            &existing_edges
        },
        existing_claims = if existing_claims.is_empty() {
            "(none)"
        } else {
            &existing_claims
        },
        markdown = truncate_for_prompt(markdown, 24000)
    )
}

pub fn provider_retrieval_context_for_prompt(context: &ImportEvidenceContext) -> String {
    let new_chunks = context
        .new_source
        .chunks
        .iter()
        .take(6)
        .map(|chunk| provider_context_chunk_line("new_source", chunk))
        .collect::<Vec<_>>();
    let old_chunks = context
        .retrieved_source_evidence
        .iter()
        .take(20)
        .map(|chunk| provider_context_chunk_line("retrieved_old_source", chunk))
        .collect::<Vec<_>>();
    let queries = context
        .retrieval_queries
        .iter()
        .take(12)
        .map(|query| {
            format!(
                "- query: {} terms: {}",
                query.query,
                join_or_none(&query.terms)
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    format!(
        "Retrieval queries:\n{queries}\n\nNew source chunks:\n{new_chunks}\n\nRetrieved old evidence chunks:\n{old_chunks}",
        queries = if queries.is_empty() { "(none)" } else { &queries },
        new_chunks = if new_chunks.is_empty() {
            "(none)".into()
        } else {
            new_chunks.join("\n")
        },
        old_chunks = if old_chunks.is_empty() {
            "(none)".into()
        } else {
            old_chunks.join("\n")
        }
    )
}

fn provider_context_chunk_line(kind: &str, chunk: &RetrievedEvidenceChunk) -> String {
    format!(
        "- kind: {kind}, evidenceRef: {}, sourceId: {}, sourceTitle: {}, heading: {}, lines: {}-{}, matchedTerms: {}, text: {}",
        chunk.evidence_ref_id,
        chunk.source_id,
        chunk.source_title,
        join_or_none(&chunk.heading_path),
        chunk.line_start,
        chunk.line_end,
        join_or_none(&chunk.matched_terms),
        truncate_for_prompt(&chunk.text, 1200)
    )
}

fn join_or_none(values: &[String]) -> String {
    if values.is_empty() {
        "none".into()
    } else {
        values.join(", ")
    }
}

fn truncate_for_prompt(value: &str, max_chars: usize) -> String {
    let mut output = value.chars().take(max_chars).collect::<String>();
    if value.chars().count() > max_chars {
        output.push_str("\n...[truncated]");
    }
    output
}
