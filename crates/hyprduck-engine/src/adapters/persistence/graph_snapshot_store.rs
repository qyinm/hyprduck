use anyhow::{anyhow, Context, Result};
use graphqlite::{Graph, PropertyValue};
use hyprduck_engine_types::{
    BrainNodeKind, BrainNodeRecord, BrainRelationKind, BrainRelationRecord, BrainRepoSnapshot,
    BrainScope, WikiPage,
};
use std::collections::{BTreeMap, BTreeSet};

use crate::policy::redact_path_for_agent;

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

fn persist_graph_checkpoint_metadata_in_transaction(
    graph: &Graph,
    snapshot: &BrainRepoSnapshot,
    report: &KnowledgeGraphPersistReport,
) -> Result<()> {
    if report.node_count == 0 && report.relation_count == 0 {
        return Ok(());
    }
    let sqlite = graph.connection().sqlite_connection();
    let actor_json = serde_json::json!({
        "actorType": "system",
        "actorId": "hyprduck-knowledge-store"
    })
    .to_string();
    let checksum = graph_checkpoint_checksum(snapshot, report)?;
    let checkpoint_id = graph_checkpoint_id(snapshot, &checksum);
    let evidence_ref_count = snapshot
        .nodes
        .iter()
        .flat_map(|node| node.evidence_ids.iter())
        .chain(
            snapshot
                .relations
                .iter()
                .flat_map(|relation| relation.evidence_ids.iter()),
        )
        .chain(
            snapshot
                .wiki_pages
                .iter()
                .flat_map(|wiki| wiki.evidence_refs.iter()),
        )
        .chain(
            snapshot
                .claims
                .iter()
                .flat_map(|claim| claim.evidence_refs.iter()),
        )
        .collect::<BTreeSet<_>>()
        .len();
    sqlite
        .execute(
            "INSERT INTO graph_checkpoints (
                checkpoint_id,
                workspace_id,
                reason,
                actor_json,
                related_event_id,
                graph_schema_version,
                graphqlite_extension_version,
                node_count,
                edge_count,
                evidence_ref_count,
                checksum,
                storage_ref,
                created_at
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)
            ON CONFLICT(checkpoint_id) DO NOTHING",
            (
                checkpoint_id.as_str(),
                snapshot.workspace_id.as_str(),
                "graph_snapshot_commit",
                actor_json.as_str(),
                snapshot.events.last().map(|event| event.event_id.as_str()),
                GRAPHQLITE_SCHEMA_VERSION,
                graphqlite_extension_version(),
                report.node_count as i64,
                report.relation_count as i64,
                evidence_ref_count as i64,
                checksum.as_str(),
                "hyprduck.sqlite:graphqlite",
                snapshot.generated_at as i64,
            ),
        )
        .context("failed storing graph checkpoint metadata")?;
    Ok(())
}

fn persist_graph_evidence_record_index_in_transaction(
    graph: &Graph,
    workspace_id: &str,
) -> Result<()> {
    let sqlite = graph.connection().sqlite_connection();
    sqlite
        .execute(
            "DELETE FROM graph_evidence_record_index WHERE workspace_id = ?1",
            [workspace_id],
        )
        .context("failed clearing graph evidence record index")?;
    persist_graph_evidence_record_index_for_table(
        graph,
        workspace_id,
        "node",
        "node_props_text",
        "node_props_int",
        "node_id",
        "logical_id",
    )?;
    persist_graph_evidence_record_index_for_table(
        graph,
        workspace_id,
        "edge",
        "edge_props_text",
        "edge_props_int",
        "edge_id",
        "relation_id",
    )?;
    Ok(())
}

fn persist_graph_evidence_record_index_for_table(
    graph: &Graph,
    workspace_id: &str,
    record_kind: &str,
    table: &str,
    int_table: &str,
    id_column: &str,
    logical_id_key: &str,
) -> Result<()> {
    let sqlite = graph.connection().sqlite_connection();
    let logical_key_id = ensure_graphlite_property_key_id(graph, logical_id_key)?;
    let version_key_id = ensure_graphlite_property_key_id(graph, "version_id")?;
    let event_key_id = ensure_graphlite_property_key_id(graph, "created_by_event_id")?;
    let valid_from_key_id = ensure_graphlite_property_key_id(graph, "valid_from")?;
    let valid_to_key_id = ensure_graphlite_property_key_id(graph, "valid_to")?;
    let mut query = sqlite
        .prepare(&format!(
            "SELECT evidence.{id_column},
                    evidence.value,
                    logical.value,
                    version.value,
                    event.value,
                    COALESCE(valid_from.value, 0),
                    COALESCE(valid_to.value, 0)
             FROM {table} evidence
             JOIN {table} workspace ON workspace.{id_column} = evidence.{id_column}
             LEFT JOIN {table} logical
               ON logical.{id_column} = evidence.{id_column}
              AND logical.key_id = ?2
             LEFT JOIN {table} version
               ON version.{id_column} = evidence.{id_column}
              AND version.key_id = ?3
             LEFT JOIN {table} event
               ON event.{id_column} = evidence.{id_column}
              AND event.key_id = ?4
             LEFT JOIN {int_table} valid_from
               ON valid_from.{id_column} = evidence.{id_column}
              AND valid_from.key_id = ?5
             LEFT JOIN {int_table} valid_to
               ON valid_to.{id_column} = evidence.{id_column}
              AND valid_to.key_id = ?6
             JOIN property_keys evidence_key ON evidence_key.id = evidence.key_id
             JOIN property_keys workspace_key ON workspace_key.id = workspace.key_id
             WHERE evidence_key.key = 'evidence_ids_json'
               AND workspace_key.key = 'workspace_id'
               AND workspace.value = ?1
             ORDER BY evidence.{id_column} ASC"
        ))
        .with_context(|| format!("failed preparing graph {record_kind} evidence index query"))?;
    let mut insert = sqlite
        .prepare(
            "INSERT OR IGNORE INTO graph_evidence_record_index (
                workspace_id,
                evidence_id,
                record_kind,
                record_internal_id,
                logical_record_id,
                version_id,
                created_by_event_id,
                valid_from,
                valid_to
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
        )
        .context("failed preparing graph evidence record index insert")?;
    let mut rows = query
        .query((
            workspace_id,
            logical_key_id,
            version_key_id,
            event_key_id,
            valid_from_key_id,
            valid_to_key_id,
        ))
        .with_context(|| format!("failed querying graph {record_kind} evidence index rows"))?;
    while let Some(row) = rows
        .next()
        .with_context(|| format!("failed reading graph {record_kind} evidence index row"))?
    {
        let record_internal_id = row
            .get::<_, i64>(0)
            .context("read graph evidence record internal id")?;
        let evidence_ids_json: String = row.get(1).context("read graph evidence refs")?;
        let logical_record_id: String = row.get(2).unwrap_or_default();
        let version_id: String = row.get(3).unwrap_or_default();
        let created_by_event_id: String = row.get(4).unwrap_or_default();
        let valid_from: i64 = row.get(5).unwrap_or_default();
        let valid_to: i64 = row.get(6).unwrap_or_default();
        let Ok(evidence_ids) = serde_json::from_str::<Vec<String>>(&evidence_ids_json) else {
            continue;
        };
        for evidence_id in evidence_ids {
            insert
                .execute((
                    workspace_id,
                    evidence_id.as_str(),
                    record_kind,
                    record_internal_id,
                    logical_record_id.as_str(),
                    version_id.as_str(),
                    created_by_event_id.as_str(),
                    valid_from,
                    valid_to,
                ))
                .with_context(|| {
                    format!("failed indexing graph {record_kind} evidence ref {evidence_id}")
                })?;
        }
    }
    Ok(())
}

