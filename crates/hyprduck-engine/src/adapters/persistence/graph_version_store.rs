//! Graph record version history SQL against GraphQLite / wiki_revisions.

use anyhow::{bail, Context, Result};
use graphqlite::Graph;
use hyprduck_engine_types::{
    GraphHistoryRecordKind, GraphRecordHistoryQuery, GraphRecordHistoryVersion,
};

pub(crate) fn read_node_record_versions(
    graph: &Graph,
    workspace_id: &str,
    record_id: &str,
) -> Result<Vec<GraphRecordHistoryVersion>> {
    let sqlite = graph.connection().sqlite_connection();
    let mut statement = sqlite
        .prepare(
            "SELECT
                logical.value AS logical_id,
                COALESCE(NULLIF(version.value, ''), logical.value) AS version_id,
                COALESCE(created.value, '') AS created_by_event_id,
                COALESCE(valid_from.value, 0) AS valid_from,
                COALESCE(valid_to.value, 0) AS valid_to,
                COALESCE(superseded.value, '') AS superseded_by,
                COALESCE(evidence.value, '[]') AS evidence_refs_json,
                COALESCE(source.value, '[]') AS source_refs_json,
                COALESCE(label.value, '') AS title
             FROM node_props_text logical
             JOIN property_keys logical_key
               ON logical_key.id = logical.key_id
              AND logical_key.key = 'logical_id'
             JOIN property_keys workspace_key
               ON workspace_key.key = 'workspace_id'
             JOIN node_props_text workspace
               ON workspace.node_id = logical.node_id
              AND workspace.key_id = workspace_key.id
             LEFT JOIN property_keys version_key ON version_key.key = 'version_id'
             LEFT JOIN node_props_text version
               ON version.node_id = logical.node_id
              AND version.key_id = version_key.id
             LEFT JOIN property_keys created_key ON created_key.key = 'created_by_event_id'
             LEFT JOIN node_props_text created
               ON created.node_id = logical.node_id
              AND created.key_id = created_key.id
             LEFT JOIN property_keys valid_from_key ON valid_from_key.key = 'valid_from'
             LEFT JOIN node_props_int valid_from
               ON valid_from.node_id = logical.node_id
              AND valid_from.key_id = valid_from_key.id
             LEFT JOIN property_keys valid_to_key ON valid_to_key.key = 'valid_to'
             LEFT JOIN node_props_int valid_to
               ON valid_to.node_id = logical.node_id
              AND valid_to.key_id = valid_to_key.id
             LEFT JOIN property_keys superseded_key ON superseded_key.key = 'superseded_by'
             LEFT JOIN node_props_text superseded
               ON superseded.node_id = logical.node_id
              AND superseded.key_id = superseded_key.id
             LEFT JOIN property_keys evidence_key ON evidence_key.key = 'evidence_ids_json'
             LEFT JOIN node_props_text evidence
               ON evidence.node_id = logical.node_id
              AND evidence.key_id = evidence_key.id
             LEFT JOIN property_keys source_key ON source_key.key = 'source_ids_json'
             LEFT JOIN node_props_text source
               ON source.node_id = logical.node_id
              AND source.key_id = source_key.id
             LEFT JOIN property_keys label_key ON label_key.key = 'label'
             LEFT JOIN node_props_text label
               ON label.node_id = logical.node_id
              AND label.key_id = label_key.id
             WHERE workspace.value = ?1
               AND logical.value = ?2
             ORDER BY COALESCE(valid_from.value, 0) DESC, version_id DESC",
        )
        .context("failed preparing graph node history query")?;
    let mut rows = statement
        .query((workspace_id, record_id))
        .context("failed querying graph node history")?;
    let mut versions = Vec::new();
    while let Some(row) = rows.next().context("failed reading graph node history")? {
        let logical_id: String = row.get(0).context("read node logical id")?;
        let version_id: String = row.get(1).context("read node version id")?;
        let title = row.get::<_, String>(8).context("read node title")?;
        versions.push(GraphRecordHistoryVersion {
            record_kind: GraphHistoryRecordKind::Node,
            logical_id,
            version_id,
            created_by_event_id: row.get(2).context("read node event id")?,
            valid_from: non_negative_u64(row.get::<_, i64>(3).context("read node valid_from")?),
            valid_to: positive_i64_as_u64(row.get::<_, i64>(4).context("read node valid_to")?),
            superseded_by: non_empty_string(row.get(5).context("read node superseded_by")?),
            revision: None,
            predecessor_revision: None,
            title: non_empty_string(title),
            source_node_id: None,
            target_node_id: None,
            evidence_refs: decode_string_array(
                &row.get::<_, String>(6).context("read node evidence refs")?,
            ),
            source_refs: decode_string_array(
                &row.get::<_, String>(7).context("read node source refs")?,
            ),
            node_refs: Vec::new(),
            relation_refs: Vec::new(),
            storage_locations: vec!["hyprduck.sqlite:graphqlite".into()],
            diff_json: None,
        });
    }
    Ok(versions)
}

