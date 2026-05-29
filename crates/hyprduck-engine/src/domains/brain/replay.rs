use crate::*;

pub(crate) fn handle_reconstruct_brain(
    request: ReconstructBrainRequest,
) -> Result<ReconstructBrainResponseData> {
    if request.write_materialized {
        bail!(
            "checkpoint rollback is not exposed in v1; reconstruct_brain can only write replay output"
        );
    }

    let root = resolve_brain_workspace_root(&request.scope)?;
    let events_path = root.join("events/brain_events.jsonl");
    let events = read_brain_events_jsonl(&events_path)
        .with_context(|| format!("failed reading replay events {}", events_path.display()))?;
    let replay = reconstruct_brain_snapshot_from_events(
        &request.scope.workspace_id,
        &events,
        request.up_to_timestamp,
        request.up_to_materialized_version,
        request.up_to_event_id.as_deref(),
    )?;
    let snapshot_id = format!(
        "snapshot-replay-{}-{}",
        request.scope.workspace_id,
        replay
            .selected_materialized_version
            .unwrap_or_else(unix_timestamp_seconds)
    );
    let output_root = request
        .output_root
        .as_deref()
        .map(PathBuf::from)
        .unwrap_or_else(|| root.join("snapshots").join(&snapshot_id).join("files"));
    let before = capture_materialized_file_snapshot(&output_root).unwrap_or_default();
    persist_reconstructed_brain_snapshot(&output_root, &replay.snapshot)?;
    let changed_files = changed_materialized_files(
        &before,
        &capture_materialized_file_snapshot(&output_root).unwrap_or_default(),
    );

    Ok(ReconstructBrainResponseData {
        snapshot: replay.snapshot,
        replayed_event_count: replay.replayed_event_count,
        selected_event_id: replay.selected_event_id,
        snapshot_id,
        output_root: output_root.display().to_string(),
        changed_files,
    })
}