fn graph_checkpoint_id(snapshot: &BrainRepoSnapshot, checksum: &str) -> String {
    let identity = snapshot
        .events
        .last()
        .map(|event| event.event_id.clone())
        .unwrap_or_else(|| format!("snapshot-{}", uuid::Uuid::now_v7().as_simple()));
    let checksum_prefix = checksum.get(..16).unwrap_or(checksum);
    format!(
        "graph-checkpoint-{}-{}-{}",
        snapshot.workspace_id, identity, checksum_prefix
    )
}

fn graph_checkpoint_checksum(
    snapshot: &BrainRepoSnapshot,
    report: &KnowledgeGraphPersistReport,
) -> Result<String> {
    let payload = serde_json::json!({
        "workspace_id": snapshot.workspace_id,
        "generated_at": snapshot.generated_at,
        "node_count": report.node_count,
        "relation_count": report.relation_count,
        "nodes": snapshot.nodes,
        "relations": snapshot.relations,
        "wiki_pages": snapshot.wiki_pages,
        "entities": snapshot.entities,
        "claims": snapshot.claims,
        "memories": snapshot.memories,
        "evidence": snapshot.evidence,
        "events": snapshot.events,
    });
    let encoded = serde_json::to_vec(&payload).context("failed encoding checkpoint payload")?;
    Ok(hex_digest(
        ring::digest::digest(&ring::digest::SHA256, &encoded).as_ref(),
    ))
}

fn hex_digest(bytes: &[u8]) -> String {
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        output.push_str(&format!("{byte:02x}"));
    }
    output
}

fn graphqlite_extension_version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

