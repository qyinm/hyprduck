use anyhow::{Context, Result};
use graphqlite::Graph;
use etyma_engine_types::BrainRepoSnapshot;
use std::collections::BTreeSet;

use super::graphlite_props::ensure_graphlite_property_key_id;
use super::{hex_digest, KnowledgeGraphPersistReport, GRAPHQLITE_SCHEMA_VERSION};

pub(super) fn persist_graph_checkpoint_metadata_in_transaction(
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
        "actorId": "etyma-knowledge-store"
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
                "etyma.sqlite:graphqlite",
                snapshot.generated_at as i64,
            ),
        )
        .context("failed storing graph checkpoint metadata")?;
    Ok(())
}

pub(super) fn persist_graph_evidence_record_index_in_transaction(
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

fn graphqlite_extension_version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

pub(super) fn mark_import_jobs_graph_ready_in_transaction(
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
