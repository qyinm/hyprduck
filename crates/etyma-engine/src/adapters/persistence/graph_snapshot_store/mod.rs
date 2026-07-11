use anyhow::{anyhow, Context, Result};
use graphqlite::Graph;
use etyma_engine_types::{
    BrainNodeKind, BrainNodeRecord, BrainRepoSnapshot, BrainScope,
};
use std::collections::{BTreeMap, BTreeSet};

mod brain_events;
mod checkpoint;
mod graphlite_props;
mod nodes;
mod relations;
mod sources_pages_evidence;
mod wiki_revisions;

use brain_events::persist_brain_events_snapshot_in_transaction;
use checkpoint::{
    mark_import_jobs_graph_ready_in_transaction, persist_graph_checkpoint_metadata_in_transaction,
    persist_graph_evidence_record_index_in_transaction,
};
use graphlite_props::{brain_node_label, graph_relation_version_type};
use nodes::{
    graph_endpoint_version_id, invalidate_live_graph_node_versions,
    invalidate_live_graph_node_versions_not_in, node_graph_metadata, node_graph_properties,
};
use relations::{
    invalidate_live_graph_relation_versions, invalidate_live_graph_relation_versions_not_in,
    relation_graph_metadata, relation_graph_properties,
};

pub(super) use nodes::graph_node_version_identity;
pub(super) use relations::graph_relation_version_identity;
pub(super) use sources_pages_evidence::purge_workspace_source_in_transaction;
use sources_pages_evidence::{
    persist_evidence_snapshot_in_transaction, persist_snapshot_sources_in_transaction,
    persist_source_pages_snapshot_in_transaction,
};
use wiki_revisions::persist_wiki_pages_snapshot_in_transaction;

pub(crate) const GRAPHQLITE_SCHEMA_VERSION: i64 = 2;
#[allow(dead_code)]
pub(super) const GRAPH_VERSION_LEGACY_EVENT_ID: &str = "legacy-graphqlite-current";

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub(crate) struct KnowledgeGraphPersistReport {
    pub(crate) node_count: usize,
    pub(crate) relation_count: usize,
}

pub(super) fn persist_graph_snapshot_in_transaction(
    graph: &Graph,
    snapshot: &BrainRepoSnapshot,
) -> Result<KnowledgeGraphPersistReport> {
    persist_snapshot_sources_in_transaction(graph, snapshot)?;
    persist_source_pages_snapshot_in_transaction(graph, snapshot)?;
    persist_evidence_snapshot_in_transaction(graph, snapshot)?;
    validate_snapshot_evidence_refs(snapshot)?;
    persist_wiki_pages_snapshot_in_transaction(graph, snapshot)?;
    persist_brain_events_snapshot_in_transaction(graph, snapshot)?;

    let created_by_event_id = graph_snapshot_created_by_event_id(snapshot);
    let graph_nodes = current_graph_nodes(snapshot);
    let current_node_logical_ids = graph_nodes
        .iter()
        .map(|node| node.node_id.clone())
        .collect::<BTreeSet<_>>();
    let mut node_version_ids_by_logical_id = BTreeMap::new();
    for node in &graph_nodes {
        let metadata = node_graph_metadata(snapshot, node);
        let identity = graph_node_version_identity(
            &snapshot.workspace_id,
            &node.node_id,
            &created_by_event_id,
        );
        graph
            .upsert_node(
                &identity.version_id,
                node_graph_properties(&snapshot.workspace_id, node, &metadata, &identity),
                brain_node_label(node.kind),
            )
            .with_context(|| format!("failed upserting GraphQLite node {}", node.node_id))?;
        invalidate_live_graph_node_versions(
            graph,
            &snapshot.workspace_id,
            &identity.logical_id,
            Some(&identity.version_id),
            graph_record_invalidation_time(snapshot, node.valid_from, node.valid_to),
            &identity.created_by_event_id,
        )?;
        node_version_ids_by_logical_id.insert(identity.logical_id, identity.version_id);
    }
    invalidate_live_graph_node_versions_not_in(
        graph,
        &snapshot.workspace_id,
        &current_node_logical_ids,
        snapshot.generated_at as i64,
        &created_by_event_id,
    )?;

    let current_relation_logical_ids = snapshot
        .relations
        .iter()
        .map(|relation| relation.relation_id.clone())
        .collect::<BTreeSet<_>>();
    for relation in &snapshot.relations {
        let metadata = relation_graph_metadata(snapshot, relation);
        let identity = graph_relation_version_identity(
            &snapshot.workspace_id,
            &relation.relation_id,
            &created_by_event_id,
        );
        let Some(source_version_id) = graph_endpoint_version_id(
            graph,
            &snapshot.workspace_id,
            &node_version_ids_by_logical_id,
            &relation.source_node_id,
        )?
        else {
            continue;
        };
        let Some(target_version_id) = graph_endpoint_version_id(
            graph,
            &snapshot.workspace_id,
            &node_version_ids_by_logical_id,
            &relation.target_node_id,
        )?
        else {
            continue;
        };
        let relation_type = graph_relation_version_type(relation.kind, &identity);
        graph
            .upsert_edge(
                &source_version_id,
                &target_version_id,
                relation_graph_properties(&snapshot.workspace_id, relation, &metadata, &identity),
                &relation_type,
            )
            .with_context(|| {
                format!(
                    "failed upserting GraphQLite relation {}",
                    relation.relation_id
                )
            })?;
        invalidate_live_graph_relation_versions(
            graph,
            &snapshot.workspace_id,
            &identity.logical_id,
            Some(&identity.version_id),
            graph_record_invalidation_time(snapshot, relation.valid_from, relation.valid_to),
            &identity.created_by_event_id,
        )?;
    }
    invalidate_live_graph_relation_versions_not_in(
        graph,
        &snapshot.workspace_id,
        &current_relation_logical_ids,
        snapshot.generated_at as i64,
        &created_by_event_id,
    )?;
    persist_graph_evidence_record_index_in_transaction(graph, &snapshot.workspace_id)?;

    mark_import_jobs_graph_ready_in_transaction(graph, snapshot)?;

    let live_graph_node_ids = graph_nodes
        .iter()
        .filter(|node| node.valid_to.is_none())
        .map(|node| node.node_id.as_str())
        .collect::<BTreeSet<_>>();
    let report = KnowledgeGraphPersistReport {
        node_count: live_graph_node_ids.len(),
        relation_count: snapshot
            .relations
            .iter()
            .filter(|relation| {
                relation.valid_to.is_none()
                    && live_graph_node_ids.contains(relation.source_node_id.as_str())
                    && live_graph_node_ids.contains(relation.target_node_id.as_str())
            })
            .count(),
    };
    persist_graph_checkpoint_metadata_in_transaction(graph, snapshot, &report)?;

    Ok(report)
}


