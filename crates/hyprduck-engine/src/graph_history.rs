use std::collections::BTreeSet;
use std::fs;
use std::path::Path;

use anyhow::{bail, Context, Result};
use graphqlite::Graph;
use hyprduck_engine_types::{
    BrainEvent, BrainEventKind, BrainRepoSnapshot, GraphHistoryEntry, GraphHistoryRecordKind,
    GraphMaterializationReportSummary, GraphRecordHistoryQuery, GraphRecordHistoryResponse,
    GraphRecordHistoryVersion, GraphSnapshotSourceRecord, ReadGraphHistoryRequest,
    ReadGraphHistoryResponseData, ReadGraphSnapshotRequest, ReadGraphSnapshotResponseData,
    ReadRecentEventsRequest, SourceRecord, SourceStatus,
};
use serde_json::Value;

use crate::{
    latest_readable_materialized_file_refs, policy::redact_path_for_agent,
    read_latest_readable_graph_snapshot_marker, BrainReader, KnowledgeStore,
    MaterializedGraphEventPayload, LATEST_READABLE_SNAPSHOT_PATH,
};

pub(crate) fn handle_read_graph_history(
    request: ReadGraphHistoryRequest,
) -> Result<ReadGraphHistoryResponseData> {
    let reader = BrainReader::open(&request.scope)?;
    let mut states = reader
        .events
        .iter()
        .filter(|event| {
            event.workspace_id == request.scope.workspace_id
                && is_completed_graph_materialized_event(event)
        })
        .cloned()
        .map(|event| graph_history_entry_from_event(reader.root(), event))
        .collect::<Result<Vec<_>>>()?;
    states.sort_by(|left, right| {
        right
            .materialized_at
            .cmp(&left.materialized_at)
            .then_with(|| right.event_id.cmp(&left.event_id))
    });
    if let Some(limit) = request.limit {
        states.truncate(limit);
    }
    let record_history = read_graph_record_history(reader.root(), &request)?;
    Ok(ReadGraphHistoryResponseData {
        states,
        record_history,
    })
}

pub(crate) fn handle_read_graph_snapshot(
    request: ReadGraphSnapshotRequest,
) -> Result<ReadGraphSnapshotResponseData> {
    let reader = BrainReader::open(&request.scope)?;
    let marker = read_latest_readable_graph_snapshot_marker(reader.root())?;
    let marker_event = marker.as_ref().and_then(|marker| {
        (marker.workspace_id == request.scope.workspace_id).then(|| {
            reader.events.iter().find(|event| {
                event.workspace_id == request.scope.workspace_id
                    && is_completed_graph_materialized_event(event)
                    && event.event_id == marker.event_id
            })
        })?
    });
    let latest = marker_event
        .or_else(|| latest_graph_materialized_event(&reader.events, &request.scope.workspace_id));
    let materialized_at = latest
        .and_then(|event| event.causality.materialized_version)
        .unwrap_or(reader.snapshot.generated_at);
    let created_at = latest
        .map(|event| event.created_at)
        .unwrap_or(reader.snapshot.generated_at);
    let snapshot_id = latest
        .and_then(|event| event.causality.snapshot_id.clone())
        .unwrap_or_else(|| {
            format!(
                "snapshot-{}-{}",
                reader.snapshot.workspace_id, materialized_at
            )
        });
    let source_ingest_id = latest
        .map(graph_snapshot_source_ingest_id)
        .unwrap_or_else(|| format!("materialized://{}", reader.snapshot.workspace_id));
    let materialized_paths = marker
        .as_ref()
        .filter(|_| marker_event.is_some())
        .map(|marker| marker.materialized_files.clone())
        .unwrap_or_else(|| latest_readable_materialized_file_refs(&reader.snapshot));
    let db_projection =
        match read_graph_canvas_projection(reader.root(), &request.scope.workspace_id)? {
            Some(projection) => projection,
            None => (
                reader.snapshot.nodes.clone(),
                reader.snapshot.relations.clone(),
                reader.read_all_wiki_pages()?,
            ),
        };
    let sources = read_graph_snapshot_sources(
        reader.root(),
        &request.scope.workspace_id,
        &reader.snapshot,
        request.include_local_paths,
    )?;
    let source_paths = graph_snapshot_source_paths(&sources);

    Ok(ReadGraphSnapshotResponseData {
        snapshot_id,
        source_ingest_id,
        workspace_id: reader.snapshot.workspace_id.clone(),
        source_of_truth_path: "events/brain_events.jsonl".into(),
        latest_readable_snapshot_path: LATEST_READABLE_SNAPSHOT_PATH.into(),
        created_at,
        materialized_at,
        materialized_paths,
        source_paths,
        sources,
        graph_materialization_reports: read_graph_materialization_reports(reader.root()),
        nodes: db_projection.0,
        edges: db_projection.1,
        claims: reader.snapshot.claims.clone(),
        memory_refs: reader
            .snapshot
            .memories
            .iter()
            .map(|memory| memory.memory_id.clone())
            .collect(),
        wiki_pages: db_projection.2,
    })
}

