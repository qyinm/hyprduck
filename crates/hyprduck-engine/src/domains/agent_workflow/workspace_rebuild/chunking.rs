//! Internal helpers extracted from the engine facade module.

use std::path::Path;

use anyhow::Result;
use serde_json::json;

use super::{
    write_json_pretty, GraphCandidateBatch, GraphCandidateNode, GraphCandidateRelation,
    SOURCE_GRAPH_AUTO_BATCH_LIMIT, SOURCE_GRAPH_CHUNK_BATCH_MAX_CHARS,
    SOURCE_GRAPH_CHUNK_BATCH_MAX_CHUNKS,
};
use crate::{
    chunk_source_markdown, unix_timestamp_seconds, BrainRepoSnapshot, EvidenceRef,
    SourceArtifactManifest,
};

pub(super) struct SourceGraphChunkBatchPlan {
    pub(super) batches: Vec<Vec<crate::source_index::SourceChunk>>,
    pub(super) discovered_batch_count: usize,
    pub(super) skipped_batch_count: usize,
}

pub(super) fn source_graph_chunk_batch_plan(
    manifest: &SourceArtifactManifest,
    markdown: &str,
) -> SourceGraphChunkBatchPlan {
    let mut batches = Vec::new();
    let mut current = Vec::new();
    let mut current_chars = 0usize;

    for chunk in chunk_source_markdown(manifest, markdown) {
        let chunk_chars = chunk.text.chars().count();
        let would_exceed_chars =
            !current.is_empty() && current_chars + chunk_chars > SOURCE_GRAPH_CHUNK_BATCH_MAX_CHARS;
        let would_exceed_count = current.len() >= SOURCE_GRAPH_CHUNK_BATCH_MAX_CHUNKS;
        if would_exceed_chars || would_exceed_count {
            batches.push(std::mem::take(&mut current));
            current_chars = 0;
        }
        current_chars += chunk_chars;
        current.push(chunk);
    }

    if !current.is_empty() {
        batches.push(current);
    }
    let discovered_batch_count = batches.len();
    let skipped_batch_count = discovered_batch_count.saturating_sub(SOURCE_GRAPH_AUTO_BATCH_LIMIT);
    SourceGraphChunkBatchPlan {
        batches: batches
            .into_iter()
            .take(SOURCE_GRAPH_AUTO_BATCH_LIMIT)
            .collect(),
        discovered_batch_count,
        skipped_batch_count,
    }
}

#[cfg(test)]
pub(super) fn source_graph_chunk_batches(
    manifest: &SourceArtifactManifest,
    markdown: &str,
) -> Vec<Vec<crate::source_index::SourceChunk>> {
    source_graph_chunk_batch_plan(manifest, markdown).batches
}

pub(super) fn write_source_chunk_run_artifact(
    artifact_root: &Path,
    run_id: &str,
    workspace_id: &str,
    source_id: &str,
    chunk_ids: &[String],
    status: &str,
    error_message: Option<String>,
) -> Result<()> {
    write_json_pretty(
        &artifact_root
            .join("provider-graph-chunks")
            .join(format!("{run_id}.json")),
        &json!({
            "runId": run_id,
            "workspaceId": workspace_id,
            "sourceId": source_id,
            "stage": "source_chunk_extract",
            "status": status,
            "chunkIds": chunk_ids,
            "errorMessage": error_message,
            "updatedAt": unix_timestamp_seconds(),
        }),
    )
}

pub(super) fn write_graph_candidate_batch_artifact(
    artifact_root: &Path,
    source_run_id: &str,
    chunk_run_id: &str,
    _workspace_id: &str,
    source_id: &str,
    chunk_ids: &[String],
    snapshot: &BrainRepoSnapshot,
) -> Result<()> {
    let batch = GraphCandidateBatch {
        run_id: source_run_id.to_string(),
        chunk_run_id: chunk_run_id.to_string(),
        source_id: source_id.to_string(),
        chunk_ids: chunk_ids.to_vec(),
        nodes: snapshot
            .nodes
            .iter()
            .filter(|node| !node.node_id.starts_with("source:"))
            .map(|node| GraphCandidateNode {
                raw_node_id: node.node_id.clone(),
                label: node.label.clone(),
                kind: node.kind,
                aliases: node.aliases.clone(),
                evidence_ids: node.evidence_ids.clone(),
                page_refs: Vec::new(),
                confidence: node.confidence,
                reason: None,
            })
            .collect(),
        relations: snapshot
            .relations
            .iter()
            .map(|relation| GraphCandidateRelation {
                raw_relation_id: relation.relation_id.clone(),
                source_raw_node_id: relation.source_node_id.clone(),
                target_raw_node_id: relation.target_node_id.clone(),
                kind: relation.kind,
                label: relation.label.clone(),
                evidence_ids: relation.evidence_ids.clone(),
                confidence: relation.confidence,
            })
            .collect(),
        claims: snapshot.claims.clone(),
        raw_response_ref: Some(format!("runs/{chunk_run_id}/provider-response.json")),
        created_at: unix_timestamp_seconds(),
    };
    write_json_pretty(
        &artifact_root
            .join("provider-graph-candidates")
            .join(format!("{chunk_run_id}.json")),
        &batch,
    )
}

pub(super) fn source_chunk_evidence_refs(
    batch: &[crate::source_index::SourceChunk],
) -> Vec<EvidenceRef> {
    batch
        .iter()
        .map(|chunk| EvidenceRef {
            id: format!("retrieved:{}:{}", chunk.source_id, chunk.chunk_id),
            page_label: if chunk.heading_path.is_empty() {
                format!("Lines {}-{}", chunk.line_start, chunk.line_end)
            } else {
                chunk.heading_path.join(" / ")
            },
            page_index: None,
            snippet: truncate_evidence_snippet(&chunk.text, 280),
            source_path: Some(chunk.source_path.clone()),
            source_id: Some(chunk.source_id.clone()),
            markdown_path: Some(chunk.markdown_path.clone()),
            image_path: None,
            provenance: Some(format!(
                "Source chunk {} lines {}-{} selected for provider graph extraction.",
                chunk.chunk_id, chunk.line_start, chunk.line_end
            )),
        })
        .collect()
}

fn truncate_evidence_snippet(value: &str, max_chars: usize) -> String {
    let mut output = value.chars().take(max_chars).collect::<String>();
    if value.chars().count() > max_chars {
        output.push_str("...");
    }
    output
}