#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct GraphRecordMetadata {
    source_ids: Vec<String>,
    producer_run_ids: Vec<String>,
    status: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
#[allow(dead_code)]
pub(super) struct GraphRecordVersionIdentity {
    pub(super) logical_id: String,
    pub(super) version_id: String,
    pub(super) created_by_event_id: String,
}

#[allow(dead_code)]
pub(super) fn graph_snapshot_created_by_event_id(snapshot: &BrainRepoSnapshot) -> String {
    snapshot
        .events
        .last()
        .map(|event| event.event_id.clone())
        .unwrap_or_else(|| {
            format!(
                "snapshot-{}-{}",
                snapshot.workspace_id, snapshot.generated_at
            )
        })
}

#[allow(dead_code)]
fn graph_record_version_id(
    record_kind: &str,
    workspace_id: &str,
    logical_id: &str,
    created_by_event_id: &str,
) -> String {
    let payload = serde_json::json!({
        "kind": record_kind,
        "workspace": workspace_id,
        "logical": logical_id,
        "event": created_by_event_id,
    });
    let encoded = serde_json::to_vec(&payload).unwrap_or_else(|_| {
        format!("{record_kind}:{workspace_id}:{logical_id}:{created_by_event_id}").into_bytes()
    });
    let digest = hex_digest(ring::digest::digest(&ring::digest::SHA256, &encoded).as_ref());
    format!("etyma-{record_kind}-version-{}", &digest[..32])
}

fn graph_record_invalidation_time(
    snapshot: &BrainRepoSnapshot,
    valid_from: u64,
    valid_to: Option<u64>,
) -> i64 {
    valid_to
        .or((valid_from > 0).then_some(valid_from))
        .unwrap_or(snapshot.generated_at) as i64
}

fn producer_run_ids_for_refs(
    snapshot: &BrainRepoSnapshot,
    evidence_ids: &[String],
    source_ids: &[String],
) -> Vec<String> {
    let mut producer_run_ids = snapshot
        .extractions
        .iter()
        .filter(|extraction| {
            source_ids.contains(&extraction.source_id)
                || extraction
                    .source_refs
                    .iter()
                    .any(|source_id| source_ids.contains(source_id))
                || extraction
                    .evidence_refs
                    .iter()
                    .any(|evidence| evidence_ids.contains(&evidence.id))
        })
        .map(|extraction| extraction.artifact_id.clone())
        .collect::<Vec<_>>();
    producer_run_ids.sort();
    producer_run_ids.dedup();
    producer_run_ids
}