fn mark_import_jobs_graph_ready_in_transaction(
    graph: &Graph,
    snapshot: &BrainRepoSnapshot,
) -> Result<()> {
    let mut source_ids = snapshot
        .sources
        .iter()
        .map(|source| source.source_id.clone())
        .collect::<BTreeSet<_>>();
    source_ids.extend(
        snapshot
            .evidence
            .iter()
            .filter_map(|evidence| evidence.source_id.clone()),
    );

    let sqlite = graph.connection().sqlite_connection();
    for source_id in source_ids {
        sqlite
            .execute(
                "UPDATE import_jobs
             SET graph_ready = 1,
                 status = 'context_ready',
                 graph_status = 'ready',
                 graph_error_category = '',
                 graph_error_message_redacted = '',
                 graph_retryable = 0,
                 graph_next_retry_at = NULL,
                 manual_retry_available = 0,
                 updated_at = ?3
             WHERE workspace_id = ?1
               AND source_id = ?2
               AND citation_ready = 1",
                (
                    snapshot.workspace_id.as_str(),
                    source_id.as_str(),
                    snapshot.generated_at as i64,
                ),
            )
            .with_context(|| format!("failed marking import job graph ready for {source_id}"))?;
    }

    Ok(())
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct GraphRecordMetadata {
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
pub(super) fn graph_node_version_identity(
    workspace_id: &str,
    logical_id: &str,
    created_by_event_id: &str,
) -> GraphRecordVersionIdentity {
    GraphRecordVersionIdentity {
        logical_id: logical_id.into(),
        version_id: graph_record_version_id("node", workspace_id, logical_id, created_by_event_id),
        created_by_event_id: created_by_event_id.into(),
    }
}

#[allow(dead_code)]
pub(super) fn graph_relation_version_identity(
    workspace_id: &str,
    logical_id: &str,
    created_by_event_id: &str,
) -> GraphRecordVersionIdentity {
    GraphRecordVersionIdentity {
        logical_id: logical_id.into(),
        version_id: graph_record_version_id(
            "relation",
            workspace_id,
            logical_id,
            created_by_event_id,
        ),
        created_by_event_id: created_by_event_id.into(),
    }
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
    format!("hyprduck-{record_kind}-version-{}", &digest[..32])
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

fn resolve_live_graph_node_version_id(
    graph: &Graph,
    workspace_id: &str,
    logical_id: &str,
) -> Result<Option<String>> {
    let sqlite = graph.connection().sqlite_connection();
    let workspace_key_id = ensure_graphlite_property_key_id(graph, "workspace_id")?;
    let logical_key_id = ensure_graphlite_property_key_id(graph, "logical_id")?;
    let id_key_id = ensure_graphlite_property_key_id(graph, "id")?;
    let valid_to_key_id = ensure_graphlite_property_key_id(graph, "valid_to")?;
    let mut statement = sqlite
        .prepare(
            "SELECT id.value
             FROM node_props_text id
             JOIN node_props_text workspace
               ON workspace.node_id = id.node_id
              AND workspace.key_id = ?1
             LEFT JOIN node_props_text logical
               ON logical.node_id = id.node_id
              AND logical.key_id = ?2
             LEFT JOIN node_props_int valid_to
               ON valid_to.node_id = id.node_id
              AND valid_to.key_id = ?4
             WHERE id.key_id = ?3
               AND workspace.value = ?5
               AND COALESCE(NULLIF(logical.value, ''), id.value) = ?6
               AND COALESCE(valid_to.value, 0) <= 0
             ORDER BY CASE WHEN logical.value IS NULL OR logical.value = '' THEN 0 ELSE 1 END DESC,
                      id.node_id DESC
             LIMIT 1",
        )
        .context("failed preparing live GraphQLite node version lookup")?;
    let mut rows = statement
        .query((
            workspace_key_id,
            logical_key_id,
            id_key_id,
            valid_to_key_id,
            workspace_id,
            logical_id,
        ))
        .context("failed querying live GraphQLite node version")?;
    Ok(rows
        .next()
        .context("failed reading live GraphQLite node version")?
        .map(|row| row.get(0).context("read live GraphQLite node version id"))
        .transpose()?)
}

fn graph_endpoint_version_id(
    graph: &Graph,
    workspace_id: &str,
    current_versions: &BTreeMap<String, String>,
    logical_id: &str,
) -> Result<Option<String>> {
    if let Some(version_id) = current_versions.get(logical_id) {
        return Ok(Some(version_id.clone()));
    }
    resolve_live_graph_node_version_id(graph, workspace_id, logical_id)
}

fn invalidate_live_graph_node_versions(
    graph: &Graph,
    workspace_id: &str,
    logical_id: &str,
    keep_version_id: Option<&str>,
    valid_to: i64,
    superseded_by: &str,
) -> Result<()> {
    let sqlite = graph.connection().sqlite_connection();
    let workspace_key_id = ensure_graphlite_property_key_id(graph, "workspace_id")?;
    let logical_key_id = ensure_graphlite_property_key_id(graph, "logical_id")?;
    let id_key_id = ensure_graphlite_property_key_id(graph, "id")?;
    let valid_to_key_id = ensure_graphlite_property_key_id(graph, "valid_to")?;
    let keep_version_id = keep_version_id.unwrap_or_default();
    let mut statement = sqlite
        .prepare(
            "SELECT id.node_id, id.value
             FROM node_props_text id
             JOIN node_props_text workspace
               ON workspace.node_id = id.node_id
              AND workspace.key_id = ?1
             LEFT JOIN node_props_text logical
               ON logical.node_id = id.node_id
              AND logical.key_id = ?2
             LEFT JOIN node_props_int current_valid_to
               ON current_valid_to.node_id = id.node_id
              AND current_valid_to.key_id = ?4
             WHERE id.key_id = ?3
               AND workspace.value = ?5
               AND COALESCE(NULLIF(logical.value, ''), id.value) = ?6
               AND (?7 = '' OR id.value != ?7)
               AND COALESCE(current_valid_to.value, 0) <= 0",
        )
        .context("failed preparing live GraphQLite node invalidation query")?;
    let mut rows = statement
        .query((
            workspace_key_id,
            logical_key_id,
            id_key_id,
            valid_to_key_id,
            workspace_id,
            logical_id,
            keep_version_id,
        ))
        .context("failed querying live GraphQLite node versions")?;
    let mut node_ids = Vec::new();
    while let Some(row) = rows
        .next()
        .context("failed reading live GraphQLite node version")?
    {
        node_ids.push(row.get::<_, i64>(0).context("read GraphQLite node id")?);
    }
    drop(rows);
    drop(statement);
    for node_id in node_ids {
        set_graphlite_int_property(
            graph,
            "node_props_int",
            "node_id",
            node_id,
            "valid_to",
            valid_to,
        )?;
        set_graphlite_text_property(
            graph,
            "node_props_text",
            "node_id",
            node_id,
            "superseded_by",
            superseded_by,
        )?;
    }
    Ok(())
}

fn invalidate_live_graph_node_versions_not_in(
    graph: &Graph,
    workspace_id: &str,
    keep_logical_ids: &BTreeSet<String>,
    valid_to: i64,
    superseded_by: &str,
) -> Result<()> {
    for logical_id in live_graph_node_logical_ids(graph, workspace_id)? {
        if !keep_logical_ids.contains(&logical_id) {
            invalidate_live_graph_node_versions(
                graph,
                workspace_id,
                &logical_id,
                None,
                valid_to,
                superseded_by,
            )?;
        }
    }
    Ok(())
}

fn live_graph_node_logical_ids(graph: &Graph, workspace_id: &str) -> Result<BTreeSet<String>> {
    let sqlite = graph.connection().sqlite_connection();
    let workspace_key_id = ensure_graphlite_property_key_id(graph, "workspace_id")?;
    let logical_key_id = ensure_graphlite_property_key_id(graph, "logical_id")?;
    let id_key_id = ensure_graphlite_property_key_id(graph, "id")?;
    let valid_to_key_id = ensure_graphlite_property_key_id(graph, "valid_to")?;
    let mut statement = sqlite
        .prepare(
            "SELECT COALESCE(NULLIF(logical.value, ''), id.value)
             FROM node_props_text id
             JOIN node_props_text workspace
               ON workspace.node_id = id.node_id
              AND workspace.key_id = ?1
             LEFT JOIN node_props_text logical
               ON logical.node_id = id.node_id
              AND logical.key_id = ?2
             LEFT JOIN node_props_int valid_to
               ON valid_to.node_id = id.node_id
              AND valid_to.key_id = ?4
             WHERE id.key_id = ?3
               AND workspace.value = ?5
               AND COALESCE(valid_to.value, 0) <= 0",
        )
        .context("failed preparing live GraphQLite node logical id query")?;
    let mut rows = statement
        .query((
            workspace_key_id,
            logical_key_id,
            id_key_id,
            valid_to_key_id,
            workspace_id,
        ))
        .context("failed querying live GraphQLite node logical ids")?;
    let mut logical_ids = BTreeSet::new();
    while let Some(row) = rows
        .next()
        .context("failed reading live GraphQLite node logical id")?
    {
        logical_ids.insert(row.get(0).context("read GraphQLite node logical id")?);
    }
    Ok(logical_ids)
}

fn invalidate_live_graph_relation_versions(
    graph: &Graph,
    workspace_id: &str,
    logical_id: &str,
    keep_version_id: Option<&str>,
    valid_to: i64,
    superseded_by: &str,
) -> Result<()> {
    let sqlite = graph.connection().sqlite_connection();
    let workspace_key_id = ensure_graphlite_property_key_id(graph, "workspace_id")?;
    let relation_key_id = ensure_graphlite_property_key_id(graph, "relation_id")?;
    let version_key_id = ensure_graphlite_property_key_id(graph, "version_id")?;
    let valid_to_key_id = ensure_graphlite_property_key_id(graph, "valid_to")?;
    let keep_version_id = keep_version_id.unwrap_or_default();
    let mut statement = sqlite
        .prepare(
            "SELECT relation.edge_id
             FROM edge_props_text relation
             JOIN edge_props_text workspace
               ON workspace.edge_id = relation.edge_id
              AND workspace.key_id = ?1
             LEFT JOIN edge_props_text version
               ON version.edge_id = relation.edge_id
              AND version.key_id = ?3
             LEFT JOIN edge_props_int current_valid_to
               ON current_valid_to.edge_id = relation.edge_id
              AND current_valid_to.key_id = ?4
             WHERE relation.key_id = ?2
               AND workspace.value = ?5
               AND relation.value = ?6
               AND (?7 = '' OR version.value != ?7)
               AND COALESCE(current_valid_to.value, 0) <= 0",
        )
        .context("failed preparing live GraphQLite relation invalidation query")?;
    let mut rows = statement
        .query((
            workspace_key_id,
            relation_key_id,
            version_key_id,
            valid_to_key_id,
            workspace_id,
            logical_id,
            keep_version_id,
        ))
        .context("failed querying live GraphQLite relation versions")?;
    let mut edge_ids = Vec::new();
    while let Some(row) = rows
        .next()
        .context("failed reading live GraphQLite relation version")?
    {
        edge_ids.push(row.get::<_, i64>(0).context("read GraphQLite edge id")?);
    }
    drop(rows);
    drop(statement);
    for edge_id in edge_ids {
        set_graphlite_int_property(
            graph,
            "edge_props_int",
            "edge_id",
            edge_id,
            "valid_to",
            valid_to,
        )?;
        set_graphlite_text_property(
            graph,
            "edge_props_text",
            "edge_id",
            edge_id,
            "superseded_by",
            superseded_by,
        )?;
    }
    Ok(())
}

fn invalidate_live_graph_relation_versions_not_in(
    graph: &Graph,
    workspace_id: &str,
    keep_logical_ids: &BTreeSet<String>,
    valid_to: i64,
    superseded_by: &str,
) -> Result<()> {
    for logical_id in live_graph_relation_logical_ids(graph, workspace_id)? {
        if !keep_logical_ids.contains(&logical_id) {
            invalidate_live_graph_relation_versions(
                graph,
                workspace_id,
                &logical_id,
                None,
                valid_to,
                superseded_by,
            )?;
        }
    }
    Ok(())
}

fn live_graph_relation_logical_ids(graph: &Graph, workspace_id: &str) -> Result<BTreeSet<String>> {
    let sqlite = graph.connection().sqlite_connection();
    let workspace_key_id = ensure_graphlite_property_key_id(graph, "workspace_id")?;
    let relation_key_id = ensure_graphlite_property_key_id(graph, "relation_id")?;
    let valid_to_key_id = ensure_graphlite_property_key_id(graph, "valid_to")?;
    let mut statement = sqlite
        .prepare(
            "SELECT relation.value
             FROM edge_props_text relation
             JOIN edge_props_text workspace
               ON workspace.edge_id = relation.edge_id
              AND workspace.key_id = ?1
             LEFT JOIN edge_props_int valid_to
               ON valid_to.edge_id = relation.edge_id
              AND valid_to.key_id = ?3
             WHERE relation.key_id = ?2
               AND workspace.value = ?4
               AND COALESCE(valid_to.value, 0) <= 0",
        )
        .context("failed preparing live GraphQLite relation logical id query")?;
    let mut rows = statement
        .query((
            workspace_key_id,
            relation_key_id,
            valid_to_key_id,
            workspace_id,
        ))
        .context("failed querying live GraphQLite relation logical ids")?;
    let mut logical_ids = BTreeSet::new();
    while let Some(row) = rows
        .next()
        .context("failed reading live GraphQLite relation logical id")?
    {
        logical_ids.insert(row.get(0).context("read GraphQLite relation logical id")?);
    }
    Ok(logical_ids)
}

fn ensure_graphlite_property_key_id(graph: &Graph, key: &str) -> Result<i64> {
    let sqlite = graph.connection().sqlite_connection();
    sqlite
        .execute(
            "INSERT OR IGNORE INTO property_keys (key) VALUES (?1)",
            [key],
        )
        .with_context(|| format!("failed ensuring GraphQLite property key {key}"))?;
    sqlite
        .query_row(
            "SELECT id FROM property_keys WHERE key = ?1",
            [key],
            |row| row.get(0),
        )
        .with_context(|| format!("failed reading GraphQLite property key {key}"))
}

fn set_graphlite_int_property(
    graph: &Graph,
    table: &str,
    id_column: &str,
    record_id: i64,
    key: &str,
    value: i64,
) -> Result<()> {
    let sqlite = graph.connection().sqlite_connection();
    let key_id = ensure_graphlite_property_key_id(graph, key)?;
    sqlite
        .execute(
            &format!(
                "INSERT OR REPLACE INTO {table} ({id_column}, key_id, value) VALUES (?1, ?2, ?3)"
            ),
            (record_id, key_id, value),
        )
        .with_context(|| format!("failed setting GraphQLite int property {key}"))
        .map(|_| ())
}

fn set_graphlite_text_property(
    graph: &Graph,
    table: &str,
    id_column: &str,
    record_id: i64,
    key: &str,
    value: &str,
) -> Result<()> {
    let sqlite = graph.connection().sqlite_connection();
    let key_id = ensure_graphlite_property_key_id(graph, key)?;
    sqlite
        .execute(
            &format!(
                "INSERT OR REPLACE INTO {table} ({id_column}, key_id, value) VALUES (?1, ?2, ?3)"
            ),
            (record_id, key_id, value),
        )
        .with_context(|| format!("failed setting GraphQLite text property {key}"))
        .map(|_| ())
}

fn node_graph_metadata(
    snapshot: &BrainRepoSnapshot,
    node: &BrainNodeRecord,
) -> GraphRecordMetadata {
    let mut source_ids = node.source_ids.clone();
    if node.kind == BrainNodeKind::Source && source_ids.is_empty() {
        if let Some(source_id) = node.node_id.strip_prefix("source:") {
            source_ids.push(source_id.to_string());
        }
    }
    source_ids.sort();
    source_ids.dedup();

    let status = snapshot
        .claims
        .iter()
        .find(|claim| claim.claim_id == node.node_id)
        .map(|claim| claim.status.clone())
        .unwrap_or_else(|| "active".into());
    let producer_run_ids = producer_run_ids_for_refs(snapshot, &node.evidence_ids, &source_ids);

    GraphRecordMetadata {
        source_ids,
        producer_run_ids,
        status,
    }
}

fn relation_graph_metadata(
    snapshot: &BrainRepoSnapshot,
    relation: &BrainRelationRecord,
) -> GraphRecordMetadata {
    let evidence_source_ids = snapshot
        .evidence
        .iter()
        .filter(|evidence| relation.evidence_ids.contains(&evidence.id))
        .filter_map(|evidence| evidence.source_id.clone());
    let endpoint_source_ids = [&relation.source_node_id, &relation.target_node_id]
        .into_iter()
        .filter_map(|node_id| node_id.strip_prefix("source:").map(ToOwned::to_owned));
    let mut source_ids = evidence_source_ids
        .chain(endpoint_source_ids)
        .collect::<Vec<_>>();
    source_ids.sort();
    source_ids.dedup();
    let producer_run_ids = producer_run_ids_for_refs(snapshot, &relation.evidence_ids, &source_ids);

    GraphRecordMetadata {
        source_ids,
        producer_run_ids,
        status: "active".into(),
    }
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

fn persist_snapshot_sources_in_transaction(
    graph: &Graph,
    snapshot: &BrainRepoSnapshot,
) -> Result<()> {
    let sqlite = graph.connection().sqlite_connection();
    for source in &snapshot.sources {
        let original_path_redacted = redact_path_for_agent(&source.original_path);
        let source_path_redacted = redact_path_for_agent(&source.source_path);
        let markdown_path_redacted = redact_path_for_agent(&source.markdown_path);
        sqlite
            .execute(
                "INSERT INTO sources (
                    source_id,
                    workspace_id,
                    project_id,
                    title,
                    original_path,
                    source_path,
                    markdown_path,
                    original_path_redacted,
                    source_path_redacted,
                    markdown_path_redacted,
                    format,
                    status,
                    page_count,
                    success_count,
                    failed_count,
                    updated_at
                ) VALUES (?1, ?2, '', ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?12, 0, ?13)
                ON CONFLICT(source_id) DO UPDATE SET
                    workspace_id=excluded.workspace_id,
                    title=excluded.title,
                    original_path=excluded.original_path,
                    source_path=excluded.source_path,
                    markdown_path=excluded.markdown_path,
                    original_path_redacted=excluded.original_path_redacted,
                    source_path_redacted=excluded.source_path_redacted,
                    markdown_path_redacted=excluded.markdown_path_redacted,
                    format=excluded.format,
                    status=excluded.status,
                    page_count=excluded.page_count,
                    updated_at=excluded.updated_at",
                (
                    source.source_id.as_str(),
                    snapshot.workspace_id.as_str(),
                    source.source_id.as_str(),
                    source.original_path.as_str(),
                    source.source_path.as_str(),
                    source.markdown_path.as_str(),
                    original_path_redacted.as_str(),
                    source_path_redacted.as_str(),
                    markdown_path_redacted.as_str(),
                    source.format.as_str(),
                    source.status.as_str(),
                    source.page_count as i64,
                    source.updated_at as i64,
                ),
            )
            .with_context(|| format!("failed upserting source row {}", source.source_id))?;
    }
    for evidence in &snapshot.evidence {
        let Some(source_id) = evidence.source_id.as_deref() else {
            continue;
        };
        sqlite
            .execute(
                "INSERT OR IGNORE INTO sources (
                    source_id,
                    workspace_id,
                    project_id,
                    title,
                    original_path,
                    source_path,
                    markdown_path,
                    format,
                    status,
                    page_count,
                    success_count,
                    failed_count,
                    updated_at
                ) VALUES (?1, ?2, '', ?1, '', '', '', 'unknown', 'unknown', 0, 0, 0, ?3)",
                (
                    source_id,
                    snapshot.workspace_id.as_str(),
                    snapshot.generated_at as i64,
                ),
            )
            .with_context(|| format!("failed upserting evidence source row {source_id}"))?;
    }
    Ok(())
}

#[derive(Debug, Default)]
struct SourcePageSnapshotRow {
    page_label: String,
    markdown_path_redacted: String,
    image_path_redacted: String,
    plain_text: String,
    parse_warnings_json: String,
    snippets: Vec<String>,
}

fn persist_source_pages_snapshot_in_transaction(
    graph: &Graph,
    snapshot: &BrainRepoSnapshot,
) -> Result<()> {
    let sqlite = graph.connection().sqlite_connection();
    let source_ids = snapshot
        .sources
        .iter()
        .map(|source| source.source_id.clone())
        .chain(
            snapshot
                .evidence
                .iter()
                .filter_map(|evidence| evidence.source_id.clone()),
        )
        .collect::<BTreeSet<_>>();
    let mut pages = BTreeMap::<(String, usize), SourcePageSnapshotRow>::new();
    for source_id in &source_ids {
        let mut statement = sqlite
            .prepare(
                "SELECT page_index,
                        page_label,
                        COALESCE(markdown_path_redacted, ''),
                        COALESCE(image_path_redacted, ''),
                        plain_text,
                        parse_warnings_json
                 FROM source_pages
                 WHERE source_id = ?1",
            )
            .with_context(|| {
                format!("failed preparing source page preservation for {source_id}")
            })?;
        let mut rows = statement
            .query([source_id.as_str()])
            .with_context(|| format!("failed reading source pages for {source_id}"))?;
        while let Some(row) = rows
            .next()
            .with_context(|| format!("failed reading source page row for {source_id}"))?
        {
            let page_index = row
                .get::<_, i64>(0)
                .context("read preserved source page index")?;
            pages.insert(
                (source_id.clone(), page_index.max(0) as usize),
                SourcePageSnapshotRow {
                    page_label: row.get(1).context("read preserved source page label")?,
                    markdown_path_redacted: row
                        .get(2)
                        .context("read preserved source page markdown path")?,
                    image_path_redacted: row
                        .get(3)
                        .context("read preserved source page image path")?,
                    plain_text: row
                        .get(4)
                        .context("read preserved source page plain text")?,
                    parse_warnings_json: row
                        .get(5)
                        .context("read preserved source page warnings")?,
                    snippets: Vec::new(),
                },
            );
        }
    }
    for source_id in &source_ids {
        sqlite
            .execute("DELETE FROM source_pages WHERE source_id = ?1", [source_id])
            .with_context(|| format!("failed clearing source pages for {source_id}"))?;
        sqlite
            .execute(
                "DELETE FROM source_page_fts WHERE source_id = ?1",
                [source_id],
            )
            .with_context(|| format!("failed clearing source page FTS for {source_id}"))?;
    }

    for evidence in &snapshot.evidence {
        let (Some(source_id), Some(page_index)) =
            (evidence.source_id.as_ref(), evidence.page_index)
        else {
            continue;
        };
        let row = pages
            .entry((source_id.clone(), page_index))
            .or_insert_with(|| SourcePageSnapshotRow {
                page_label: evidence.page_label.clone(),
                markdown_path_redacted: evidence
                    .markdown_path
                    .as_deref()
                    .map(redact_path_for_agent)
                    .unwrap_or_default(),
                image_path_redacted: evidence
                    .image_path
                    .as_deref()
                    .map(redact_path_for_agent)
                    .unwrap_or_default(),
                plain_text: String::new(),
                parse_warnings_json: "[]".into(),
                snippets: Vec::new(),
            });
        if row.page_label.is_empty() {
            row.page_label = evidence.page_label.clone();
        }
        if row.markdown_path_redacted.is_empty() {
            row.markdown_path_redacted = evidence
                .markdown_path
                .as_deref()
                .map(redact_path_for_agent)
                .unwrap_or_default();
        }
        if row.image_path_redacted.is_empty() {
            row.image_path_redacted = evidence
                .image_path
                .as_deref()
                .map(redact_path_for_agent)
                .unwrap_or_default();
        }
        if !evidence.snippet.trim().is_empty() {
            row.snippets.push(evidence.snippet.clone());
        }
    }

    for ((source_id, page_index), row) in pages {
        let plain_text = if !row.plain_text.trim().is_empty() {
            row.plain_text
        } else if row.snippets.is_empty() {
            String::new()
        } else {
            row.snippets.join("\n\n")
        };
        sqlite
            .execute(
                "INSERT INTO source_pages (
                    source_id,
                    page_index,
                    page_label,
                    markdown_path_redacted,
                    image_path_redacted,
                    plain_text,
                    parse_warnings_json
                ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                (
                    source_id.as_str(),
                    page_index as i64,
                    row.page_label.as_str(),
                    row.markdown_path_redacted.as_str(),
                    row.image_path_redacted.as_str(),
                    plain_text.as_str(),
                    row.parse_warnings_json.as_str(),
                ),
            )
            .with_context(|| {
                format!("failed inserting migrated source page {source_id}:{page_index}")
            })?;
        if !plain_text.trim().is_empty() {
            sqlite
                .execute(
                    "INSERT INTO source_page_fts (source_id, page_index, page_label, text)
                 VALUES (?1, ?2, ?3, ?4)",
                    (
                        source_id.as_str(),
                        page_index as i64,
                        row.page_label.as_str(),
                        plain_text.as_str(),
                    ),
                )
                .with_context(|| format!("failed indexing source page {source_id}:{page_index}"))?;
        }
    }

    Ok(())
}

fn persist_evidence_snapshot_in_transaction(
    graph: &Graph,
    snapshot: &BrainRepoSnapshot,
) -> Result<()> {
    let sqlite = graph.connection().sqlite_connection();
    sqlite
        .execute(
            "DELETE FROM evidence_fts WHERE evidence_id IN (SELECT evidence_id FROM evidence_items WHERE workspace_id = ?1)",
            [snapshot.workspace_id.as_str()],
        )
        .context("failed clearing evidence FTS rows")?;
    sqlite
        .execute(
            "DELETE FROM evidence_items WHERE workspace_id = ?1",
            [snapshot.workspace_id.as_str()],
        )
        .context("failed clearing relational evidence rows")?;

    for evidence in &snapshot.evidence {
        let source_id = evidence.source_id.as_deref().unwrap_or_default();
        let page_index = evidence
            .page_index
            .map(|value| value.to_string())
            .unwrap_or_default();
        let source_path_redacted =
            redact_path_for_agent(evidence.source_path.as_deref().unwrap_or_default());
        let markdown_path_redacted =
            redact_path_for_agent(evidence.markdown_path.as_deref().unwrap_or_default());
        let image_path_redacted =
            redact_path_for_agent(evidence.image_path.as_deref().unwrap_or_default());
        sqlite
            .execute(
                "INSERT INTO evidence_items (
                    evidence_id,
                    workspace_id,
                    source_id,
                    page_index,
                    page_label,
                    evidence_type,
                    snippet,
                    source_path_redacted,
                    markdown_path_redacted,
                    image_path_redacted,
                    provenance,
                    status
                ) VALUES (?1, ?2, ?3, NULLIF(?4, ''), ?5, 'text_evidence', ?6, ?7, ?8, ?9, ?10, 'active')",
                (
                    evidence.id.as_str(),
                    snapshot.workspace_id.as_str(),
                    source_id,
                    page_index.as_str(),
                    evidence.page_label.as_str(),
                    evidence.snippet.as_str(),
                    source_path_redacted.as_str(),
                    markdown_path_redacted.as_str(),
                    image_path_redacted.as_str(),
                    evidence.provenance.as_deref().unwrap_or_default(),
                ),
            )
            .with_context(|| format!("failed inserting evidence row {}", evidence.id))?;
        sqlite
            .execute(
                "INSERT INTO evidence_fts (evidence_id, source_id, evidence_type, text)
                 VALUES (?1, ?2, 'text_evidence', ?3)",
                (evidence.id.as_str(), source_id, evidence.snippet.as_str()),
            )
            .with_context(|| format!("failed indexing evidence row {}", evidence.id))?;
    }

    Ok(())
}

fn persist_wiki_pages_snapshot_in_transaction(
    graph: &Graph,
    snapshot: &BrainRepoSnapshot,
) -> Result<()> {
    let sqlite = graph.connection().sqlite_connection();
    let created_by_event_id = graph_snapshot_created_by_event_id(snapshot);
    let mut current_page_ids = BTreeSet::new();
    for page in &snapshot.wiki_pages {
        let stored_page_id = stored_wiki_page_id(graph, &snapshot.workspace_id, page)?;
        current_page_ids.insert(stored_page_id.clone());
        let previous = load_stored_wiki_page_state(graph, &snapshot.workspace_id, &stored_page_id)?;
        let same_event_revision = previous
            .as_ref()
            .is_some_and(|state| state.current_revision_event_id == created_by_event_id);
        let revision = if same_event_revision {
            previous.as_ref().map(|state| state.revision).unwrap_or(1)
        } else {
            previous
                .as_ref()
                .map(|state| state.revision.saturating_add(1))
                .unwrap_or(1)
        };
        if let Some(previous) = previous.as_ref().filter(|_| !same_event_revision) {
            close_wiki_revision(
                graph,
                &snapshot.workspace_id,
                &stored_page_id,
                previous.revision,
                snapshot.generated_at as i64,
                &created_by_event_id,
            )?;
        }
        let evidence_refs_json = serde_json::to_string(&page.evidence_refs)
            .context("failed encoding wiki evidence refs")?;
        let source_refs_json =
            serde_json::to_string(&page.source_refs).context("failed encoding wiki source refs")?;
        let node_refs_json =
            serde_json::to_string(&page.node_refs).context("failed encoding wiki node refs")?;
        let relation_refs_json = "[]";
        let predecessor_revision = (revision > 1).then_some(revision - 1);
        let version_id = wiki_revision_version_id(
            &snapshot.workspace_id,
            &stored_page_id,
            revision,
            &created_by_event_id,
        );
        let valid_from = page.updated_at.max(
            (page.updated_at == 0)
                .then_some(snapshot.generated_at)
                .unwrap_or(0),
        ) as i64;
        let approval_status = "materialized";
        let diff_json = "{}";
        sqlite
            .execute(
                "INSERT INTO wiki_pages (
                    wiki_page_id,
                    workspace_id,
                    path,
                    title,
                    body,
                    approval_status,
                    evidence_refs_json,
                    revision,
                    current_revision_event_id,
                    current_revision_version_id,
                    valid_from,
                    valid_to,
                    superseded_by,
                    updated_at
                ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, 0, '', ?12)
                ON CONFLICT(wiki_page_id) DO UPDATE SET
                    workspace_id=excluded.workspace_id,
                    path=excluded.path,
                    title=excluded.title,
                    body=excluded.body,
                    approval_status=excluded.approval_status,
                    evidence_refs_json=excluded.evidence_refs_json,
                    revision=excluded.revision,
                    current_revision_event_id=excluded.current_revision_event_id,
                    current_revision_version_id=excluded.current_revision_version_id,
                    valid_from=excluded.valid_from,
                    valid_to=0,
                    superseded_by='',
                    updated_at=excluded.updated_at",
                (
                    stored_page_id.as_str(),
                    snapshot.workspace_id.as_str(),
                    page.path.as_str(),
                    page.title.as_str(),
                    page.body.as_str(),
                    approval_status,
                    evidence_refs_json.as_str(),
                    revision,
                    created_by_event_id.as_str(),
                    version_id.as_str(),
                    valid_from,
                    page.updated_at as i64,
                ),
            )
            .with_context(|| format!("failed upserting wiki page row {}", page.page_id))?;
        sqlite
            .execute(
                "INSERT INTO wiki_revisions (
                    wiki_page_id,
                    revision,
                    workspace_id,
                    title,
                    body,
                    approval_status,
                    evidence_refs_json,
                    source_refs_json,
                    node_refs_json,
                    relation_refs_json,
                    diff_json,
                    version_id,
                    created_by_event_id,
                    predecessor_revision,
                    superseded_by_event_id,
                    valid_from,
                    valid_to,
                    updated_at
                ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, '', ?15, 0, ?16)
                ON CONFLICT(wiki_page_id, revision) DO UPDATE SET
                    workspace_id=excluded.workspace_id,
                    title=excluded.title,
                    body=excluded.body,
                    approval_status=excluded.approval_status,
                    evidence_refs_json=excluded.evidence_refs_json,
                    source_refs_json=excluded.source_refs_json,
                    node_refs_json=excluded.node_refs_json,
                    relation_refs_json=excluded.relation_refs_json,
                    diff_json=excluded.diff_json,
                    version_id=excluded.version_id,
                    created_by_event_id=excluded.created_by_event_id,
                    predecessor_revision=excluded.predecessor_revision,
                    valid_from=excluded.valid_from,
                    valid_to=0,
                    updated_at=excluded.updated_at",
                (
                    stored_page_id.as_str(),
                    revision,
                    snapshot.workspace_id.as_str(),
                    page.title.as_str(),
                    page.body.as_str(),
                    approval_status,
                    evidence_refs_json.as_str(),
                    source_refs_json.as_str(),
                    node_refs_json.as_str(),
                    relation_refs_json,
                    diff_json,
                    version_id.as_str(),
                    created_by_event_id.as_str(),
                    predecessor_revision,
                    valid_from,
                    page.updated_at as i64,
                ),
            )
            .with_context(|| format!("failed upserting wiki revision row {}", page.page_id))?;
        sqlite
            .execute(
                "DELETE FROM wiki_sections WHERE wiki_page_id = ?1 AND revision = ?2",
                (stored_page_id.as_str(), revision),
            )
            .with_context(|| format!("failed clearing wiki sections for {}", page.page_id))?;
        sqlite
            .execute(
                "DELETE FROM wiki_fts WHERE wiki_page_id = ?1 AND revision = ?2",
                (stored_page_id.as_str(), revision),
            )
            .with_context(|| format!("failed clearing wiki FTS rows for {}", page.page_id))?;
        for section in wiki_sections_from_page(page) {
            let section_evidence_refs_json = serde_json::to_string(&section.evidence_refs)
                .context("failed encoding wiki section evidence refs")?;
            sqlite
                .execute(
                    "INSERT INTO wiki_sections (
                        wiki_page_id,
                        revision,
                        section_index,
                        heading,
                        body,
                        evidence_refs_json,
                        updated_at
                    ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                    (
                        stored_page_id.as_str(),
                        revision,
                        section.index,
                        section.heading.as_str(),
                        section.body.as_str(),
                        section_evidence_refs_json.as_str(),
                        page.updated_at as i64,
                    ),
                )
                .with_context(|| format!("failed inserting wiki section row {}", page.page_id))?;
            sqlite
                .execute(
                    "INSERT INTO wiki_fts (workspace_id, wiki_page_id, revision, section_index, title, text)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                    (
                        snapshot.workspace_id.as_str(),
                        stored_page_id.as_str(),
                        revision,
                        section.index,
                        page.title.as_str(),
                        section.body.as_str(),
                    ),
                )
                .with_context(|| format!("failed indexing wiki section {}", page.page_id))?;
        }
    }
    invalidate_absent_wiki_pages(
        graph,
        &snapshot.workspace_id,
        &current_page_ids,
        snapshot.generated_at as i64,
        &created_by_event_id,
    )?;
    Ok(())
}