#[derive(Debug, Clone)]
pub(crate) struct BrainReplayResult {
    snapshot: BrainRepoSnapshot,
    replayed_event_count: usize,
    selected_event_id: Option<String>,
    selected_materialized_version: Option<u64>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct MaterializedGraphEventPayload {
    #[serde(default)]
    pub(crate) materialized_graph: Option<MaterializedGraphPayload>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct MaterializedGraphPayload {
    #[serde(default)]
    pub(crate) generated_at: Option<u64>,
    #[serde(default)]
    pub(crate) sources: Vec<SourceRecord>,
    #[serde(default)]
    pub(crate) nodes: Vec<BrainNodeRecord>,
    #[serde(default, rename = "edges")]
    pub(crate) relations: Vec<BrainRelationRecord>,
    #[serde(default)]
    pub(crate) evidence: Vec<EvidenceRef>,
    #[serde(default)]
    pub(crate) memories: Vec<MemoryRecord>,
    #[serde(default)]
    pub(crate) wiki_pages: Vec<WikiPage>,
    #[serde(default)]
    pub(crate) entities: Vec<EntityRecord>,
    #[serde(default)]
    pub(crate) claims: Vec<ClaimRecord>,
    #[serde(default)]
    pub(crate) extractions: Vec<StructuredExtractionArtifact>,
}

pub(crate) fn reconstruct_brain_snapshot_from_events(
    workspace_id: &str,
    events: &[BrainEvent],
    up_to_timestamp: Option<u64>,
    up_to_materialized_version: Option<u64>,
    up_to_event_id: Option<&str>,
) -> Result<BrainReplayResult> {
    let mut ordered = events
        .iter()
        .enumerate()
        .filter(|(_, event)| event.workspace_id == workspace_id)
        .map(|(index, event)| (index, event.clone()))
        .collect::<Vec<_>>();
    ordered.sort_by(|left, right| {
        left.1
            .causality
            .materialized_version
            .unwrap_or(left.1.created_at)
            .cmp(
                &right
                    .1
                    .causality
                    .materialized_version
                    .unwrap_or(right.1.created_at),
            )
            .then_with(|| left.1.created_at.cmp(&right.1.created_at))
            .then_with(|| left.0.cmp(&right.0))
    });

    let mut replay_state = BrainReplayState::new(workspace_id);
    let mut included = Vec::new();
    let mut selected_event_id = None;
    let mut selected_materialized_version = None;
    for (_, event) in ordered {
        if up_to_timestamp.is_some_and(|cutoff| event.created_at > cutoff) {
            continue;
        }
        if up_to_materialized_version.is_some_and(|cutoff| {
            event
                .causality
                .materialized_version
                .unwrap_or(event.created_at)
                > cutoff
        }) {
            break;
        }
        selected_materialized_version = event
            .causality
            .materialized_version
            .or(Some(event.created_at));
        replay_state.apply_event(&event)?;
        selected_event_id = Some(event.event_id.clone());
        included.push(event.clone());
        if up_to_event_id.is_some_and(|target| target == event.event_id) {
            break;
        }
    }

    replay_state.replay_provider_overlays()?;
    let mut snapshot = replay_state.into_snapshot();
    snapshot.events = included;
    if let Some(target_event_id) = up_to_event_id {
        if selected_event_id.as_deref() != Some(target_event_id) {
            bail!(
                "replay target event `{target_event_id}` was not found in events/brain_events.jsonl"
            );
        }
    }
    snapshot.generated_at = selected_materialized_version.unwrap_or(snapshot.generated_at);
    refresh_materialized_wiki_pages(&mut snapshot);
    Ok(BrainReplayResult {
        replayed_event_count: snapshot.events.len(),
        snapshot,
        selected_event_id,
        selected_materialized_version,
    })
}

#[derive(Debug, Clone)]
struct BrainReplayState {
    snapshot: BrainRepoSnapshot,
    provider_overlay_events: Vec<BrainEvent>,
}

impl BrainReplayState {
    fn new(workspace_id: &str) -> Self {
        Self {
            snapshot: empty_replayed_brain_snapshot(workspace_id),
            provider_overlay_events: Vec::new(),
        }
    }

    fn apply_event(&mut self, event: &BrainEvent) -> Result<()> {
        apply_replayed_brain_event(event, &mut self.snapshot, &mut self.provider_overlay_events)
    }

    fn replay_provider_overlays(&mut self) -> Result<()> {
        crate::domains::knowledge::replay_materialized_graph_overlay_events(
            &mut self.snapshot,
            &self.provider_overlay_events,
            &Default::default(),
            &Default::default(),
        )
        .map(|_| ())
    }

    fn into_snapshot(self) -> BrainRepoSnapshot {
        self.snapshot
    }
}

fn apply_replayed_brain_event(
    event: &BrainEvent,
    snapshot: &mut BrainRepoSnapshot,
    provider_overlay_events: &mut Vec<BrainEvent>,
) -> Result<()> {
    match event.event_type {
        BrainEventKind::GraphMaterialized => {
            if is_provider_graph_overlay_event(event) {
                provider_overlay_events.push(event.clone());
                return Ok(());
            }
            let payload = event
                .payload_as::<MaterializedGraphEventPayload>()
                .with_context(|| {
                    format!(
                        "failed parsing graph materialized payload for event `{}`",
                        event.event_id
                    )
                })?;
            if let Some(materialized) = payload.materialized_graph {
                apply_materialized_graph_payload(snapshot, materialized, event);
            }
        }
        BrainEventKind::MemoryAccepted => {
            if let Ok(memory) = event.payload_as::<MemoryRecord>() {
                upsert_replayed_memory(snapshot, memory);
            }
        }
        _ => {}
    }
    Ok(())
}

fn is_provider_graph_overlay_event(event: &BrainEvent) -> bool {
    event.event_type == BrainEventKind::GraphMaterialized
        && matches!(
            event.operation_type.as_deref(),
            Some("full_workspace_rebuild" | "source_graph_build" | "workspace_linking")
        )
        && event.policy_result == "materialized"
}

fn apply_materialized_graph_payload(
    snapshot: &mut BrainRepoSnapshot,
    payload: MaterializedGraphPayload,
    event: &BrainEvent,
) {
    snapshot.generated_at = payload
        .generated_at
        .or(event.causality.materialized_version)
        .unwrap_or(event.created_at);
    snapshot.sources = payload.sources;
    snapshot.nodes = payload.nodes;
    snapshot.relations = payload.relations;
    snapshot.evidence = payload.evidence;
    snapshot.memories = payload.memories;
    snapshot.wiki_pages = payload.wiki_pages;
    snapshot.entities = payload.entities;
    snapshot.claims = payload.claims;
    snapshot.extractions = payload.extractions;
}

fn upsert_replayed_memory(snapshot: &mut BrainRepoSnapshot, memory: MemoryRecord) {
    if let Some(existing) = snapshot
        .memories
        .iter_mut()
        .find(|existing| existing.memory_id == memory.memory_id)
    {
        merge_memory_record(existing, memory);
    } else {
        snapshot.memories.push(memory);
    }
    snapshot.memories.sort_by(|left, right| {
        right
            .updated_at
            .cmp(&left.updated_at)
            .then_with(|| left.memory_id.cmp(&right.memory_id))
    });
}

pub(crate) fn persist_reconstructed_brain_snapshot(
    root: &Path,
    snapshot: &BrainRepoSnapshot,
) -> Result<()> {
    ensure_materialized_brain_repo_dirs(root)?;
    persist_materialized_graph_and_wiki_state(root, snapshot)?;
    write_json_pretty(&root.join("memory/records.json"), &snapshot.memories)?;
    write_structured_extraction_artifacts(root, &snapshot.extractions)?;
    write_brain_events_jsonl(&root.join("events/brain_events.jsonl"), &snapshot.events)?;
    publish_latest_readable_graph_snapshot_marker(root, snapshot)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reconstruct_brain_rejects_materialized_rollback_writes() {
        let temp = tempfile::tempdir().expect("tempdir");
        let error = handle_reconstruct_brain(ReconstructBrainRequest {
            scope: BrainReadScope {
                workspace_id: "default".into(),
                root_dir: Some(temp.path().display().to_string()),
            },
            up_to_timestamp: None,
            up_to_materialized_version: None,
            up_to_event_id: None,
            output_root: None,
            write_materialized: true,
        })
        .expect_err("materialized rollback writes are not exposed");

        assert!(error
            .to_string()
            .contains("checkpoint rollback is not exposed in v1"));
    }

    #[test]
    fn reconstruct_replays_provider_graph_events_as_latest_overlays() {
        let workspace_id = "default";
        let source = SourceRecord {
            source_id: "source-a".into(),
            workspace_id: workspace_id.into(),
            original_path: "/tmp/source-a.pdf".into(),
            source_path: "/tmp/source-a.pdf".into(),
            markdown_path: "/tmp/source-a.md".into(),
            format: "pdf".into(),
            status: "ingested".into(),
            page_count: 1,
            description: String::new(),
            user_context: String::new(),
            ingest_instruction: String::new(),
            updated_at: 1,
        };
        let evidence = EvidenceRef {
            id: "ev-source-a".into(),
            page_label: "Page 1".into(),
            page_index: Some(0),
            snippet: "Source A evidence.".into(),
            source_path: Some(source.source_path.clone()),
            source_id: Some(source.source_id.clone()),
            markdown_path: Some(source.markdown_path.clone()),
            image_path: None,
            provenance: Some("test".into()),
        };
        let source_node = BrainNodeRecord {
            node_id: "source:source-a".into(),
            kind: BrainNodeKind::Source,
            label: "source-a.pdf".into(),
            scope: BrainScope::Project,
            aliases: Vec::new(),
            evidence_ids: vec![evidence.id.clone()],
            source_ids: vec![source.source_id.clone()],
            confidence: Some(1.0),
            updated_at: 1,
        };
        let concept_x = provider_test_concept("concept-x", &source, &evidence, 200);
        let concept_y = provider_test_concept("concept-y", &source, &evidence, 100);
        let base_event = test_graph_event(TestGraphEventInput {
            workspace_id,
            event_id: "evt-base",
            operation_type: "graph_materialized",
            generated_at: 1,
            sources: std::slice::from_ref(&source),
            nodes: std::slice::from_ref(&source_node),
            relations: &[],
            evidence: std::slice::from_ref(&evidence),
        });
        let old_provider_event = test_graph_event(TestGraphEventInput {
            workspace_id,
            event_id: "evt-provider-old",
            operation_type: "source_graph_build",
            generated_at: 100,
            sources: std::slice::from_ref(&source),
            nodes: &[source_node.clone(), concept_y],
            relations: &[],
            evidence: std::slice::from_ref(&evidence),
        });
        let new_provider_event = test_graph_event(TestGraphEventInput {
            workspace_id,
            event_id: "evt-provider-new",
            operation_type: "source_graph_build",
            generated_at: 200,
            sources: std::slice::from_ref(&source),
            nodes: &[source_node, concept_x],
            relations: &[],
            evidence: std::slice::from_ref(&evidence),
        });

        let replay = reconstruct_brain_snapshot_from_events(
            workspace_id,
            &[base_event, old_provider_event, new_provider_event],
            None,
            None,
            None,
        )
        .expect("reconstruct graph");

        assert!(replay
            .snapshot
            .nodes
            .iter()
            .any(|node| node.node_id == "source:source-a"));
        assert!(replay
            .snapshot
            .nodes
            .iter()
            .any(|node| node.node_id == "concept-x"));
        assert!(!replay
            .snapshot
            .nodes
            .iter()
            .any(|node| node.node_id == "concept-y"));
    }

    #[test]
    fn reconstruct_timestamp_cutoff_filters_without_truncating_replay_order() {
        let workspace_id = "default";
        let source = SourceRecord {
            source_id: "source-a".into(),
            workspace_id: workspace_id.into(),
            original_path: "/tmp/source-a.pdf".into(),
            source_path: "/tmp/source-a.pdf".into(),
            markdown_path: "/tmp/source-a.md".into(),
            format: "pdf".into(),
            status: "ingested".into(),
            page_count: 1,
            description: String::new(),
            user_context: String::new(),
            ingest_instruction: String::new(),
            updated_at: 1,
        };
        let evidence = EvidenceRef {
            id: "ev-source-a".into(),
            page_label: "Page 1".into(),
            page_index: Some(0),
            snippet: "Source A evidence.".into(),
            source_path: Some(source.source_path.clone()),
            source_id: Some(source.source_id.clone()),
            markdown_path: Some(source.markdown_path.clone()),
            image_path: None,
            provenance: Some("test".into()),
        };
        let source_node = BrainNodeRecord {
            node_id: "source:source-a".into(),
            kind: BrainNodeKind::Source,
            label: "source-a.pdf".into(),
            scope: BrainScope::Project,
            aliases: Vec::new(),
            evidence_ids: vec![evidence.id.clone()],
            source_ids: vec![source.source_id.clone()],
            confidence: Some(1.0),
            updated_at: 1,
        };
        let base_event = test_graph_event(TestGraphEventInput {
            workspace_id,
            event_id: "evt-base",
            operation_type: "graph_materialized",
            generated_at: 1,
            sources: std::slice::from_ref(&source),
            nodes: std::slice::from_ref(&source_node),
            relations: &[],
            evidence: std::slice::from_ref(&evidence),
        });
        let concept_skipped = provider_test_concept("concept-skipped", &source, &evidence, 100);
        let mut created_after_cutoff = test_graph_event(TestGraphEventInput {
            workspace_id,
            event_id: "evt-created-after-cutoff",
            operation_type: "source_graph_build",
            generated_at: 100,
            sources: std::slice::from_ref(&source),
            nodes: &[source_node.clone(), concept_skipped],
            relations: &[],
            evidence: std::slice::from_ref(&evidence),
        });
        created_after_cutoff.created_at = 1_000;
        let concept_included = provider_test_concept("concept-included", &source, &evidence, 200);
        let mut created_before_cutoff = test_graph_event(TestGraphEventInput {
            workspace_id,
            event_id: "evt-created-before-cutoff",
            operation_type: "source_graph_build",
            generated_at: 200,
            sources: std::slice::from_ref(&source),
            nodes: &[source_node, concept_included],
            relations: &[],
            evidence: std::slice::from_ref(&evidence),
        });
        created_before_cutoff.created_at = 800;

        let replay = reconstruct_brain_snapshot_from_events(
            workspace_id,
            &[base_event, created_after_cutoff, created_before_cutoff],
            Some(900),
            None,
            None,
        )
        .expect("reconstruct with timestamp cutoff");

        assert!(replay
            .snapshot
            .nodes
            .iter()
            .any(|node| node.node_id == "concept-included"));
        assert!(!replay
            .snapshot
            .nodes
            .iter()
            .any(|node| node.node_id == "concept-skipped"));
    }

    #[test]
    fn reconstruct_fails_on_corrupt_graph_materialized_payload() {
        let workspace_id = "default";
        let mut event = test_graph_event(TestGraphEventInput {
            workspace_id,
            event_id: "evt-corrupt",
            operation_type: "graph_materialized",
            generated_at: 1,
            sources: &[],
            nodes: &[],
            relations: &[],
            evidence: &[],
        });
        event.payload_json = "{not valid json}".into();

        let error = reconstruct_brain_snapshot_from_events(
            workspace_id,
            std::slice::from_ref(&event),
            None,
            None,
            None,
        )
        .expect_err("corrupt graph materialized payload should fail");

        assert!(error
            .to_string()
            .contains("failed parsing graph materialized payload for event `evt-corrupt`"));
    }

    fn provider_test_concept(
        node_id: &str,
        source: &SourceRecord,
        evidence: &EvidenceRef,
        updated_at: u64,
    ) -> BrainNodeRecord {
        BrainNodeRecord {
            node_id: node_id.into(),
            kind: BrainNodeKind::Concept,
            label: node_id.into(),
            scope: BrainScope::Project,
            aliases: Vec::new(),
            evidence_ids: vec![evidence.id.clone()],
            source_ids: vec![source.source_id.clone()],
            confidence: Some(0.9),
            updated_at,
        }
    }

    struct TestGraphEventInput<'a> {
        workspace_id: &'a str,
        event_id: &'a str,
        operation_type: &'a str,
        generated_at: u64,
        sources: &'a [SourceRecord],
        nodes: &'a [BrainNodeRecord],
        relations: &'a [BrainRelationRecord],
        evidence: &'a [EvidenceRef],
    }

    fn test_graph_event(input: TestGraphEventInput<'_>) -> BrainEvent {
        BrainEvent {
            event_id: input.event_id.into(),
            schema_version: BRAIN_EVENT_SCHEMA_VERSION,
            workspace_id: input.workspace_id.into(),
            scope: BrainScope::Project,
            event_type: BrainEventKind::GraphMaterialized,
            operation_type: Some(input.operation_type.into()),
            actor: BrainActor {
                actor_type: BrainActorType::Agent,
                actor_id: "hyprduck-provider-graph-agent:test".into(),
            },
            source_refs: input
                .sources
                .iter()
                .map(|source| source.source_id.clone())
                .collect(),
            source_markdown_refs: input
                .sources
                .iter()
                .map(|source| source.markdown_path.clone())
                .collect(),
            node_refs: input
                .nodes
                .iter()
                .map(|node| node.node_id.clone())
                .collect(),
            relation_refs: input
                .relations
                .iter()
                .map(|relation| relation.relation_id.clone())
                .collect(),
            claim_refs: Vec::new(),
            memory_refs: Vec::new(),
            target_node_ids: input
                .nodes
                .iter()
                .filter(|node| node.kind != BrainNodeKind::Source)
                .map(|node| node.node_id.clone())
                .collect(),
            target_edge_ids: input
                .relations
                .iter()
                .map(|relation| relation.relation_id.clone())
                .collect(),
            target_claim_ids: Vec::new(),
            target_memory_ids: Vec::new(),
            evidence_refs: input
                .evidence
                .iter()
                .map(|evidence| evidence.id.clone())
                .collect(),
            payload_json: materialized_graph_event_payload_json(
                input.generated_at,
                input.sources,
                input.nodes,
                input.relations,
                input.evidence,
                &[],
                &[],
                &[],
                &[],
                &[],
            )
            .expect("graph payload"),
            causality: BrainEventCausality {
                caused_by_source_ids: input
                    .sources
                    .iter()
                    .map(|source| source.source_id.clone())
                    .collect(),
                snapshot_id: Some(format!("snapshot-{}", input.event_id)),
                materialized_version: Some(input.generated_at),
                ..Default::default()
            },
            confidence: Some("test".into()),
            policy_result: "materialized".into(),
            created_at: input.generated_at,
        }
    }
}
