use anyhow::{Context, Result};
use graphqlite::{Graph, PropertyValue};
use hyprduck_engine_types::{BrainNodeKind, BrainNodeRecord, BrainRepoSnapshot};
use std::collections::{BTreeMap, BTreeSet};

use super::graphlite_props::{
    brain_node_kind_slug, brain_scope_slug, ensure_graphlite_property_key_id,
    set_graphlite_int_property, set_graphlite_text_property,
};
use super::{
    graph_record_version_id, GraphRecordMetadata, GraphRecordVersionIdentity,
};

#[allow(dead_code)]
pub(crate) fn graph_node_version_identity(
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

pub(super) fn resolve_live_graph_node_version_id(
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

pub(super) fn graph_endpoint_version_id(
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

pub(super) fn invalidate_live_graph_node_versions(
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

pub(super) fn invalidate_live_graph_node_versions_not_in(
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

pub(super) fn node_graph_metadata(
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
    let producer_run_ids = super::producer_run_ids_for_refs(snapshot, &node.evidence_ids, &source_ids);

    GraphRecordMetadata {
        source_ids,
        producer_run_ids,
        status,
    }
}

pub(super) fn node_graph_properties(
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