struct StoredWikiPageState {
    revision: i64,
    current_revision_event_id: String,
}

fn stored_wiki_page_id(graph: &Graph, workspace_id: &str, page: &WikiPage) -> Result<String> {
    let sqlite = graph.connection().sqlite_connection();
    let mut statement = sqlite
        .prepare("SELECT workspace_id FROM wiki_pages WHERE wiki_page_id = ?1 LIMIT 1")
        .context("failed preparing wiki page id scope query")?;
    let mut rows = statement
        .query([page.page_id.as_str()])
        .context("failed checking existing wiki page id scope")?;
    let existing_workspace = rows
        .next()
        .context("failed reading existing wiki page id scope")?
        .map(|row| row.get::<_, String>(0))
        .transpose()
        .context("failed decoding existing wiki page id scope")?;
    if existing_workspace
        .as_deref()
        .is_some_and(|existing| existing != workspace_id)
    {
        Ok(format!("{}:{}", workspace_id, page.page_id))
    } else {
        Ok(page.page_id.clone())
    }
}

fn load_stored_wiki_page_state(
    graph: &Graph,
    workspace_id: &str,
    wiki_page_id: &str,
) -> Result<Option<StoredWikiPageState>> {
    let sqlite = graph.connection().sqlite_connection();
    let mut statement = sqlite
        .prepare(
            "SELECT revision, current_revision_event_id
             FROM wiki_pages
             WHERE workspace_id = ?1 AND wiki_page_id = ?2
             LIMIT 1",
        )
        .context("failed preparing wiki page state query")?;
    let mut rows = statement
        .query((workspace_id, wiki_page_id))
        .context("failed querying wiki page state")?;
    let Some(row) = rows.next().context("failed reading wiki page state")? else {
        return Ok(None);
    };
    Ok(Some(StoredWikiPageState {
        revision: row.get(0).context("read wiki revision")?,
        current_revision_event_id: row.get(1).context("read wiki current event")?,
    }))
}

