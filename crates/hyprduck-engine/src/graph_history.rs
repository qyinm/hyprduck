use std::collections::BTreeSet;
use std::fs;
use std::path::Path;

use anyhow::Result;
use hyprduck_engine_types::{
    BrainEvent, BrainEventKind, BrainRepoSnapshot, GraphHistoryEntry,
    GraphMaterializationReportSummary, GraphSnapshotSourceRecord, ReadGraphHistoryRequest,
    ReadGraphHistoryResponseData, ReadGraphSnapshotRequest, ReadGraphSnapshotResponseData,
    ReadRecentEventsRequest, SourceRecord, SourceStatus,
};
use serde_json::Value;

use crate::{
    latest_readable_materialized_file_refs, read_latest_readable_graph_snapshot_marker,
    BrainReader, KnowledgeStore, MaterializedGraphEventPayload, LATEST_READABLE_SNAPSHOT_PATH,
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
    Ok(ReadGraphHistoryResponseData { states })
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
        updated_at: source.updated_at,
    }
}

fn graph_snapshot_path(value: &str, include_local_paths: bool) -> String {
    if include_local_paths {
        return value.to_string();
    }
    redact_path_for_agent(value)
}

fn redact_path_for_agent(value: &str) -> String {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return String::new();
    }
    Path::new(trimmed)
        .file_name()
        .and_then(|name| name.to_str())
        .filter(|name| !name.trim().is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| "<redacted>".into())
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
    materialized_at: u64,
) -> Vec<String> {
    let mut locations = vec![
        format!("events/brain_events.jsonl#{event_id}"),
        format!("replay://up_to_event_id={event_id}"),
        format!("replay://up_to_materialized_version={materialized_at}"),
    ];
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