fn read_graph_canvas_projection(
    root: &Path,
    workspace_id: &str,
) -> Result<
    Option<(
        Vec<hyprduck_engine_types::BrainNodeRecord>,
        Vec<hyprduck_engine_types::BrainRelationRecord>,
        Vec<hyprduck_engine_types::WikiPage>,
    )>,
> {
    let db_path = KnowledgeStore::default_path_for_root(root);
    if !db_path.exists() {
        return Ok(None);
    }
    KnowledgeStore::open(db_path)?.read_graph_canvas_projection_from_db(workspace_id)
}

fn read_graph_snapshot_sources(
    root: &Path,
    workspace_id: &str,
    snapshot: &BrainRepoSnapshot,
    include_local_paths: bool,
) -> Result<Vec<GraphSnapshotSourceRecord>> {
    let db_path = KnowledgeStore::default_path_for_root(root);
    if db_path.exists() {
        let sources = KnowledgeStore::open(db_path)?
            .read_graph_snapshot_sources_from_db(workspace_id, include_local_paths)?;
        if !sources.is_empty() {
            return Ok(sources);
        }
    }
    Ok(snapshot
        .sources
        .iter()
        .map(|source| graph_snapshot_source_from_record(source, include_local_paths))
        .collect())
}

fn read_graph_record_history(
    root: &Path,
    request: &ReadGraphHistoryRequest,
) -> Result<Option<GraphRecordHistoryResponse>> {
    let Some(query) = graph_record_history_query(request)? else {
        return Ok(None);
    };
    let db_path = KnowledgeStore::default_path_for_root(root);
    if !db_path.exists() {
        return Ok(Some(GraphRecordHistoryResponse {
            query,
            versions: Vec::new(),
        }));
    }

    KnowledgeStore::open(db_path.clone())?;
    let graph = Graph::open(&db_path).context("GraphQLite failed to open knowledge DB")?;
    let mut versions = match query.record_kind {
        GraphHistoryRecordKind::Node => {
            let record_id = query
                .record_id
                .as_deref()
                .expect("node history query has record_id");
            read_node_record_versions(&graph, &request.scope.workspace_id, record_id)?
        }
        GraphHistoryRecordKind::Relation => {
            let record_id = query
                .record_id
                .as_deref()
                .expect("relation history query has record_id");
            read_relation_record_versions(&graph, &request.scope.workspace_id, record_id)?
        }
        GraphHistoryRecordKind::WikiPage => read_wiki_record_versions(
            &graph,
            &request.scope.workspace_id,
            &query,
            request.include_diff,
        )?,
    };
    if let Some(limit) = request.limit {
        versions.truncate(limit);
    }
    Ok(Some(GraphRecordHistoryResponse { query, versions }))
}