fn close_wiki_revision(
    graph: &Graph,
    workspace_id: &str,
    wiki_page_id: &str,
    revision: i64,
    valid_to: i64,
    superseded_by_event_id: &str,
) -> Result<()> {
    let sqlite = graph.connection().sqlite_connection();
    sqlite
        .execute(
            "UPDATE wiki_revisions
             SET valid_to = ?4,
                 superseded_by_event_id = ?5
             WHERE workspace_id = ?1
               AND wiki_page_id = ?2
               AND revision = ?3
               AND valid_to = 0",
            (
                workspace_id,
                wiki_page_id,
                revision,
                valid_to,
                superseded_by_event_id,
            ),
        )
        .with_context(|| format!("failed closing wiki revision {wiki_page_id}:{revision}"))?;
    Ok(())
}

fn invalidate_absent_wiki_pages(
    graph: &Graph,
    workspace_id: &str,
    current_page_ids: &BTreeSet<String>,
    valid_to: i64,
    superseded_by_event_id: &str,
) -> Result<()> {
    let sqlite = graph.connection().sqlite_connection();
    let mut statement = sqlite
        .prepare(
            "SELECT wiki_page_id, revision
             FROM wiki_pages
             WHERE workspace_id = ?1 AND valid_to = 0",
        )
        .context("failed preparing live wiki page query")?;
    let mut rows = statement
        .query([workspace_id])
        .context("failed querying live wiki pages")?;
    let mut stale_pages = Vec::new();
    while let Some(row) = rows.next().context("failed reading live wiki page")? {
        let wiki_page_id: String = row.get(0).context("read live wiki page id")?;
        let revision: i64 = row.get(1).context("read live wiki page revision")?;
        if !current_page_ids.contains(&wiki_page_id) {
            stale_pages.push((wiki_page_id, revision));
        }
    }
    drop(rows);
    drop(statement);
    for (wiki_page_id, revision) in stale_pages {
        sqlite
            .execute(
                "UPDATE wiki_pages
                 SET valid_to = ?3,
                     superseded_by = ?4
                 WHERE workspace_id = ?1
                   AND wiki_page_id = ?2
                   AND valid_to = 0",
                (
                    workspace_id,
                    wiki_page_id.as_str(),
                    valid_to,
                    superseded_by_event_id,
                ),
            )
            .with_context(|| format!("failed invalidating wiki page {wiki_page_id}"))?;
        close_wiki_revision(
            graph,
            workspace_id,
            &wiki_page_id,
            revision,
            valid_to,
            superseded_by_event_id,
        )?;
    }
    Ok(())
}