pub(crate) fn read_relation_record_versions(
    graph: &Graph,
    workspace_id: &str,
    record_id: &str,
) -> Result<Vec<GraphRecordHistoryVersion>> {
    let sqlite = graph.connection().sqlite_connection();
    let mut statement = sqlite
        .prepare(
            "SELECT
                relation.value AS logical_id,
                COALESCE(NULLIF(version.value, ''), relation.value) AS version_id,
                COALESCE(created.value, '') AS created_by_event_id,
                COALESCE(valid_from.value, 0) AS valid_from,
                COALESCE(valid_to.value, 0) AS valid_to,
                COALESCE(superseded.value, '') AS superseded_by,
                COALESCE(evidence.value, '[]') AS evidence_refs_json,
                COALESCE(source.value, '[]') AS source_refs_json,
                COALESCE(source_logical.value, '') AS source_node_id,
                COALESCE(target_logical.value, '') AS target_node_id,
                COALESCE(label.value, '') AS title
             FROM edge_props_text relation
             JOIN property_keys relation_key
               ON relation_key.id = relation.key_id
              AND relation_key.key = 'relation_id'
             JOIN property_keys workspace_key
               ON workspace_key.key = 'workspace_id'
             JOIN edge_props_text workspace
               ON workspace.edge_id = relation.edge_id
              AND workspace.key_id = workspace_key.id
             LEFT JOIN property_keys version_key ON version_key.key = 'version_id'
             LEFT JOIN edge_props_text version
               ON version.edge_id = relation.edge_id
              AND version.key_id = version_key.id
             LEFT JOIN property_keys created_key ON created_key.key = 'created_by_event_id'
             LEFT JOIN edge_props_text created
               ON created.edge_id = relation.edge_id
              AND created.key_id = created_key.id
             LEFT JOIN property_keys valid_from_key ON valid_from_key.key = 'valid_from'
             LEFT JOIN edge_props_int valid_from
               ON valid_from.edge_id = relation.edge_id
              AND valid_from.key_id = valid_from_key.id
             LEFT JOIN property_keys valid_to_key ON valid_to_key.key = 'valid_to'
             LEFT JOIN edge_props_int valid_to
               ON valid_to.edge_id = relation.edge_id
              AND valid_to.key_id = valid_to_key.id
             LEFT JOIN property_keys superseded_key ON superseded_key.key = 'superseded_by'
             LEFT JOIN edge_props_text superseded
               ON superseded.edge_id = relation.edge_id
              AND superseded.key_id = superseded_key.id
             LEFT JOIN property_keys evidence_key ON evidence_key.key = 'evidence_ids_json'
             LEFT JOIN edge_props_text evidence
               ON evidence.edge_id = relation.edge_id
              AND evidence.key_id = evidence_key.id
             LEFT JOIN property_keys source_key ON source_key.key = 'source_ids_json'
             LEFT JOIN edge_props_text source
               ON source.edge_id = relation.edge_id
              AND source.key_id = source_key.id
             LEFT JOIN property_keys source_logical_key ON source_logical_key.key = 'source_logical_id'
             LEFT JOIN edge_props_text source_logical
               ON source_logical.edge_id = relation.edge_id
              AND source_logical.key_id = source_logical_key.id
             LEFT JOIN property_keys target_logical_key ON target_logical_key.key = 'target_logical_id'
             LEFT JOIN edge_props_text target_logical
               ON target_logical.edge_id = relation.edge_id
              AND target_logical.key_id = target_logical_key.id
             LEFT JOIN property_keys label_key ON label_key.key = 'label'
             LEFT JOIN edge_props_text label
               ON label.edge_id = relation.edge_id
              AND label.key_id = label_key.id
             WHERE workspace.value = ?1
               AND relation.value = ?2
             ORDER BY COALESCE(valid_from.value, 0) DESC, version_id DESC",
        )
        .context("failed preparing graph relation history query")?;
    let mut rows = statement
        .query((workspace_id, record_id))
        .context("failed querying graph relation history")?;
    let mut versions = Vec::new();
    while let Some(row) = rows
        .next()
        .context("failed reading graph relation history")?
    {
        let logical_id: String = row.get(0).context("read relation logical id")?;
        let version_id: String = row.get(1).context("read relation version id")?;
        let title = row.get::<_, String>(10).context("read relation title")?;
        versions.push(GraphRecordHistoryVersion {
            record_kind: GraphHistoryRecordKind::Relation,
            logical_id,
            version_id,
            created_by_event_id: row.get(2).context("read relation event id")?,
            valid_from: non_negative_u64(row.get::<_, i64>(3).context("read relation valid_from")?),
            valid_to: positive_i64_as_u64(row.get::<_, i64>(4).context("read relation valid_to")?),
            superseded_by: non_empty_string(row.get(5).context("read relation superseded_by")?),
            revision: None,
            predecessor_revision: None,
            title: non_empty_string(title),
            source_node_id: non_empty_string(row.get(8).context("read relation source node id")?),
            target_node_id: non_empty_string(row.get(9).context("read relation target node id")?),
            evidence_refs: decode_string_array(
                &row.get::<_, String>(6)
                    .context("read relation evidence refs")?,
            ),
            source_refs: decode_string_array(
                &row.get::<_, String>(7)
                    .context("read relation source refs")?,
            ),
            node_refs: Vec::new(),
            relation_refs: Vec::new(),
            storage_locations: vec!["hyprduck.sqlite:graphqlite".into()],
            diff_json: None,
        });
    }
    Ok(versions)
}