fn graph_record_history_query(
    request: &ReadGraphHistoryRequest,
) -> Result<Option<GraphRecordHistoryQuery>> {
    let record_id = normalized_history_arg("recordId", request.record_id.as_deref())?;
    let wiki_path = normalized_history_arg("wikiPath", request.wiki_path.as_deref())?;
    match (request.record_kind, record_id, wiki_path) {
        (None, None, None) => Ok(None),
        (None, Some(_), None) => bail!("argument recordKind is required when recordId is provided"),
        (None, Some(_), Some(_)) => {
            bail!("argument recordId requires recordKind; use wikiPath alone for wiki page history")
        }
        (None, None, Some(wiki_path)) => Ok(Some(GraphRecordHistoryQuery {
            record_kind: GraphHistoryRecordKind::WikiPage,
            record_id: None,
            wiki_path: Some(wiki_path),
        })),
        (Some(GraphHistoryRecordKind::Node), Some(record_id), None) => {
            Ok(Some(GraphRecordHistoryQuery {
                record_kind: GraphHistoryRecordKind::Node,
                record_id: Some(record_id),
                wiki_path: None,
            }))
        }
        (Some(GraphHistoryRecordKind::Relation), Some(record_id), None) => {
            Ok(Some(GraphRecordHistoryQuery {
                record_kind: GraphHistoryRecordKind::Relation,
                record_id: Some(record_id),
                wiki_path: None,
            }))
        }
        (Some(GraphHistoryRecordKind::Node | GraphHistoryRecordKind::Relation), None, None) => {
            bail!("argument recordId is required when recordKind is node or relation")
        }
        (Some(GraphHistoryRecordKind::Node | GraphHistoryRecordKind::Relation), _, Some(_)) => {
            bail!("argument wikiPath can only be used with recordKind wiki_page or without recordKind")
        }
        (Some(GraphHistoryRecordKind::WikiPage), Some(record_id), None) => {
            Ok(Some(GraphRecordHistoryQuery {
                record_kind: GraphHistoryRecordKind::WikiPage,
                record_id: Some(record_id),
                wiki_path: None,
            }))
        }
        (Some(GraphHistoryRecordKind::WikiPage), None, Some(wiki_path)) => {
            Ok(Some(GraphRecordHistoryQuery {
                record_kind: GraphHistoryRecordKind::WikiPage,
                record_id: None,
                wiki_path: Some(wiki_path),
            }))
        }
        (Some(GraphHistoryRecordKind::WikiPage), None, None) => {
            bail!("argument recordId or wikiPath is required when recordKind is wiki_page")
        }
        (Some(GraphHistoryRecordKind::WikiPage), Some(_), Some(_)) => {
            bail!("use only one of recordId or wikiPath for wiki_page history")
        }
    }
}

fn normalized_history_arg(name: &str, value: Option<&str>) -> Result<Option<String>> {
    let Some(value) = value else {
        return Ok(None);
    };
    let value = value.trim();
    if value.is_empty() {
        bail!("argument {name} cannot be empty");
    }
    Ok(Some(value.to_owned()))
}