fn wiki_revision_version_id(
    workspace_id: &str,
    wiki_page_id: &str,
    revision: i64,
    created_by_event_id: &str,
) -> String {
    graph_record_version_id(
        "wiki",
        workspace_id,
        &format!("{wiki_page_id}:{revision}"),
        created_by_event_id,
    )
}

struct WikiSectionRow {
    index: i64,
    heading: String,
    body: String,
    evidence_refs: Vec<String>,
}

fn wiki_sections_from_page(page: &WikiPage) -> Vec<WikiSectionRow> {
    let mut sections = Vec::new();
    let mut current_heading = page.title.clone();
    let mut current_body = String::new();

    for line in page.body.lines() {
        if let Some(heading) = markdown_heading_text(line) {
            if !current_body.trim().is_empty() || !sections.is_empty() {
                sections.push(WikiSectionRow {
                    index: sections.len() as i64,
                    heading: current_heading,
                    body: current_body.trim().to_owned(),
                    evidence_refs: page.evidence_refs.clone(),
                });
                current_body.clear();
            }
            current_heading = heading.to_owned();
        } else {
            if !current_body.is_empty() {
                current_body.push('\n');
            }
            current_body.push_str(line);
        }
    }

    if !current_body.trim().is_empty() || sections.is_empty() {
        sections.push(WikiSectionRow {
            index: sections.len() as i64,
            heading: current_heading,
            body: current_body.trim().to_owned(),
            evidence_refs: page.evidence_refs.clone(),
        });
    }

    sections
}