pub(crate) fn read_wiki_record_versions(
    graph: &Graph,
    workspace_id: &str,
    query: &GraphRecordHistoryQuery,
    include_diff: bool,
) -> Result<Vec<GraphRecordHistoryVersion>> {
    let sqlite = graph.connection().sqlite_connection();
    let (field, value) = match (query.record_id.as_deref(), query.wiki_path.as_deref()) {
        (Some(record_id), None) => ("wr.wiki_page_id", record_id),
        (None, Some(wiki_path)) => ("wp.path", wiki_path),
        _ => bail!("wiki history query requires recordId or wikiPath"),
    };
    let sql = format!(
        "SELECT
            wr.wiki_page_id,
            wr.revision,
            COALESCE(NULLIF(wr.version_id, ''), wr.wiki_page_id || ':' || wr.revision) AS version_id,
            wr.created_by_event_id,
            wr.predecessor_revision,
            wr.superseded_by_event_id,
            wr.valid_from,
            wr.valid_to,
            wr.title,
            wr.evidence_refs_json,
            wr.source_refs_json,
            wr.node_refs_json,
            wr.relation_refs_json,
            wr.diff_json
         FROM wiki_revisions wr
         LEFT JOIN wiki_pages wp
           ON wp.wiki_page_id = wr.wiki_page_id
          AND wp.workspace_id = wr.workspace_id
         WHERE wr.workspace_id = ?1
           AND {field} = ?2
         ORDER BY wr.revision DESC"
    );
    let mut statement = sqlite
        .prepare(&sql)
        .context("failed preparing wiki history query")?;
    let mut rows = statement
        .query((workspace_id, value))
        .context("failed querying wiki history")?;
    let mut versions = Vec::new();
    while let Some(row) = rows.next().context("failed reading wiki history")? {
        let diff_json = row.get::<_, String>(13).context("read wiki diff")?;
        versions.push(GraphRecordHistoryVersion {
            record_kind: GraphHistoryRecordKind::WikiPage,
            logical_id: row.get(0).context("read wiki page id")?,
            version_id: row.get(2).context("read wiki version id")?,
            created_by_event_id: row.get(3).context("read wiki event id")?,
            valid_from: non_negative_u64(row.get::<_, i64>(6).context("read wiki valid_from")?),
            valid_to: positive_i64_as_u64(row.get::<_, i64>(7).context("read wiki valid_to")?),
            superseded_by: non_empty_string(row.get(5).context("read wiki superseded_by")?),
            revision: Some(row.get(1).context("read wiki revision")?),
            predecessor_revision: row.get(4).context("read wiki predecessor revision")?,
            title: non_empty_string(row.get(8).context("read wiki title")?),
            source_node_id: None,
            target_node_id: None,
            evidence_refs: decode_string_array(
                &row.get::<_, String>(9).context("read wiki evidence refs")?,
            ),
            source_refs: decode_string_array(
                &row.get::<_, String>(10).context("read wiki source refs")?,
            ),
            node_refs: decode_string_array(
                &row.get::<_, String>(11).context("read wiki node refs")?,
            ),
            relation_refs: decode_string_array(
                &row.get::<_, String>(12)
                    .context("read wiki relation refs")?,
            ),
            storage_locations: vec!["hyprduck.sqlite:wiki_revisions".into()],
            diff_json: include_diff.then_some(diff_json),
        });
    }
    Ok(versions)
}

fn decode_string_array(value: &str) -> Vec<String> {
    serde_json::from_str::<Vec<String>>(value).unwrap_or_default()
}

fn non_empty_string(value: String) -> Option<String> {
    (!value.trim().is_empty()).then_some(value)
}

fn non_negative_u64(value: i64) -> u64 {
    value.max(0) as u64
}

fn positive_i64_as_u64(value: i64) -> Option<u64> {
    (value > 0).then_some(value as u64)
}
