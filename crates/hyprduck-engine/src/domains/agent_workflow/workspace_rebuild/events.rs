//! Provider graph event and artifact helpers.

use std::collections::BTreeSet;
use std::path::Path;

use anyhow::Result;
use serde_json::json;

use super::{write_json_pretty, SourceArtifactManifest};
use crate::{
    materialized_graph_event_payload_json, BrainActor, BrainActorType, BrainEvent,
    BrainEventCausality, BrainEventKind, BrainRepoSnapshot, BrainScope, BRAIN_EVENT_SCHEMA_VERSION,
    PROVIDER_GRAPH_AGENT_ID,
};

pub(super) fn write_graph_diff_artifact(
    workspace_root: &Path,
    run_id: &str,
    workspace_id: &str,
    source_id: &str,
    changed_files: &[String],
    snapshot: &BrainRepoSnapshot,
) -> Result<()> {
    write_json_pretty(
        &workspace_root
            .join("runs")
            .join(run_id)
            .join("graph-diff.json"),
        &json!({
            "runId": run_id,
            "workspaceId": workspace_id,
            "sourceId": source_id,
            "changedFiles": changed_files,
            "nodeCount": snapshot.nodes.len(),
            "relationCount": snapshot.relations.len(),
            "claimCount": snapshot.claims.len(),
            "memoryCount": snapshot.memories.len(),
            "updatedAt": snapshot.generated_at,
        }),
    )
}

pub(super) fn source_graph_build_materialized_event(
    workspace_id: &str,
    run_id: &str,
    manifest: &SourceArtifactManifest,
    snapshot: &BrainRepoSnapshot,
) -> Result<BrainEvent> {
    provider_graph_materialized_event(
        workspace_id,
        run_id,
        manifest,
        snapshot,
        "source_graph_build",
        "source-graph-build",
        "provider_source_graph_build",
    )
}

pub(super) fn workspace_linking_materialized_event(
    workspace_id: &str,
    run_id: &str,
    manifest: &SourceArtifactManifest,
    snapshot: &BrainRepoSnapshot,
) -> Result<BrainEvent> {
    let endpoint_node_ids = snapshot
        .relations
        .iter()
        .flat_map(|relation| {
            [
                relation.source_node_id.clone(),
                relation.target_node_id.clone(),
            ]
        })
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect::<Vec<_>>();
    Ok(BrainEvent {
        event_id: format!("evt-{run_id}"),
        schema_version: BRAIN_EVENT_SCHEMA_VERSION,
        workspace_id: workspace_id.to_string(),
        scope: BrainScope::Project,
        event_type: BrainEventKind::GraphMaterialized,
        operation_type: Some("workspace_linking".into()),
        actor: BrainActor {
            actor_type: BrainActorType::Agent,
            actor_id: format!("{PROVIDER_GRAPH_AGENT_ID}:workspace-linking"),
        },
        source_refs: snapshot
            .relations
            .iter()
            .flat_map(|relation| {
                snapshot
                    .nodes
                    .iter()
                    .filter(move |node| {
                        node.node_id == relation.source_node_id
                            || node.node_id == relation.target_node_id
                    })
                    .flat_map(|node| node.source_ids.clone())
            })
            .chain(std::iter::once(manifest.source_id.clone()))
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect(),
        source_markdown_refs: vec![manifest.markdown_path.clone()],
        node_refs: endpoint_node_ids,
        relation_refs: snapshot
            .relations
            .iter()
            .map(|relation| relation.relation_id.clone())
            .collect(),
        claim_refs: snapshot
            .claims
            .iter()
            .map(|claim| claim.claim_id.clone())
            .collect(),
        memory_refs: snapshot
            .memories
            .iter()
            .map(|memory| memory.memory_id.clone())
            .collect(),
        target_node_ids: Vec::new(),
        target_edge_ids: snapshot
            .relations
            .iter()
            .map(|relation| relation.relation_id.clone())
            .collect(),
        target_claim_ids: snapshot
            .claims
            .iter()
            .map(|claim| claim.claim_id.clone())
            .collect(),
        target_memory_ids: snapshot
            .memories
            .iter()
            .map(|memory| memory.memory_id.clone())
            .collect(),
        evidence_refs: Vec::new(),
        payload_json: materialized_graph_event_payload_json(
            snapshot.generated_at,
            &[],
            &[],
            &snapshot.relations,
            &[],
            &snapshot.memories,
            &snapshot.wiki_pages,
            &[],
            &snapshot.claims,
            &[],
        )?,
        causality: BrainEventCausality {
            caused_by_source_ids: vec![manifest.source_id.clone()],
            caused_by_event_ids: Vec::new(),
            snapshot_id: Some(format!("snapshot-{run_id}")),
            previous_snapshot_id: None,
            materialized_version: Some(snapshot.generated_at),
            schema_version: 1,
        },
        confidence: Some("provider_workspace_linking".into()),
        policy_result: "materialized".into(),
        created_at: snapshot.generated_at,
    })
}