fn markdown_heading_text(line: &str) -> Option<&str> {
    let trimmed = line.trim_start();
    let hashes = trimmed.chars().take_while(|value| *value == '#').count();
    if !(1..=6).contains(&hashes) {
        return None;
    }
    let rest = trimmed.get(hashes..)?.trim_start();
    if rest.is_empty() {
        None
    } else {
        Some(rest)
    }
}

fn persist_brain_events_snapshot_in_transaction(
    graph: &Graph,
    snapshot: &BrainRepoSnapshot,
) -> Result<()> {
    let sqlite = graph.connection().sqlite_connection();
    for event in &snapshot.events {
        let actor_json =
            serde_json::to_string(&event.actor).context("failed encoding brain event actor")?;
        let evidence_refs_json = serde_json::to_string(&event.evidence_refs)
            .context("failed encoding brain event evidence refs")?;
        let operation_type = event
            .operation_type
            .clone()
            .unwrap_or_else(|| format!("{:?}", event.event_type).to_ascii_lowercase());
        let payload_json = if event.payload_json.trim().is_empty() {
            "{}"
        } else {
            event.payload_json.as_str()
        };

        sqlite
            .execute(
                "INSERT INTO brain_events (
                    event_id,
                    workspace_id,
                    actor_json,
                    operation_type,
                    evidence_refs_json,
                    payload_json,
                    created_at
                ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
                ON CONFLICT(event_id) DO UPDATE SET
                    workspace_id=excluded.workspace_id,
                    actor_json=excluded.actor_json,
                    operation_type=excluded.operation_type,
                    evidence_refs_json=excluded.evidence_refs_json,
                    payload_json=excluded.payload_json,
                    created_at=excluded.created_at",
                (
                    event.event_id.as_str(),
                    event.workspace_id.as_str(),
                    actor_json.as_str(),
                    operation_type.as_str(),
                    evidence_refs_json.as_str(),
                    payload_json,
                    event.created_at as i64,
                ),
            )
            .with_context(|| format!("failed inserting brain event row {}", event.event_id))?;
    }
    Ok(())
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

fn node_graph_properties(
    workspace_id: &str,
    node: &BrainNodeRecord,
    metadata: &GraphRecordMetadata,
    identity: &GraphRecordVersionIdentity,
) -> Vec<(String, PropertyValue)> {
    vec![
        (
            "workspace_id".into(),
            PropertyValue::Text(workspace_id.into()),
        ),
        (
            "logical_id".into(),
            PropertyValue::Text(identity.logical_id.clone()),
        ),
        (
            "version_id".into(),
            PropertyValue::Text(identity.version_id.clone()),
        ),
        (
            "created_by_event_id".into(),
            PropertyValue::Text(identity.created_by_event_id.clone()),
        ),
        (
            "kind".into(),
            PropertyValue::Text(brain_node_kind_slug(node.kind).into()),
        ),
        ("label".into(), PropertyValue::Text(node.label.clone())),
        (
            "scope".into(),
            PropertyValue::Text(brain_scope_slug(node.scope).into()),
        ),
        (
            "aliases_json".into(),
            PropertyValue::Text(
                serde_json::to_string(&node.aliases).unwrap_or_else(|_| "[]".into()),
            ),
        ),
        (
            "evidence_ids_json".into(),
            PropertyValue::Text(
                serde_json::to_string(&node.evidence_ids).unwrap_or_else(|_| "[]".into()),
            ),
        ),
        (
            "source_ids_json".into(),
            PropertyValue::Text(
                serde_json::to_string(&metadata.source_ids).unwrap_or_else(|_| "[]".into()),
            ),
        ),
        (
            "producer_run_id".into(),
            PropertyValue::Text(
                metadata
                    .producer_run_ids
                    .first()
                    .cloned()
                    .unwrap_or_default(),
            ),
        ),
        (
            "producer_run_ids_json".into(),
            PropertyValue::Text(
                serde_json::to_string(&metadata.producer_run_ids).unwrap_or_else(|_| "[]".into()),
            ),
        ),
        (
            "confidence".into(),
            PropertyValue::Text(
                node.confidence
                    .map(|value| value.to_string())
                    .unwrap_or_default(),
            ),
        ),
        (
            "status".into(),
            PropertyValue::Text(metadata.status.clone()),
        ),
        (
            "updated_at".into(),
            PropertyValue::Integer(node.updated_at as i64),
        ),
        (
            "valid_from".into(),
            PropertyValue::Integer(node.valid_from as i64),
        ),
        (
            "valid_to".into(),
            PropertyValue::Integer(node.valid_to.unwrap_or_default() as i64),
        ),
        (
            "superseded_by".into(),
            PropertyValue::Text(node.superseded_by.clone().unwrap_or_default()),
        ),
    ]
}

fn relation_graph_properties(
    workspace_id: &str,
    relation: &BrainRelationRecord,
    metadata: &GraphRecordMetadata,
    identity: &GraphRecordVersionIdentity,
) -> Vec<(String, PropertyValue)> {
    vec![
        (
            "workspace_id".into(),
            PropertyValue::Text(workspace_id.into()),
        ),
        (
            "logical_id".into(),
            PropertyValue::Text(identity.logical_id.clone()),
        ),
        (
            "version_id".into(),
            PropertyValue::Text(identity.version_id.clone()),
        ),
        (
            "created_by_event_id".into(),
            PropertyValue::Text(identity.created_by_event_id.clone()),
        ),
        (
            "relation_id".into(),
            PropertyValue::Text(relation.relation_id.clone()),
        ),
        (
            "source_logical_id".into(),
            PropertyValue::Text(relation.source_node_id.clone()),
        ),
        (
            "target_logical_id".into(),
            PropertyValue::Text(relation.target_node_id.clone()),
        ),
        (
            "kind".into(),
            PropertyValue::Text(brain_relation_kind_slug(relation.kind).into()),
        ),
        ("label".into(), PropertyValue::Text(relation.label.clone())),
        (
            "evidence_ids_json".into(),
            PropertyValue::Text(
                serde_json::to_string(&relation.evidence_ids).unwrap_or_else(|_| "[]".into()),
            ),
        ),
        (
            "source_ids_json".into(),
            PropertyValue::Text(
                serde_json::to_string(&metadata.source_ids).unwrap_or_else(|_| "[]".into()),
            ),
        ),
        (
            "producer_run_id".into(),
            PropertyValue::Text(
                metadata
                    .producer_run_ids
                    .first()
                    .cloned()
                    .unwrap_or_default(),
            ),
        ),
        (
            "producer_run_ids_json".into(),
            PropertyValue::Text(
                serde_json::to_string(&metadata.producer_run_ids).unwrap_or_else(|_| "[]".into()),
            ),
        ),
        (
            "confidence".into(),
            PropertyValue::Text(
                relation
                    .confidence
                    .map(|value| value.to_string())
                    .unwrap_or_default(),
            ),
        ),
        (
            "status".into(),
            PropertyValue::Text(metadata.status.clone()),
        ),
        (
            "updated_at".into(),
            PropertyValue::Integer(relation.updated_at as i64),
        ),
        (
            "valid_from".into(),
            PropertyValue::Integer(relation.valid_from as i64),
        ),
        (
            "valid_to".into(),
            PropertyValue::Integer(relation.valid_to.unwrap_or_default() as i64),
        ),
        (
            "superseded_by".into(),
            PropertyValue::Text(relation.superseded_by.clone().unwrap_or_default()),
        ),
    ]
}

fn brain_node_label(kind: BrainNodeKind) -> &'static str {
    match kind {
        BrainNodeKind::Source => "Source",
        BrainNodeKind::Memory => "Memory",
        BrainNodeKind::WikiPage => "WikiPage",
        BrainNodeKind::Person => "Person",
        BrainNodeKind::Company => "Company",
        BrainNodeKind::Project => "Project",
        BrainNodeKind::Product => "Product",
        BrainNodeKind::Team => "Team",
        BrainNodeKind::Event => "Event",
        BrainNodeKind::Decision => "Decision",
        BrainNodeKind::Task => "Task",
        BrainNodeKind::Claim => "Claim",
        BrainNodeKind::Topic => "Topic",
        BrainNodeKind::Concept => "Concept",
    }
}

