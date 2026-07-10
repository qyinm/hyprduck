use anyhow::{Context, Result};
use graphqlite::{Graph, PropertyValue};
use hyprduck_engine_types::{BrainRelationRecord, BrainRepoSnapshot};
use std::collections::BTreeSet;

use super::graphlite_props::{
    brain_relation_kind_slug, ensure_graphlite_property_key_id, set_graphlite_int_property,
    set_graphlite_text_property,
};
use super::{
    graph_record_version_id, GraphRecordMetadata, GraphRecordVersionIdentity,
};

#[allow(dead_code)]
pub(crate) fn graph_relation_version_identity(
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

pub(super) fn invalidate_live_graph_relation_versions(
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

pub(super) fn invalidate_live_graph_relation_versions_not_in(
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

pub(super) fn relation_graph_metadata(
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
    let producer_run_ids = super::producer_run_ids_for_refs(snapshot, &relation.evidence_ids, &source_ids);

    GraphRecordMetadata {
        source_ids,
        producer_run_ids,
        status: "active".into(),
    }
}

pub(super) fn relation_graph_properties(
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