fn provider_graph_materialized_event(
    workspace_id: &str,
    run_id: &str,
    manifest: &SourceArtifactManifest,
    snapshot: &BrainRepoSnapshot,
    operation_type: &str,
    actor_suffix: &str,
    confidence: &str,
) -> Result<BrainEvent> {
    Ok(BrainEvent {
        event_id: format!("evt-{run_id}"),
        schema_version: BRAIN_EVENT_SCHEMA_VERSION,
        workspace_id: workspace_id.to_string(),
        scope: BrainScope::Project,
        event_type: BrainEventKind::GraphMaterialized,
        operation_type: Some(operation_type.into()),
        actor: BrainActor {
            actor_type: BrainActorType::Agent,
            actor_id: format!("{PROVIDER_GRAPH_AGENT_ID}:{actor_suffix}"),
        },
        source_refs: snapshot
            .sources
            .iter()
            .map(|source| source.source_id.clone())
            .collect(),
        source_markdown_refs: vec![manifest.markdown_path.clone()],
        node_refs: snapshot
            .nodes
            .iter()
            .map(|node| node.node_id.clone())
            .collect(),
        relation_refs: snapshot
            .relations
            .iter()
            .map(|relation| relation.relation_id.clone())
            .collect(),
        claim_refs: snapshot
            .claims
            .iter()
            .map(|claim| claim.claim_id.clone())
            .collect(),
        memory_refs: snapshot
            .memories
            .iter()
            .map(|memory| memory.memory_id.clone())
            .collect(),
        target_node_ids: snapshot
            .nodes
            .iter()
            .map(|node| node.node_id.clone())
            .collect(),
        target_edge_ids: snapshot
            .relations
            .iter()
            .map(|relation| relation.relation_id.clone())
            .collect(),
        target_claim_ids: snapshot
            .claims
            .iter()
            .map(|claim| claim.claim_id.clone())
            .collect(),
        target_memory_ids: snapshot
            .memories
            .iter()
            .map(|memory| memory.memory_id.clone())
            .collect(),
        evidence_refs: snapshot
            .evidence
            .iter()
            .map(|evidence| evidence.id.clone())
            .collect(),
        payload_json: materialized_graph_event_payload_json(
            snapshot.generated_at,
            &snapshot.sources,
            &snapshot.nodes,
            &snapshot.relations,
            &snapshot.evidence,
            &snapshot.memories,
            &snapshot.wiki_pages,
            &snapshot.entities,
            &snapshot.claims,
            &snapshot.extractions,
        )?,
        causality: BrainEventCausality {
            caused_by_source_ids: vec![manifest.source_id.clone()],
            caused_by_event_ids: Vec::new(),
            snapshot_id: Some(format!("snapshot-{run_id}")),
            previous_snapshot_id: None,
            materialized_version: Some(snapshot.generated_at),
            schema_version: 1,
        },
        confidence: Some(confidence.into()),
        policy_result: "materialized".into(),
        created_at: snapshot.generated_at,
    })
}