fn brain_node_kind_slug(kind: BrainNodeKind) -> &'static str {
    match kind {
        BrainNodeKind::Source => "source",
        BrainNodeKind::Memory => "memory",
        BrainNodeKind::WikiPage => "wiki_page",
        BrainNodeKind::Person => "person",
        BrainNodeKind::Company => "company",
        BrainNodeKind::Project => "project",
        BrainNodeKind::Product => "product",
        BrainNodeKind::Team => "team",
        BrainNodeKind::Event => "event",
        BrainNodeKind::Decision => "decision",
        BrainNodeKind::Task => "task",
        BrainNodeKind::Claim => "claim",
        BrainNodeKind::Topic => "topic",
        BrainNodeKind::Concept => "concept",
    }
}

fn brain_scope_slug(scope: BrainScope) -> &'static str {
    match scope {
        BrainScope::Personal => "personal",
        BrainScope::Project => "project",
        BrainScope::Team => "team",
        BrainScope::Company => "company",
    }
}

fn brain_relation_type(kind: BrainRelationKind) -> &'static str {
    match kind {
        BrainRelationKind::Mentions => "MENTIONS",
        BrainRelationKind::Supports => "SUPPORTS",
        BrainRelationKind::Contradicts => "CONTRADICTS",
        BrainRelationKind::Supersedes => "SUPERSEDES",
        BrainRelationKind::SameAs => "SAME_AS",
        BrainRelationKind::WorksAt => "WORKS_AT",
        BrainRelationKind::Founded => "FOUNDED",
        BrainRelationKind::InvestedIn => "INVESTED_IN",
        BrainRelationKind::Advises => "ADVISES",
        BrainRelationKind::Attended => "ATTENDED",
        BrainRelationKind::Owns => "OWNS",
        BrainRelationKind::ResponsibleFor => "RESPONSIBLE_FOR",
        BrainRelationKind::Decided => "DECIDED",
        BrainRelationKind::Blocks => "BLOCKS",
        BrainRelationKind::DependsOn => "DEPENDS_ON",
        BrainRelationKind::SourceOf => "SOURCE_OF",
        BrainRelationKind::DerivedFrom => "DERIVED_FROM",
        BrainRelationKind::Cites => "CITES",
        BrainRelationKind::LinksTo => "LINKS_TO",
        BrainRelationKind::RelatedTo => "RELATED_TO",
    }
}

fn graph_relation_version_type(
    kind: BrainRelationKind,
    identity: &GraphRecordVersionIdentity,
) -> String {
    let suffix = identity
        .version_id
        .rsplit('-')
        .next()
        .unwrap_or(identity.version_id.as_str());
    let suffix = suffix.get(..16).unwrap_or(suffix);
    format!("{}_V_{}", brain_relation_type(kind), suffix)
}

fn brain_relation_kind_slug(kind: BrainRelationKind) -> &'static str {
    match kind {
        BrainRelationKind::Mentions => "mentions",
        BrainRelationKind::Supports => "supports",
        BrainRelationKind::Contradicts => "contradicts",
        BrainRelationKind::Supersedes => "supersedes",
        BrainRelationKind::SameAs => "same_as",
        BrainRelationKind::WorksAt => "works_at",
        BrainRelationKind::Founded => "founded",
        BrainRelationKind::InvestedIn => "invested_in",
        BrainRelationKind::Advises => "advises",
        BrainRelationKind::Attended => "attended",
        BrainRelationKind::Owns => "owns",
        BrainRelationKind::ResponsibleFor => "responsible_for",
        BrainRelationKind::Decided => "decided",
        BrainRelationKind::Blocks => "blocks",
        BrainRelationKind::DependsOn => "depends_on",
        BrainRelationKind::SourceOf => "source_of",
        BrainRelationKind::DerivedFrom => "derived_from",
        BrainRelationKind::Cites => "cites",
        BrainRelationKind::LinksTo => "links_to",
        BrainRelationKind::RelatedTo => "related_to",
    }
}
