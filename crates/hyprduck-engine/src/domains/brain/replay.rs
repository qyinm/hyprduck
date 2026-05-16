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
    replay_state.apply_accepted_mutations()?;
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
    provider_overlay_events: Vec<BrainEvent>,
    accepted_mutations: Vec<AcceptedReplayMutation>,
}

impl BrainReplayState {
    fn new(workspace_id: &str) -> Self {
        Self {
            snapshot: empty_replayed_brain_snapshot(workspace_id),
            pending_proposals: BTreeMap::new(),
            provider_overlay_events: Vec::new(),
            accepted_mutations: Vec::new(),
        }
    }

    fn apply_event(&mut self, event: &BrainEvent) -> Result<()> {
        apply_replayed_brain_event(
            event,
            &mut self.snapshot,
            &mut self.pending_proposals,
            &mut self.provider_overlay_events,
            &mut self.accepted_mutations,
        )
    }

    fn replay_provider_overlays(&mut self) -> Result<()> {
        crate::domains::knowledge::replay_materialized_graph_overlay_events(
            &mut self.snapshot,
            &self.provider_overlay_events,
        )
    }

    fn apply_accepted_mutations(&mut self) -> Result<()> {
        for mutation in &self.accepted_mutations {
            match mutation {
                AcceptedReplayMutation::Proposal(proposal) => {
                    apply_replayed_accepted_proposal(&mut self.snapshot, proposal)?;
                }
                AcceptedReplayMutation::Memory(memory) => {
                    upsert_replayed_memory(&mut self.snapshot, memory.clone());
                }
            }
        }
        Ok(())
    }

    fn into_snapshot(self) -> BrainRepoSnapshot {
        self.snapshot
    }
}

fn apply_replayed_brain_event(
    event: &BrainEvent,
    snapshot: &mut BrainRepoSnapshot,
    pending_proposals: &mut BTreeMap<String, BrainUpdateProposal>,
    provider_overlay_events: &mut Vec<BrainEvent>,
    accepted_mutations: &mut Vec<AcceptedReplayMutation>,
) -> Result<()> {
    match event.event_type {
        BrainEventKind::GraphMaterialized => {
            if is_provider_graph_overlay_event(event) {
                provider_overlay_events.push(event.clone());
                return Ok(());
            }
            let payload =
                serde_json::from_str::<MaterializedGraphEventPayload>(&event.payload_json)
                    .with_context(|| {
                        format!(
                            "failed parsing graph materialized payload for event `{}`",
                            event.event_id
                        )
                    })?;
            if let Some(materialized) = payload.materialized_graph {
                apply_materialized_graph_payload(snapshot, materialized, event);
            } else if let Some(mut proposal) = payload.proposal {
                proposal.status = BrainProposalStatus::Accepted;
                if proposal.kind == BrainProposalKind::Memory {
                    accepted_mutations.push(AcceptedReplayMutation::Memory(
                        memory_record_for_proposal(&proposal),
                    ));
                } else {
                    accepted_mutations.push(AcceptedReplayMutation::Proposal(proposal));
                }
            }
        }
        BrainEventKind::NodeProposed
        | BrainEventKind::ClaimProposed
        | BrainEventKind::LinkProposed
        | BrainEventKind::MemoryProposed
        | BrainEventKind::WikiPageProposed => {
            if let Ok(mut proposal) =
                serde_json::from_str::<BrainUpdateProposal>(&event.payload_json)
            {
                if event.policy_result == "auto_applied"
                    || proposal.status == BrainProposalStatus::Accepted
                {
                    proposal.status = BrainProposalStatus::Accepted;
                    accepted_mutations.push(AcceptedReplayMutation::Proposal(proposal));
                } else {
                    pending_proposals.insert(proposal.proposal_id.clone(), proposal);
                }
            }
        }
        BrainEventKind::MemoryAccepted => {
            if let Ok(proposal) = serde_json::from_str::<BrainUpdateProposal>(&event.payload_json) {
                accepted_mutations.push(AcceptedReplayMutation::Memory(
                    memory_record_for_proposal(&proposal),
                ));
            } else if let Ok(memory) = serde_json::from_str::<MemoryRecord>(&event.payload_json) {
                accepted_mutations.push(AcceptedReplayMutation::Memory(memory));
            }
        }
        BrainEventKind::ReviewResolved => {
            if let Some(proposal_id) = accepted_review_resolved_proposal_id(event) {
                if let Some(mut proposal) = pending_proposals.remove(&proposal_id) {
                    proposal.status = BrainProposalStatus::Accepted;
                    accepted_mutations.push(AcceptedReplayMutation::Proposal(proposal));
                }
            }
        }
        _ => {}
    }
    Ok(())
}

