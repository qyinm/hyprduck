use crate::*;

pub(crate) fn handle_reconstruct_brain(
    request: ReconstructBrainRequest,
) -> Result<ReconstructBrainResponseData> {
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
    let mut changed_files = changed_materialized_files(
        &before,
        &capture_materialized_file_snapshot(&output_root).unwrap_or_default(),
    );

    if request.write_materialized {
        let writer = BrainWorkspaceWriter::open(root.clone())?;
        let before_current = capture_materialized_file_snapshot(writer.root())?;
        let rollback_at = unix_timestamp_seconds();
        let backup_snapshot_id = format!(
            "snapshot-pre-rollback-{}-{}",
            request.scope.workspace_id, rollback_at
        );
        let previous_snapshot =
            read_materialized_brain_snapshot(writer.root(), &request.scope.workspace_id).ok();
        persist_materialized_snapshot(writer.root(), &backup_snapshot_id, &before_current)?;
        let rollback_result = (|| -> Result<Vec<String>> {
            let mut restored_snapshot = replay.snapshot.clone();
            restored_snapshot.generated_at = rollback_at;
            let rollback_event = brain_graph_rollback_event(
                &restored_snapshot,
                &snapshot_id,
                &backup_snapshot_id,
                previous_snapshot.as_ref(),
                replay.selected_event_id.as_deref(),
                rollback_at,
            )?;
            restored_snapshot.events = events
                .iter()
                .cloned()
                .chain(std::iter::once(rollback_event))
                .collect();
            restore_selected_materialized_brain_snapshot(
                writer.root(),
                &restored_snapshot,
                previous_snapshot.as_ref(),
            )?;
            Ok(changed_materialized_files(
                &before_current,
                &capture_materialized_file_snapshot(writer.root())?,
            ))
        })();
        match rollback_result {
            Ok(current_changed_files) => changed_files = current_changed_files,
            Err(error) => {
                restore_materialized_file_snapshot(writer.root(), &before_current)?;
                return Err(error);
            }
        }
    }

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
    #[serde(default)]
    pub(crate) proposal: Option<BrainUpdateProposal>,
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
            break;
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

    let mut snapshot = replay_state.into_snapshot();
    snapshot.events = included;
    if let Some(target_event_id) = up_to_event_id {
        if selected_event_id.as_deref() != Some(target_event_id) {
            bail!("replay target event `{target_event_id}` was not found in events/brain_events.jsonl");
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
    pending_proposals: BTreeMap<String, BrainUpdateProposal>,
}

impl BrainReplayState {
    fn new(workspace_id: &str) -> Self {
        Self {
            snapshot: empty_replayed_brain_snapshot(workspace_id),
            pending_proposals: BTreeMap::new(),
        }
    }

    fn apply_event(&mut self, event: &BrainEvent) -> Result<()> {
        apply_replayed_brain_event(event, &mut self.snapshot, &mut self.pending_proposals)
    }

    fn into_snapshot(self) -> BrainRepoSnapshot {
        self.snapshot
    }
}

fn apply_replayed_brain_event(
    event: &BrainEvent,
    snapshot: &mut BrainRepoSnapshot,
    pending_proposals: &mut BTreeMap<String, BrainUpdateProposal>,
) -> Result<()> {
    match event.event_type {
        BrainEventKind::GraphMaterialized => {
            let payload =
                serde_json::from_str::<MaterializedGraphEventPayload>(&event.payload_json)
                    .unwrap_or(MaterializedGraphEventPayload {
                        materialized_graph: None,
                        proposal: None,
                    });
            if let Some(materialized) = payload.materialized_graph {
                apply_materialized_graph_payload(snapshot, materialized, event);
            } else if let Some(mut proposal) = payload.proposal {
                proposal.status = BrainProposalStatus::Accepted;
                if proposal.kind == BrainProposalKind::Memory {
                    upsert_replayed_memory(snapshot, memory_record_for_proposal(&proposal));
                } else {
                    apply_accepted_proposal_to_snapshot(&proposal, snapshot)?;
                }
            }
        }
        BrainEventKind::NodeProposed
        | BrainEventKind::ClaimProposed
        | BrainEventKind::LinkProposed
        | BrainEventKind::MemoryProposed => {
            if let Ok(mut proposal) =
                serde_json::from_str::<BrainUpdateProposal>(&event.payload_json)
            {
                if event.policy_result == "auto_applied"
                    || proposal.status == BrainProposalStatus::Accepted
                {
                    proposal.status = BrainProposalStatus::Accepted;
                    apply_replayed_accepted_proposal(snapshot, &proposal)?;
                } else {
                    pending_proposals.insert(proposal.proposal_id.clone(), proposal);
                }
            }
        }
        BrainEventKind::MemoryAccepted => {
            if let Ok(proposal) = serde_json::from_str::<BrainUpdateProposal>(&event.payload_json) {
                upsert_replayed_memory(snapshot, memory_record_for_proposal(&proposal));
            } else if let Ok(memory) = serde_json::from_str::<MemoryRecord>(&event.payload_json) {
                upsert_replayed_memory(snapshot, memory);
            }
        }
        BrainEventKind::ReviewResolved => {
            if let Some(proposal_id) = accepted_review_resolved_proposal_id(event) {
                if let Some(mut proposal) = pending_proposals.remove(&proposal_id) {
                    proposal.status = BrainProposalStatus::Accepted;
                    apply_replayed_accepted_proposal(snapshot, &proposal)?;
                }
            }
        }
        _ => {}
    }
    Ok(())
}

fn apply_replayed_accepted_proposal(
    snapshot: &mut BrainRepoSnapshot,
    proposal: &BrainUpdateProposal,
) -> Result<()> {
    if proposal.kind == BrainProposalKind::Memory {
        upsert_replayed_memory(snapshot, memory_record_for_proposal(proposal));
    } else {
        apply_accepted_proposal_to_snapshot(proposal, snapshot)?;
    }
    Ok(())
}

fn accepted_review_resolved_proposal_id(event: &BrainEvent) -> Option<String> {
    if event.policy_result != "accept" && event.policy_result != "auto_applied" {
        return None;
    }
    let payload = serde_json::from_str::<Value>(&event.payload_json).ok()?;
    let decision = payload
        .get("decision")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let status = payload
        .get("status")
        .and_then(Value::as_str)
        .unwrap_or_default();
    if !matches!(decision, "accept" | "auto_accept") && status != "accepted" {
        return None;
    }
    payload
        .get("proposalId")
        .and_then(Value::as_str)
        .map(str::to_string)
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

fn restore_selected_materialized_brain_snapshot(
    root: &Path,
    snapshot: &BrainRepoSnapshot,
    previous_snapshot: Option<&BrainRepoSnapshot>,
) -> Result<()> {
    ensure_materialized_brain_repo_dirs(root)?;
    if let Some(previous_snapshot) = previous_snapshot {
        let next_wiki_paths = snapshot
            .wiki_pages
            .iter()
            .map(|page| page.path.as_str())
            .collect::<BTreeSet<_>>();
        for page in &previous_snapshot.wiki_pages {
            if !next_wiki_paths.contains(page.path.as_str()) && is_wiki_markdown_ref(&page.path) {
                let path = root.join(&page.path);
                if path.exists() {
                    fs::remove_file(&path).with_context(|| {
                        format!("failed removing stale wiki page {}", path.display())
                    })?;
                }
            }
        }
    }
    persist_materialized_graph_and_wiki_state(root, snapshot)?;
    write_json_pretty(&root.join("memory/records.json"), &snapshot.memories)?;
    write_structured_extraction_artifacts(root, &snapshot.extractions)?;
    write_brain_events_jsonl(&root.join("events/brain_events.jsonl"), &snapshot.events)?;
    publish_latest_readable_graph_snapshot_marker(root, snapshot)?;
    Ok(())
}

fn brain_graph_rollback_event(
    snapshot: &BrainRepoSnapshot,
    snapshot_id: &str,
    pre_rollback_snapshot_id: &str,
    previous_snapshot: Option<&BrainRepoSnapshot>,
    selected_event_id: Option<&str>,
    rollback_at: u64,
) -> Result<BrainEvent> {
    Ok(BrainEvent {
        event_id: format!("evt-{}", Uuid::now_v7()),
        schema_version: BRAIN_EVENT_SCHEMA_VERSION,
        workspace_id: snapshot.workspace_id.clone(),
        scope: BrainScope::Project,
        event_type: BrainEventKind::GraphMaterialized,
        operation_type: Some("graph_rollback".into()),
        actor: BrainActor {
            actor_type: BrainActorType::Agent,
            actor_id: "hyprduck-agent-rollback".into(),
        },
        source_refs: snapshot
            .sources
            .iter()
            .map(|source| source.source_id.clone())
            .collect(),
        source_markdown_refs: snapshot
            .sources
            .iter()
            .map(|source| source.markdown_path.clone())
            .collect(),
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
        payload_json: rollback_materialized_graph_event_payload_json(
            snapshot,
            snapshot_id,
            pre_rollback_snapshot_id,
            previous_snapshot,
            selected_event_id,
        )?,
        causality: BrainEventCausality {
            caused_by_event_ids: selected_event_id
                .map(|event_id| vec![event_id.to_string()])
                .unwrap_or_default(),
            caused_by_source_ids: snapshot
                .sources
                .iter()
                .map(|source| source.source_id.clone())
                .collect(),
            snapshot_id: Some(snapshot_id.to_string()),
            previous_snapshot_id: Some(pre_rollback_snapshot_id.to_string()),
            materialized_version: Some(rollback_at),
            ..Default::default()
        },
        confidence: None,
        policy_result: "rollback_applied".into(),
        created_at: rollback_at,
    })
}

fn rollback_materialized_graph_event_payload_json(
    snapshot: &BrainRepoSnapshot,
    restored_snapshot_id: &str,
    pre_rollback_snapshot_id: &str,
    previous_snapshot: Option<&BrainRepoSnapshot>,
    selected_event_id: Option<&str>,
) -> Result<String> {
    serde_json::to_string(&json!({
        "nodeCount": snapshot.nodes.len(),
        "relationCount": snapshot.relations.len(),
        "sourceCount": snapshot.sources.len(),
        "rollback": {
            "restoredSnapshotId": restored_snapshot_id,
            "preRollbackSnapshotId": pre_rollback_snapshot_id,
            "selectedEventId": selected_event_id,
            "replaySelector": selected_event_id
                .map(|event_id| format!("--event {event_id}"))
                .unwrap_or_else(|| "--latest".into()),
            "sourceEventCount": snapshot.events.len(),
            "sourceOfTruth": "events/brain_events.jsonl",
        },
        "diff": rollback_snapshot_diff(previous_snapshot, snapshot),
        "materializedGraph": {
            "generatedAt": snapshot.generated_at,
            "sources": snapshot.sources,
            "nodes": snapshot.nodes,
            "edges": snapshot.relations,
            "evidence": snapshot.evidence,
            "memories": snapshot.memories,
            "wikiPages": snapshot.wiki_pages,
            "entities": snapshot.entities,
            "claims": snapshot.claims,
            "extractions": snapshot.extractions,
        }
    }))
    .context("failed to encode rollback materialized graph event payload")
}

fn rollback_snapshot_diff(
    previous_snapshot: Option<&BrainRepoSnapshot>,
    snapshot: &BrainRepoSnapshot,
) -> Value {
    let previous_node_ids = previous_snapshot
        .map(|snapshot| {
            snapshot
                .nodes
                .iter()
                .map(|node| node.node_id.as_str())
                .collect::<BTreeSet<_>>()
        })
        .unwrap_or_default();
    let next_node_ids = snapshot
        .nodes
        .iter()
        .map(|node| node.node_id.as_str())
        .collect::<BTreeSet<_>>();
    let previous_edge_ids = previous_snapshot
        .map(|snapshot| {
            snapshot
                .relations
                .iter()
                .map(|relation| relation.relation_id.as_str())
                .collect::<BTreeSet<_>>()
        })
        .unwrap_or_default();
    let next_edge_ids = snapshot
        .relations
        .iter()
        .map(|relation| relation.relation_id.as_str())
        .collect::<BTreeSet<_>>();
    let previous_claim_ids = previous_snapshot
        .map(|snapshot| {
            snapshot
                .claims
                .iter()
                .map(|claim| claim.claim_id.as_str())
                .collect::<BTreeSet<_>>()
        })
        .unwrap_or_default();
    let next_claim_ids = snapshot
        .claims
        .iter()
        .map(|claim| claim.claim_id.as_str())
        .collect::<BTreeSet<_>>();
    let previous_memory_ids = previous_snapshot
        .map(|snapshot| {
            snapshot
                .memories
                .iter()
                .map(|memory| memory.memory_id.as_str())
                .collect::<BTreeSet<_>>()
        })
        .unwrap_or_default();
    let next_memory_ids = snapshot
        .memories
        .iter()
        .map(|memory| memory.memory_id.as_str())
        .collect::<BTreeSet<_>>();
    let previous_wiki_paths = previous_snapshot
        .map(|snapshot| {
            snapshot
                .wiki_pages
                .iter()
                .map(|page| page.path.as_str())
                .collect::<BTreeSet<_>>()
        })
        .unwrap_or_default();
    let next_wiki_paths = snapshot
        .wiki_pages
        .iter()
        .map(|page| page.path.as_str())
        .collect::<BTreeSet<_>>();

    json!({
        "nodeCountBefore": previous_node_ids.len(),
        "nodeCountAfter": next_node_ids.len(),
        "edgeCountBefore": previous_edge_ids.len(),
        "edgeCountAfter": next_edge_ids.len(),
        "claimCountBefore": previous_claim_ids.len(),
        "claimCountAfter": next_claim_ids.len(),
        "memoryCountBefore": previous_memory_ids.len(),
        "memoryCountAfter": next_memory_ids.len(),
        "wikiPageCountBefore": previous_wiki_paths.len(),
        "wikiPageCountAfter": next_wiki_paths.len(),
        "addedNodeIds": sorted_set_difference(&next_node_ids, &previous_node_ids),
        "removedNodeIds": sorted_set_difference(&previous_node_ids, &next_node_ids),
        "addedEdgeIds": sorted_set_difference(&next_edge_ids, &previous_edge_ids),
        "removedEdgeIds": sorted_set_difference(&previous_edge_ids, &next_edge_ids),
        "addedClaimIds": sorted_set_difference(&next_claim_ids, &previous_claim_ids),
        "removedClaimIds": sorted_set_difference(&previous_claim_ids, &next_claim_ids),
        "addedMemoryIds": sorted_set_difference(&next_memory_ids, &previous_memory_ids),
        "removedMemoryIds": sorted_set_difference(&previous_memory_ids, &next_memory_ids),
        "addedWikiPaths": sorted_set_difference(&next_wiki_paths, &previous_wiki_paths),
        "removedWikiPaths": sorted_set_difference(&previous_wiki_paths, &next_wiki_paths),
    })
}

fn sorted_set_difference(left: &BTreeSet<&str>, right: &BTreeSet<&str>) -> Vec<String> {
    left.difference(right)
        .map(|value| (*value).to_string())
        .collect()
}