fn read_node_record_versions(
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

fn read_relation_record_versions(
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

fn read_wiki_record_versions(
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

fn read_graph_materialization_reports(root: &Path) -> Vec<GraphMaterializationReportSummary> {
    let artifacts_root = root.join("artifacts");
    let Ok(entries) = fs::read_dir(&artifacts_root) else {
        return Vec::new();
    };
    let mut reports = entries
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.path().join("provider-graph-materialization.json"))
        .filter(|path| path.exists())
        .filter_map(|path| fs::read_to_string(path).ok())
        .filter_map(|contents| {
            serde_json::from_str::<GraphMaterializationReportSummary>(&contents).ok()
        })
        .collect::<Vec<_>>();
    reports.sort_by(|left, right| left.source_id.cmp(&right.source_id));
    reports
}

pub(crate) fn graph_snapshot_source_ingest_id(event: &BrainEvent) -> String {
    event
        .source_refs
        .first()
        .cloned()
        .or_else(|| event.causality.caused_by_source_ids.first().cloned())
        .unwrap_or_else(|| event.event_id.clone())
}

pub(crate) fn latest_graph_materialized_event<'a>(
    events: &'a [BrainEvent],
    workspace_id: &str,
) -> Option<&'a BrainEvent> {
    events
        .iter()
        .filter(|event| {
            event.workspace_id == workspace_id && is_completed_graph_materialized_event(event)
        })
        .max_by(|left, right| {
            left.causality
                .materialized_version
                .unwrap_or(left.created_at)
                .cmp(
                    &right
                        .causality
                        .materialized_version
                        .unwrap_or(right.created_at),
                )
                .then_with(|| left.event_id.cmp(&right.event_id))
        })
}

fn is_completed_graph_materialized_event(event: &BrainEvent) -> bool {
    event.event_type == BrainEventKind::GraphMaterialized
        && event.causality.materialized_version.is_some()
        && !matches!(
            event.policy_result.as_str(),
            "failed" | "stale" | "in_progress" | "ingest_in_progress"
        )
}

