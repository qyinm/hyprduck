//! Source graph response parsing and merge helpers.

use std::collections::BTreeMap;

#[cfg(test)]
use anyhow::bail;
use anyhow::Result;

use super::super::response::{
    normalize_provider_source_local_graph_snapshot, parse_provider_workspace_rebuild_snapshot,
};
use super::super::validation::validate_provider_source_local_graph_snapshot;
use super::workspace_rebuild_compaction::strip_source_of_relations;
#[cfg(test)]
use super::workspace_rebuild_compaction::{
    compact_source_graph_snapshot, SourceGraphCompactionResult,
};
use crate::{
    empty_replayed_brain_snapshot, unix_timestamp_seconds, BrainNodeRecord, BrainRelationKind,
    BrainRelationRecord, BrainRepoSnapshot, ClaimRecord, EntityRecord, EvidenceRef, MemoryRecord,
    StructuredExtractionArtifact, WikiPage,
};

pub(super) fn parse_and_validate_source_graph_response(
    response: &str,
    workspace_id: &str,
    baseline: &BrainRepoSnapshot,
    source_id: &str,
    batch_evidence: &[EvidenceRef],
) -> Result<(BrainRepoSnapshot, usize)> {
    let mut snapshot = parse_provider_workspace_rebuild_snapshot(response)?;
    let provider_source_of_relation_count = snapshot
        .relations
        .iter()
        .filter(|relation| relation.kind == BrainRelationKind::SourceOf)
        .count();
    let mut stage_baseline = baseline.clone();
    stage_baseline.evidence = batch_evidence.to_vec();
    normalize_provider_source_local_graph_snapshot(
        &mut snapshot,
        workspace_id,
        &stage_baseline,
        source_id,
        unix_timestamp_seconds(),
    );
    validate_provider_source_local_graph_snapshot(&snapshot, source_id)?;
    Ok((snapshot, provider_source_of_relation_count))
}

#[cfg(test)]
pub(super) fn merge_source_graph_snapshots(
    workspace_id: &str,
    baseline: &BrainRepoSnapshot,
    source_id: &str,
    snapshots: Vec<BrainRepoSnapshot>,
    stripped_source_of_relation_count: usize,
) -> Result<SourceGraphCompactionResult> {
    if snapshots.is_empty() {
        bail!("no valid source graph chunk output to materialize");
    }

    let raw_snapshot =
        merge_raw_source_graph_snapshots(workspace_id, baseline, source_id, snapshots)?;
    let (canonical_snapshot, report) = compact_source_graph_snapshot(
        workspace_id,
        baseline,
        source_id,
        &raw_snapshot,
        stripped_source_of_relation_count,
    )?;
    Ok(SourceGraphCompactionResult {
        raw_snapshot,
        canonical_snapshot,
        report,
    })
}

pub(super) fn merge_raw_source_graph_snapshots(
    workspace_id: &str,
    baseline: &BrainRepoSnapshot,
    source_id: &str,
    snapshots: Vec<BrainRepoSnapshot>,
) -> Result<BrainRepoSnapshot> {
    let mut nodes = BTreeMap::<String, BrainNodeRecord>::new();
    let mut relations = BTreeMap::<String, BrainRelationRecord>::new();
    let mut evidence = BTreeMap::<String, EvidenceRef>::new();
    let mut claims = BTreeMap::<String, ClaimRecord>::new();
    let mut memories = BTreeMap::<String, MemoryRecord>::new();
    let mut wiki_pages = BTreeMap::<String, WikiPage>::new();
    let mut entities = BTreeMap::<String, EntityRecord>::new();
    let mut extractions = BTreeMap::<String, StructuredExtractionArtifact>::new();

    for snapshot in snapshots {
        for evidence_ref in snapshot.evidence {
            evidence
                .entry(evidence_ref.id.clone())
                .or_insert(evidence_ref);
        }
        for node in snapshot.nodes {
            nodes.entry(node.node_id.clone()).or_insert(node);
        }
        for relation in snapshot.relations {
            relations
                .entry(relation.relation_id.clone())
                .or_insert(relation);
        }
        for claim in snapshot.claims {
            claims.entry(claim.claim_id.clone()).or_insert(claim);
        }
        for memory in snapshot.memories {
            memories.entry(memory.memory_id.clone()).or_insert(memory);
        }
        for page in snapshot.wiki_pages {
            wiki_pages.entry(page.page_id.clone()).or_insert(page);
        }
        for entity in snapshot.entities {
            entities.entry(entity.entity_id.clone()).or_insert(entity);
        }
        for extraction in snapshot.extractions {
            extractions
                .entry(extraction.artifact_id.clone())
                .or_insert(extraction);
        }
    }

    let mut merged = empty_replayed_brain_snapshot(workspace_id);
    merged.generated_at = unix_timestamp_seconds();
    merged.sources = baseline
        .sources
        .iter()
        .filter(|source| source.source_id == source_id)
        .cloned()
        .collect();
    merged.evidence = evidence.into_values().collect();
    merged.nodes = nodes.into_values().collect();
    merged.relations = relations.into_values().collect();
    merged.claims = claims.into_values().collect();
    merged.memories = memories.into_values().collect();
    merged.wiki_pages = wiki_pages.into_values().collect();
    merged.entities = entities.into_values().collect();
    merged.extractions = extractions.into_values().collect();

    let mut source_local_baseline = baseline.clone();
    source_local_baseline.evidence = merged.evidence.clone();
    normalize_provider_source_local_graph_snapshot(
        &mut merged,
        workspace_id,
        &source_local_baseline,
        source_id,
        unix_timestamp_seconds(),
    );
    validate_provider_source_local_graph_snapshot(&merged, source_id)?;
    strip_source_of_relations(&mut merged);
    Ok(merged)
}