fn current_graph_nodes(snapshot: &BrainRepoSnapshot) -> Vec<BrainNodeRecord> {
    let mut nodes_by_id = snapshot
        .nodes
        .iter()
        .cloned()
        .map(|node| (node.node_id.clone(), node))
        .collect::<std::collections::BTreeMap<_, _>>();

    for source in &snapshot.sources {
        let node_id = format!("source:{}", source.source_id);
        nodes_by_id
            .entry(node_id.clone())
            .or_insert(BrainNodeRecord {
                node_id,
                kind: BrainNodeKind::Source,
                label: source.source_id.clone(),
                scope: BrainScope::Project,
                aliases: Vec::new(),
                evidence_ids: Vec::new(),
                source_ids: vec![source.source_id.clone()],
                confidence: None,
                updated_at: source.updated_at,
                valid_from: 0,
                valid_to: None,
                superseded_by: None,
            });
    }
    for wiki_page in &snapshot.wiki_pages {
        nodes_by_id
            .entry(wiki_page.page_id.clone())
            .or_insert(BrainNodeRecord {
                node_id: wiki_page.page_id.clone(),
                kind: BrainNodeKind::WikiPage,
                label: wiki_page.title.clone(),
                scope: BrainScope::Project,
                aliases: vec![wiki_page.path.clone()],
                evidence_ids: wiki_page.evidence_refs.clone(),
                source_ids: wiki_page.source_refs.clone(),
                confidence: None,
                updated_at: wiki_page.updated_at,
                valid_from: 0,
                valid_to: None,
                superseded_by: None,
            });
    }
    for entity in &snapshot.entities {
        nodes_by_id
            .entry(entity.entity_id.clone())
            .or_insert(BrainNodeRecord {
                node_id: entity.entity_id.clone(),
                kind: entity.kind,
                label: entity.name.clone(),
                scope: BrainScope::Project,
                aliases: entity.aliases.clone(),
                evidence_ids: entity.evidence_refs.clone(),
                source_ids: entity.source_refs.clone(),
                confidence: None,
                updated_at: entity.updated_at,
                valid_from: 0,
                valid_to: None,
                superseded_by: None,
            });
    }
    for claim in &snapshot.claims {
        nodes_by_id
            .entry(claim.claim_id.clone())
            .or_insert(BrainNodeRecord {
                node_id: claim.claim_id.clone(),
                kind: BrainNodeKind::Claim,
                label: claim.statement.clone(),
                scope: BrainScope::Project,
                aliases: claim.topic_refs.clone(),
                evidence_ids: claim.evidence_refs.clone(),
                source_ids: claim.source_refs.clone(),
                confidence: None,
                updated_at: claim.updated_at,
                valid_from: 0,
                valid_to: None,
                superseded_by: None,
            });
    }

    nodes_by_id.into_values().collect()
}

fn hex_digest(bytes: &[u8]) -> String {
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        output.push_str(&format!("{byte:02x}"));
    }
    output
}

fn validate_snapshot_evidence_refs(snapshot: &BrainRepoSnapshot) -> Result<()> {
    let evidence_ids = snapshot
        .evidence
        .iter()
        .map(|evidence| evidence.id.as_str())
        .collect::<std::collections::BTreeSet<_>>();
    for node in &snapshot.nodes {
        validate_record_evidence_refs(
            &format!("node {}", node.node_id),
            &node.evidence_ids,
            &evidence_ids,
        )?;
    }
    for relation in &snapshot.relations {
        validate_record_evidence_refs(
            &format!("relation {}", relation.relation_id),
            &relation.evidence_ids,
            &evidence_ids,
        )?;
    }
    for wiki_page in &snapshot.wiki_pages {
        validate_record_evidence_refs(
            &format!("wiki page {}", wiki_page.page_id),
            &wiki_page.evidence_refs,
            &evidence_ids,
        )?;
    }
    for claim in &snapshot.claims {
        validate_record_evidence_refs(
            &format!("claim {}", claim.claim_id),
            &claim.evidence_refs,
            &evidence_ids,
        )?;
    }
    for memory in &snapshot.memories {
        validate_record_evidence_refs(
            &format!("memory {}", memory.memory_id),
            &memory.evidence_refs,
            &evidence_ids,
        )?;
    }
    Ok(())
}

fn validate_record_evidence_refs(
    record_label: &str,
    refs: &[String],
    evidence_ids: &std::collections::BTreeSet<&str>,
) -> Result<()> {
    for evidence_ref in refs {
        if !evidence_ids.contains(evidence_ref.as_str()) {
            return Err(anyhow!(
                "{} references missing relational evidence row {}",
                record_label,
                evidence_ref
            ));
        }
    }
    Ok(())
}