fn graph_snapshot_source_paths(sources: &[GraphSnapshotSourceRecord]) -> Vec<String> {
    sources
        .iter()
        .flat_map(|source| [source.source_path.clone(), source.markdown_path.clone()])
        .filter(|path| !path.trim().is_empty())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

fn graph_snapshot_source_from_record(
    source: &SourceRecord,
    include_local_paths: bool,
) -> GraphSnapshotSourceRecord {
    let success_count = if source.status == SourceStatus::failed() {
        0
    } else {
        source.page_count
    };
    GraphSnapshotSourceRecord {
        source_id: source.source_id.clone(),
        workspace_id: source.workspace_id.clone(),
        original_path: graph_snapshot_path(&source.original_path, include_local_paths),
        source_path: graph_snapshot_path(&source.source_path, include_local_paths),
        markdown_path: graph_snapshot_path(&source.markdown_path, include_local_paths),
        format: source.format.clone(),
        status: source.status.clone(),
        page_count: source.page_count,
        success_count,
        failed_count: 0,
        description: source.description.clone(),
        user_context: source.user_context.clone(),
        ingest_instruction: source.ingest_instruction.clone(),
        citation_ready: success_count > 0,
        graph_ready: false,
        graph_status: String::new(),
        manual_retry_available: false,
        updated_at: source.updated_at,
    }
}

fn graph_snapshot_path(value: &str, include_local_paths: bool) -> String {
    if include_local_paths {
        return value.to_string();
    }
    redact_path_for_agent(value)
}

fn graph_history_entry_from_event(root: &Path, event: BrainEvent) -> Result<GraphHistoryEntry> {
    let snapshot_id = event
        .causality
        .snapshot_id
        .clone()
        .unwrap_or_else(|| format!("snapshot-{}-{}", event.workspace_id, event.created_at));
    let materialized_at = event
        .causality
        .materialized_version
        .unwrap_or(event.created_at);
    let payload = serde_json::from_str::<MaterializedGraphEventPayload>(&event.payload_json).ok();
    let fallback_payload =
        serde_json::from_str::<Value>(&event.payload_json).unwrap_or(Value::Null);
    let graph = payload.and_then(|payload| payload.materialized_graph);

    Ok(GraphHistoryEntry {
        snapshot_id: snapshot_id.clone(),
        materialized_at,
        event_id: event.event_id.clone(),
        operation_type: event.operation_type.clone(),
        source_run_ids: graph_history_source_run_ids(&event),
        source_markdown_refs: event.source_markdown_refs.clone(),
        storage_locations: graph_history_storage_locations(
            root,
            &snapshot_id,
            &event.event_id,
            materialized_at,
        ),
        node_count: graph
            .as_ref()
            .map(|graph| graph.nodes.len())
            .or_else(|| json_usize(&fallback_payload, "nodeCount"))
            .unwrap_or(event.node_refs.len()),
        edge_count: graph
            .as_ref()
            .map(|graph| graph.relations.len())
            .or_else(|| json_usize(&fallback_payload, "relationCount"))
            .unwrap_or(event.relation_refs.len()),
        claim_count: graph
            .as_ref()
            .map(|graph| graph.claims.len())
            .or_else(|| json_usize(&fallback_payload, "claimCount"))
            .unwrap_or(event.claim_refs.len()),
        memory_count: graph
            .as_ref()
            .map(|graph| graph.memories.len())
            .or_else(|| json_usize(&fallback_payload, "memoryCount"))
            .unwrap_or(event.memory_refs.len()),
        wiki_page_count: graph
            .as_ref()
            .map(|graph| graph.wiki_pages.len())
            .or_else(|| json_usize(&fallback_payload, "wikiPageCount"))
            .unwrap_or(0),
    })
}

pub(crate) fn graph_history_source_run_ids(event: &BrainEvent) -> Vec<String> {
    let mut ids = BTreeSet::new();
    ids.extend(event.source_refs.iter().cloned());
    ids.extend(event.causality.caused_by_source_ids.iter().cloned());
    ids.extend(event.causality.caused_by_event_ids.iter().cloned());
    ids.into_iter().collect()
}

fn graph_history_storage_locations(
    root: &Path,
    snapshot_id: &str,
    event_id: &str,
    _materialized_at: u64,
) -> Vec<String> {
    let mut locations = vec![format!("events/brain_events.jsonl#{event_id}")];
    let snapshot_files = root.join("snapshots").join(snapshot_id).join("files");
    if snapshot_files.exists() {
        locations.push(format!("snapshots/{snapshot_id}/files"));
    }
    locations.extend([
        "brain-manifest.json".to_string(),
        "graph/nodes.json".to_string(),
        "graph/edges.json".to_string(),
        "graph/claims.json".to_string(),
        "memory/records.json".to_string(),
        "wiki/index.md".to_string(),
    ]);
    locations
}

fn json_usize(value: &Value, key: &str) -> Option<usize> {
    value
        .get(key)
        .and_then(Value::as_u64)
        .and_then(|value| usize::try_from(value).ok())
}

pub(crate) fn event_matches_recent_events_request(
    event: &BrainEvent,
    request: &ReadRecentEventsRequest,
) -> bool {
    if let Some(run_id) = request.run_id.as_deref() {
        if !event_matches_run_id(event, run_id) {
            return false;
        }
    }
    if let Some(source_ref) = request.source_ref.as_deref() {
        if !event.source_refs.iter().any(|value| value == source_ref)
            && !event
                .source_markdown_refs
                .iter()
                .any(|value| value == source_ref)
            && !event
                .causality
                .caused_by_source_ids
                .iter()
                .any(|value| value == source_ref)
        {
            return false;
        }
    }
    if let Some(node_id) = request.node_id.as_deref() {
        if !event.node_refs.iter().any(|value| value == node_id)
            && !event.target_node_ids.iter().any(|value| value == node_id)
        {
            return false;
        }
    }
    if let Some(edge_id) = request.edge_id.as_deref() {
        if !event.relation_refs.iter().any(|value| value == edge_id)
            && !event.target_edge_ids.iter().any(|value| value == edge_id)
        {
            return false;
        }
    }
    if let Some(claim_id) = request.claim_id.as_deref() {
        if !event.claim_refs.iter().any(|value| value == claim_id)
            && !event.target_claim_ids.iter().any(|value| value == claim_id)
        {
            return false;
        }
    }
    if let Some(memory_id) = request.memory_id.as_deref() {
        if !event.memory_refs.iter().any(|value| value == memory_id)
            && !event
                .target_memory_ids
                .iter()
                .any(|value| value == memory_id)
        {
            return false;
        }
    }
    if let Some(change_type) = request.change_type.as_deref() {
        if !event_matches_change_type(event, change_type) {
            return false;
        }
    }
    true
}

fn event_matches_run_id(event: &BrainEvent, run_id: &str) -> bool {
    graph_history_source_run_ids(event)
        .iter()
        .any(|value| value == run_id)
        || event_payload_string(event, "runId").as_deref() == Some(run_id)
}

fn event_matches_change_type(event: &BrainEvent, change_type: &str) -> bool {
    event.operation_type.as_deref() == Some(change_type)
        || serialized_event_type(event).as_deref() == Some(change_type)
        || event_payload_string(event, "changeType").as_deref() == Some(change_type)
        || event_payload_string(event, "operationType").as_deref() == Some(change_type)
}

fn serialized_event_type(event: &BrainEvent) -> Option<String> {
    serde_json::to_value(event.event_type)
        .ok()
        .and_then(|value| value.as_str().map(ToOwned::to_owned))
}

fn event_payload_string(event: &BrainEvent, key: &str) -> Option<String> {
    serde_json::from_str::<Value>(&event.payload_json)
        .ok()
        .and_then(|value| {
            value
                .get(key)
                .and_then(Value::as_str)
                .map(ToOwned::to_owned)
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use hyprduck_engine_types::{
        BrainActor, BrainActorType, BrainEventCausality, BrainNodeKind, BrainNodeRecord,
        BrainReadScope, BrainRelationKind, BrainRelationRecord, BrainRepoSnapshot, BrainScope,
        EvidenceRef, PolicyResult, SourceFormat, SourceRecord, SourceStatus, WikiPage,
        BRAIN_EVENT_SCHEMA_VERSION,
    };

    #[test]
    fn read_graph_history_returns_record_versions_for_graph_and_wiki_records() {
        let temp = tempfile::tempdir().expect("temp dir");
        let workspace_root = temp.path().join("workspace-default");
        let store = KnowledgeStore::open(KnowledgeStore::default_path_for_root(&workspace_root))
            .expect("open knowledge store");
        store
            .persist_graph_snapshot(&history_snapshot("event-a", 10, "Alpha"))
            .expect("persist first snapshot");
        store
            .persist_graph_snapshot(&history_snapshot("event-b", 20, "Beta"))
            .expect("persist second snapshot");

        let scope = BrainReadScope {
            workspace_id: "workspace-default".into(),
            root_dir: Some(temp.path().to_string_lossy().to_string()),
        };
        let node_history = handle_read_graph_history(ReadGraphHistoryRequest {
            scope: scope.clone(),
            limit: None,
            record_kind: Some(GraphHistoryRecordKind::Node),
            record_id: Some("node-history".into()),
            wiki_path: None,
            include_diff: false,
        })
        .expect("read node history")
        .record_history
        .expect("node record history");
        assert_eq!(node_history.versions.len(), 2);
        assert_eq!(node_history.versions[0].logical_id, "node-history");
        assert_eq!(node_history.versions[0].created_by_event_id, "event-b");
        assert_eq!(node_history.versions[0].valid_to, None);
        assert_eq!(node_history.versions[1].created_by_event_id, "event-a");
        assert_eq!(node_history.versions[1].valid_to, Some(20));
        assert_eq!(
            node_history.versions[0].evidence_refs,
            vec!["evidence-history"]
        );
        assert_eq!(
            node_history.versions[0].storage_locations,
            vec!["hyprduck.sqlite:graphqlite"]
        );

        let relation_history = handle_read_graph_history(ReadGraphHistoryRequest {
            scope: scope.clone(),
            limit: None,
            record_kind: Some(GraphHistoryRecordKind::Relation),
            record_id: Some("rel-history".into()),
            wiki_path: None,
            include_diff: false,
        })
        .expect("read relation history")
        .record_history
        .expect("relation record history");
        assert_eq!(relation_history.versions.len(), 2);
        assert_eq!(
            relation_history.versions[0].source_node_id.as_deref(),
            Some("node-history")
        );
        assert_eq!(
            relation_history.versions[0].target_node_id.as_deref(),
            Some("node-target")
        );
        assert_eq!(relation_history.versions[0].created_by_event_id, "event-b");

        let wiki_history = handle_read_graph_history(ReadGraphHistoryRequest {
            scope,
            limit: None,
            record_kind: None,
            record_id: None,
            wiki_path: Some("wiki/history.md".into()),
            include_diff: true,
        })
        .expect("read wiki history")
        .record_history
        .expect("wiki record history");
        assert_eq!(
            wiki_history.query.record_kind,
            GraphHistoryRecordKind::WikiPage
        );
        assert_eq!(wiki_history.versions.len(), 2);
        assert_eq!(wiki_history.versions[0].revision, Some(2));
        assert_eq!(wiki_history.versions[0].created_by_event_id, "event-b");
        assert_eq!(wiki_history.versions[0].node_refs, vec!["node-history"]);
        assert_eq!(
            wiki_history.versions[0].storage_locations,
            vec!["hyprduck.sqlite:wiki_revisions"]
        );
        assert_eq!(wiki_history.versions[0].diff_json.as_deref(), Some("{}"));
        assert_eq!(wiki_history.versions[1].revision, Some(1));
        assert_eq!(wiki_history.versions[1].valid_to, Some(20));
    }

    #[test]
    fn read_graph_history_rejects_invalid_record_history_arguments() {
        let temp = tempfile::tempdir().expect("temp dir");
        let scope = BrainReadScope {
            workspace_id: "workspace-default".into(),
            root_dir: Some(temp.path().to_string_lossy().to_string()),
        };

        let missing_record_id = handle_read_graph_history(ReadGraphHistoryRequest {
            scope: scope.clone(),
            limit: None,
            record_kind: Some(GraphHistoryRecordKind::Node),
            record_id: None,
            wiki_path: None,
            include_diff: false,
        })
        .expect_err("node history requires recordId");
        assert!(missing_record_id.to_string().contains("recordId"));

        let mixed_wiki_path = handle_read_graph_history(ReadGraphHistoryRequest {
            scope,
            limit: None,
            record_kind: Some(GraphHistoryRecordKind::Relation),
            record_id: Some("rel-history".into()),
            wiki_path: Some("wiki/history.md".into()),
            include_diff: false,
        })
        .expect_err("relation history rejects wikiPath");
        assert!(mixed_wiki_path.to_string().contains("wikiPath"));
    }

    #[test]
    fn graph_history_storage_locations_do_not_expose_replay_selectors() {
        let temp = tempfile::tempdir().expect("temp dir");
        let locations =
            graph_history_storage_locations(temp.path(), "snapshot-history", "event-history", 42);

        assert!(locations
            .iter()
            .all(|location| !location.contains("replay://")));
    }

    fn history_snapshot(event_id: &str, timestamp: u64, label: &str) -> BrainRepoSnapshot {
        BrainRepoSnapshot {
            workspace_id: "workspace-default".into(),
            generated_at: timestamp,
            sources: vec![SourceRecord {
                source_id: "source-history".into(),
                workspace_id: "workspace-default".into(),
                original_path: "/tmp/source-history.pdf".into(),
                source_path: "sources/source-history.pdf".into(),
                markdown_path: "sources/source-history.md".into(),
                format: SourceFormat::pdf(),
                status: SourceStatus::ingested(),
                page_count: 1,
                description: String::new(),
                user_context: String::new(),
                ingest_instruction: String::new(),
                updated_at: timestamp,
            }],
            nodes: vec![
                BrainNodeRecord {
                    node_id: "node-history".into(),
                    kind: BrainNodeKind::Concept,
                    label: label.into(),
                    scope: BrainScope::Project,
                    aliases: Vec::new(),
                    evidence_ids: vec!["evidence-history".into()],
                    source_ids: vec!["source-history".into()],
                    confidence: Some(0.9),
                    updated_at: timestamp,
                    valid_from: timestamp,
                    valid_to: None,
                    superseded_by: None,
                },
                BrainNodeRecord {
                    node_id: "node-target".into(),
                    kind: BrainNodeKind::Concept,
                    label: format!("{label} target"),
                    scope: BrainScope::Project,
                    aliases: Vec::new(),
                    evidence_ids: vec!["evidence-history".into()],
                    source_ids: vec!["source-history".into()],
                    confidence: Some(0.8),
                    updated_at: timestamp,
                    valid_from: timestamp,
                    valid_to: None,
                    superseded_by: None,
                },
            ],
            relations: vec![BrainRelationRecord {
                relation_id: "rel-history".into(),
                kind: BrainRelationKind::RelatedTo,
                source_node_id: "node-history".into(),
                target_node_id: "node-target".into(),
                label: "relates".into(),
                evidence_ids: vec!["evidence-history".into()],
                confidence: Some(0.8),
                updated_at: timestamp,
                valid_from: timestamp,
                valid_to: None,
                superseded_by: None,
            }],
            evidence: vec![EvidenceRef {
                id: "evidence-history".into(),
                page_label: "p1".into(),
                page_index: Some(0),
                snippet: format!("{label} evidence."),
                source_path: Some("sources/source-history.pdf".into()),
                source_id: Some("source-history".into()),
                markdown_path: Some("sources/source-history.md".into()),
                image_path: None,
                provenance: Some("test".into()),
            }],
            memories: Vec::new(),
            wiki_pages: vec![WikiPage {
                page_id: "wiki-history".into(),
                workspace_id: "workspace-default".into(),
                path: "wiki/history.md".into(),
                title: "History".into(),
                body: format!("# {label}\n"),
                node_refs: vec!["node-history".into()],
                source_refs: vec!["source-history".into()],
                evidence_refs: vec!["evidence-history".into()],
                updated_at: timestamp,
            }],
            entities: Vec::new(),
            claims: Vec::new(),
            extractions: Vec::new(),
            events: vec![BrainEvent {
                event_id: event_id.into(),
                schema_version: BRAIN_EVENT_SCHEMA_VERSION,
                workspace_id: "workspace-default".into(),
                scope: BrainScope::Project,
                event_type: BrainEventKind::GraphMaterialized,
                operation_type: Some("graph_materialized".into()),
                actor: BrainActor {
                    actor_type: BrainActorType::Agent,
                    actor_id: "history-test".into(),
                },
                source_refs: vec!["source-history".into()],
                source_markdown_refs: Vec::new(),
                node_refs: vec!["node-history".into()],
                relation_refs: vec!["rel-history".into()],
                claim_refs: Vec::new(),
                memory_refs: Vec::new(),
                target_node_ids: Vec::new(),
                target_edge_ids: Vec::new(),
                target_claim_ids: Vec::new(),
                target_memory_ids: Vec::new(),
                evidence_refs: vec!["evidence-history".into()],
                payload_json: "{}".into(),
                causality: BrainEventCausality {
                    caused_by_event_ids: Vec::new(),
                    caused_by_source_ids: vec!["source-history".into()],
                    snapshot_id: Some(format!("snapshot-{event_id}")),
                    previous_snapshot_id: None,
                    schema_version: BRAIN_EVENT_SCHEMA_VERSION,
                    materialized_version: Some(timestamp),
                },
                confidence: None,
                policy_result: PolicyResult::materialized(),
                created_at: timestamp,
            }],
        }
    }
}