#[derive(Debug, Clone)]
enum AcceptedReplayMutation {
    Proposal(BrainUpdateProposal),
    Memory(MemoryRecord),
}

fn is_provider_graph_overlay_event(event: &BrainEvent) -> bool {
    event.event_type == BrainEventKind::GraphMaterialized
        && matches!(
            event.operation_type.as_deref(),
            Some("full_workspace_rebuild" | "source_graph_build" | "workspace_linking")
        )
        && event.policy_result == "materialized"
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

#[cfg(test)]
mod tests {
    use super::*;
    use hyprduck_engine_types::{
        AgentNewClaimPayload, AgentNewEdgePayload, AgentNewMemoryPayload, AgentNewNodePayload,
    };

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
        let base_event = test_graph_event(
            workspace_id,
            "evt-base",
            "graph_materialized",
            1,
            &[source.clone()],
            std::slice::from_ref(&source_node),
            &[],
            std::slice::from_ref(&evidence),
        );
        let old_provider_event = test_graph_event(
            workspace_id,
            "evt-provider-old",
            "source_graph_build",
            100,
            std::slice::from_ref(&source),
            &[source_node.clone(), concept_y],
            &[],
            std::slice::from_ref(&evidence),
        );
        let new_provider_event = test_graph_event(
            workspace_id,
            "evt-provider-new",
            "source_graph_build",
            200,
            std::slice::from_ref(&source),
            &[source_node, concept_x],
            &[],
            std::slice::from_ref(&evidence),
        );

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
        let base_event = test_graph_event(
            workspace_id,
            "evt-base",
            "graph_materialized",
            1,
            std::slice::from_ref(&source),
            std::slice::from_ref(&source_node),
            &[],
            std::slice::from_ref(&evidence),
        );
        let concept_skipped = provider_test_concept("concept-skipped", &source, &evidence, 100);
        let mut created_after_cutoff = test_graph_event(
            workspace_id,
            "evt-created-after-cutoff",
            "source_graph_build",
            100,
            std::slice::from_ref(&source),
            &[source_node.clone(), concept_skipped],
            &[],
            std::slice::from_ref(&evidence),
        );
        created_after_cutoff.created_at = 1_000;
        let concept_included = provider_test_concept("concept-included", &source, &evidence, 200);
        let mut created_before_cutoff = test_graph_event(
            workspace_id,
            "evt-created-before-cutoff",
            "source_graph_build",
            200,
            std::slice::from_ref(&source),
            &[source_node, concept_included],
            &[],
            std::slice::from_ref(&evidence),
        );
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
        let mut event = test_graph_event(
            workspace_id,
            "evt-corrupt",
            "graph_materialized",
            1,
            &[],
            &[],
            &[],
            &[],
        );
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

    #[test]
    fn reconstruct_applies_accepted_proposals_after_provider_overlays() {
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
        let provider_concept = BrainNodeRecord {
            node_id: "concept-shared".into(),
            kind: BrainNodeKind::Concept,
            label: "Provider Label".into(),
            scope: BrainScope::Project,
            aliases: Vec::new(),
            evidence_ids: vec![evidence.id.clone()],
            source_ids: vec![source.source_id.clone()],
            confidence: Some(0.8),
            updated_at: 100,
        };
        let proposal = BrainUpdateProposal {
            proposal_id: "proposal-user-label".into(),
            workspace_id: workspace_id.into(),
            kind: BrainProposalKind::Node,
            status: BrainProposalStatus::Accepted,
            actor: BrainActor {
                actor_type: BrainActorType::User,
                actor_id: "local-user".into(),
            },
            scope: BrainScope::Project,
            title: "Rename shared concept".into(),
            body: "Use the user approved label.".into(),
            target_node_id: None,
            target_source_id: None,
            relation_kind: None,
            source_refs: vec![source.source_id.clone()],
            node_refs: vec!["concept-shared".into()],
            evidence_refs: vec![evidence.id.clone()],
            proposal_payload: Some(AgentGraphProposalPayload::NewNode {
                node: AgentNewNodePayload {
                    label: "User Approved Label".into(),
                    kind: BrainNodeKind::Concept,
                    source_path: source.source_path.clone(),
                    node_id: Some("concept-shared".into()),
                    aliases: Vec::new(),
                    source_refs: vec![source.source_id.clone()],
                    evidence_refs: vec![evidence.id.clone()],
                    reason: None,
                },
            }),
            created_at: 200,
        };
        let base_event = test_graph_event(
            workspace_id,
            "evt-base",
            "graph_materialized",
            1,
            std::slice::from_ref(&source),
            std::slice::from_ref(&source_node),
            &[],
            std::slice::from_ref(&evidence),
        );
        let provider_event = test_graph_event(
            workspace_id,
            "evt-provider",
            "source_graph_build",
            100,
            std::slice::from_ref(&source),
            &[source_node, provider_concept],
            &[],
            std::slice::from_ref(&evidence),
        );
        let proposal_event = BrainEvent {
            event_id: "evt-proposal-user-label".into(),
            schema_version: BRAIN_EVENT_SCHEMA_VERSION,
            workspace_id: workspace_id.into(),
            scope: BrainScope::Project,
            event_type: BrainEventKind::NodeProposed,
            operation_type: Some("new_node".into()),
            actor: proposal.actor.clone(),
            source_refs: proposal.source_refs.clone(),
            source_markdown_refs: Vec::new(),
            node_refs: proposal.node_refs.clone(),
            relation_refs: Vec::new(),
            claim_refs: Vec::new(),
            memory_refs: Vec::new(),
            target_node_ids: proposal.node_refs.clone(),
            target_edge_ids: Vec::new(),
            target_claim_ids: Vec::new(),
            target_memory_ids: Vec::new(),
            evidence_refs: proposal.evidence_refs.clone(),
            payload_json: serde_json::to_string(&proposal).expect("proposal payload"),
            causality: BrainEventCausality::default(),
            confidence: None,
            policy_result: "auto_applied".into(),
            created_at: 200,
        };
        let events = vec![
            base_event.clone(),
            provider_event.clone(),
            proposal_event.clone(),
        ];

        let replay =
            reconstruct_brain_snapshot_from_events(workspace_id, &events, None, None, None)
                .expect("reconstruct graph");

        let concept = replay
            .snapshot
            .nodes
            .iter()
            .find(|node| node.node_id == "concept-shared")
            .expect("shared concept");
        assert_eq!(concept.label, "User Approved Label");

        let temp = tempfile::tempdir().expect("temp dir");
        let workspace_root = temp.path().join(workspace_id);
        ensure_materialized_brain_repo_dirs(&workspace_root).expect("ensure repo dirs");
        write_json_pretty(
            &workspace_root
                .join("reviews/proposed-updates")
                .join("proposal-user-label.json"),
            &proposal,
        )
        .expect("write accepted proposal");
        let mut materialized_input = empty_replayed_brain_snapshot(workspace_id);
        materialized_input.generated_at = 1;
        materialized_input.sources = vec![source];
        materialized_input.evidence = vec![evidence];
        materialized_input.events = vec![provider_event];
        materialized_input.nodes = replay
            .snapshot
            .nodes
            .iter()
            .filter(|node| node.kind == BrainNodeKind::Source)
            .cloned()
            .collect();
        let effective = compute_effective_brain_snapshot(&workspace_root, &materialized_input)
            .expect("compute materialized snapshot");
        let materialized_concept = effective
            .nodes
            .iter()
            .find(|node| node.node_id == "concept-shared")
            .expect("materialized shared concept");
        assert_eq!(materialized_concept.label, concept.label);
    }

    #[test]
    fn reconstruct_and_materialization_match_accepted_proposal_kinds() {
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
        let concept_a = provider_test_concept("concept-a", &source, &evidence, 1);
        let concept_b = provider_test_concept("concept-b", &source, &evidence, 1);
        let base_event = test_graph_event(
            workspace_id,
            "evt-base",
            "graph_materialized",
            1,
            std::slice::from_ref(&source),
            &[source_node.clone(), concept_a.clone(), concept_b.clone()],
            &[],
            std::slice::from_ref(&evidence),
        );
        let proposals = vec![
            accepted_test_proposal(
                workspace_id,
                "proposal-claim",
                BrainProposalKind::Claim,
                "Accepted claim",
                "Claim accepted by user.",
                vec![source.source_id.clone()],
                vec!["concept-a".into()],
                vec![evidence.id.clone()],
                None,
                Some(AgentGraphProposalPayload::NewClaim {
                    claim: AgentNewClaimPayload {
                        statement: "Claim accepted by user.".into(),
                        source_path: source.source_path.clone(),
                        claim_id: Some("claim-parity".into()),
                        topic_refs: vec!["concept-a".into()],
                        source_refs: vec![source.source_id.clone()],
                        evidence_refs: vec![evidence.id.clone()],
                        reason: None,
                    },
                }),
                10,
            ),
            accepted_test_proposal(
                workspace_id,
                "proposal-link",
                BrainProposalKind::Link,
                "Accepted relation",
                "Concept A relates to concept B.",
                vec![source.source_id.clone()],
                vec!["concept-a".into()],
                vec![evidence.id.clone()],
                Some("concept-b".into()),
                Some(AgentGraphProposalPayload::NewEdge {
                    edge: AgentNewEdgePayload {
                        source_node_id: "concept-a".into(),
                        target_node_id: "concept-b".into(),
                        kind: BrainRelationKind::RelatedTo,
                        label: "relates to".into(),
                        source_path: source.source_path.clone(),
                        edge_id: Some("relation-parity".into()),
                        source_refs: vec![source.source_id.clone()],
                        evidence_refs: vec![evidence.id.clone()],
                        reason: None,
                    },
                }),
                11,
            ),
            accepted_test_proposal(
                workspace_id,
                "proposal-wiki",
                BrainProposalKind::WikiPage,
                "Accepted wiki page",
                "Wiki page accepted by user.",
                vec![source.source_id.clone()],
                vec!["concept-a".into()],
                vec![evidence.id.clone()],
                Some("concept-a".into()),
                None,
                12,
            ),
            accepted_test_proposal(
                workspace_id,
                "proposal-memory",
                BrainProposalKind::Memory,
                "Accepted memory",
                "Memory accepted by user.",
                vec![source.source_id.clone()],
                Vec::new(),
                vec![evidence.id.clone()],
                None,
                Some(AgentGraphProposalPayload::NewMemory {
                    memory: AgentNewMemoryPayload {
                        title: "Accepted memory".into(),
                        body: "Memory accepted by user.".into(),
                        source_path: source.source_path.clone(),
                        memory_id: Some("memory-parity".into()),
                        source_refs: vec![source.source_id.clone()],
                        evidence_refs: vec![evidence.id.clone()],
                        reason: None,
                    },
                }),
                13,
            ),
        ];
        let mut events = vec![base_event.clone()];
        for proposal in &proposals {
            let mut event = brain_event_for_proposal(proposal).expect("proposal event");
            event.policy_result = "auto_applied".into();
            events.push(event);
            if proposal.kind == BrainProposalKind::Memory {
                events.push(brain_memory_accepted_event(proposal).expect("memory accepted event"));
            }
        }
        let replay =
            reconstruct_brain_snapshot_from_events(workspace_id, &events, None, None, None)
                .expect("reconstruct proposal parity snapshot");

        let temp = tempfile::tempdir().expect("temp dir");
        let workspace_root = temp.path().join(workspace_id);
        ensure_materialized_brain_repo_dirs(&workspace_root).expect("ensure repo dirs");
        for proposal in &proposals {
            write_json_pretty(
                &workspace_root
                    .join("reviews/proposed-updates")
                    .join(format!("{}.json", proposal.proposal_id)),
                proposal,
            )
            .expect("write accepted proposal");
        }
        write_json_pretty(
            &workspace_root.join("memory/records.json"),
            &[memory_record_for_proposal(
                proposals
                    .iter()
                    .find(|proposal| proposal.kind == BrainProposalKind::Memory)
                    .expect("memory proposal"),
            )],
        )
        .expect("write memory record");
        let mut materialized_input = empty_replayed_brain_snapshot(workspace_id);
        materialized_input.generated_at = 1;
        materialized_input.sources = vec![source];
        materialized_input.evidence = vec![evidence];
        materialized_input.nodes = vec![source_node, concept_a, concept_b];
        materialized_input.events = vec![base_event];
        let effective = compute_effective_brain_snapshot(&workspace_root, &materialized_input)
            .expect("compute materialized proposal parity snapshot");

        assert_eq!(
            replay
                .snapshot
                .claims
                .iter()
                .find(|claim| claim.claim_id == "claim-parity")
                .map(|claim| claim.statement.as_str()),
            effective
                .claims
                .iter()
                .find(|claim| claim.claim_id == "claim-parity")
                .map(|claim| claim.statement.as_str())
        );
        assert_eq!(
            replay
                .snapshot
                .relations
                .iter()
                .find(|relation| relation.relation_id == "relation-parity")
                .map(|relation| relation.label.as_str()),
            effective
                .relations
                .iter()
                .find(|relation| relation.relation_id == "relation-parity")
                .map(|relation| relation.label.as_str())
        );
        assert_eq!(
            replay
                .snapshot
                .wiki_pages
                .iter()
                .find(|page| page.title == "Accepted wiki page")
                .map(|page| page.body.as_str()),
            effective
                .wiki_pages
                .iter()
                .find(|page| page.title == "Accepted wiki page")
                .map(|page| page.body.as_str())
        );
        assert_eq!(
            replay
                .snapshot
                .memories
                .iter()
                .find(|memory| memory.memory_id == "memory-parity")
                .map(|memory| memory.body.as_str()),
            effective
                .memories
                .iter()
                .find(|memory| memory.memory_id == "memory-parity")
                .map(|memory| memory.body.as_str())
        );
    }

    fn accepted_test_proposal(
        workspace_id: &str,
        proposal_id: &str,
        kind: BrainProposalKind,
        title: &str,
        body: &str,
        source_refs: Vec<String>,
        node_refs: Vec<String>,
        evidence_refs: Vec<String>,
        target_node_id: Option<String>,
        proposal_payload: Option<AgentGraphProposalPayload>,
        created_at: u64,
    ) -> BrainUpdateProposal {
        BrainUpdateProposal {
            proposal_id: proposal_id.into(),
            workspace_id: workspace_id.into(),
            kind,
            status: BrainProposalStatus::Accepted,
            actor: BrainActor {
                actor_type: BrainActorType::User,
                actor_id: "local-user".into(),
            },
            scope: BrainScope::Project,
            title: title.into(),
            body: body.into(),
            target_node_id,
            target_source_id: None,
            relation_kind: None,
            source_refs,
            node_refs,
            evidence_refs,
            proposal_payload,
            created_at,
        }
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

    fn test_graph_event(
        workspace_id: &str,
        event_id: &str,
        operation_type: &str,
        generated_at: u64,
        sources: &[SourceRecord],
        nodes: &[BrainNodeRecord],
        relations: &[BrainRelationRecord],
        evidence: &[EvidenceRef],
    ) -> BrainEvent {
        BrainEvent {
            event_id: event_id.into(),
            schema_version: BRAIN_EVENT_SCHEMA_VERSION,
            workspace_id: workspace_id.into(),
            scope: BrainScope::Project,
            event_type: BrainEventKind::GraphMaterialized,
            operation_type: Some(operation_type.into()),
            actor: BrainActor {
                actor_type: BrainActorType::Agent,
                actor_id: "hyprduck-provider-graph-agent:test".into(),
            },
            source_refs: sources
                .iter()
                .map(|source| source.source_id.clone())
                .collect(),
            source_markdown_refs: sources
                .iter()
                .map(|source| source.markdown_path.clone())
                .collect(),
            node_refs: nodes.iter().map(|node| node.node_id.clone()).collect(),
            relation_refs: relations
                .iter()
                .map(|relation| relation.relation_id.clone())
                .collect(),
            claim_refs: Vec::new(),
            memory_refs: Vec::new(),
            target_node_ids: nodes
                .iter()
                .filter(|node| node.kind != BrainNodeKind::Source)
                .map(|node| node.node_id.clone())
                .collect(),
            target_edge_ids: relations
                .iter()
                .map(|relation| relation.relation_id.clone())
                .collect(),
            target_claim_ids: Vec::new(),
            target_memory_ids: Vec::new(),
            evidence_refs: evidence
                .iter()
                .map(|evidence| evidence.id.clone())
                .collect(),
            payload_json: materialized_graph_event_payload_json(
                generated_at,
                sources,
                nodes,
                relations,
                evidence,
                &[],
                &[],
                &[],
                &[],
                &[],
            )
            .expect("graph payload"),
            causality: BrainEventCausality {
                caused_by_source_ids: sources
                    .iter()
                    .map(|source| source.source_id.clone())
                    .collect(),
                snapshot_id: Some(format!("snapshot-{event_id}")),
                materialized_version: Some(generated_at),
                ..Default::default()
            },
            confidence: Some("test".into()),
            policy_result: "materialized".into(),
            created_at: generated_at,
        }
    }
}
