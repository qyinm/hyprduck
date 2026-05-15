use crate::*;

pub(crate) fn handle_propose_brain_update(
    request: ProposeBrainUpdateRequest,
) -> Result<ProposeBrainUpdateResponseData> {
    validate_brain_update_proposal(&request)?;
    let root = resolve_brain_workspace_root(&request.scope)?;
    let writer = BrainWorkspaceWriter::open(root)?;

    let created_at = unix_timestamp_seconds();
    let proposal_id = format!("proposal-{}", Uuid::now_v7());
    let mut source_refs = request.source_refs.clone();
    if let Some(source_id) = &request.target_source_id {
        if !source_refs.iter().any(|value| value == source_id) {
            source_refs.push(source_id.clone());
        }
    }
    let mut node_refs = request.node_refs.clone();
    if let Some(node_id) = &request.target_node_id {
        if !node_refs.iter().any(|value| value == node_id) {
            node_refs.push(node_id.clone());
        }
    }
    let mut target_node_id = request.target_node_id.clone();
    let mut relation_kind = request.relation_kind;
    let mut evidence_refs = request.evidence_refs.clone();
    if let Some(payload) = &request.proposal_payload {
        match payload {
            AgentGraphProposalPayload::NewNode { node } => {
                for source_ref in &node.source_refs {
                    merge_unique_string(&mut source_refs, source_ref);
                }
                for evidence_ref in &node.evidence_refs {
                    merge_unique_string(&mut evidence_refs, evidence_ref);
                }
            }
            AgentGraphProposalPayload::NewEdge { edge } => {
                let source_node_id = edge.source_node_id.trim();
                if !source_node_id.is_empty() {
                    node_refs.retain(|node_id| node_id != source_node_id);
                    node_refs.insert(0, source_node_id.to_string());
                }
                merge_unique_string(&mut node_refs, edge.target_node_id.trim());
                for source_ref in &edge.source_refs {
                    merge_unique_string(&mut source_refs, source_ref);
                }
                for evidence_ref in &edge.evidence_refs {
                    merge_unique_string(&mut evidence_refs, evidence_ref);
                }
                if target_node_id.is_none() {
                    target_node_id = Some(edge.target_node_id.trim().to_string());
                }
                if relation_kind.is_none() {
                    relation_kind = Some(edge.kind);
                }
            }
            AgentGraphProposalPayload::NewClaim { claim } => {
                for topic_ref in &claim.topic_refs {
                    merge_unique_string(&mut node_refs, topic_ref);
                }
                for source_ref in &claim.source_refs {
                    merge_unique_string(&mut source_refs, source_ref);
                }
                for evidence_ref in &claim.evidence_refs {
                    merge_unique_string(&mut evidence_refs, evidence_ref);
                }
            }
            AgentGraphProposalPayload::NewMemory { memory } => {
                for source_ref in &memory.source_refs {
                    merge_unique_string(&mut source_refs, source_ref);
                }
                for evidence_ref in &memory.evidence_refs {
                    merge_unique_string(&mut evidence_refs, evidence_ref);
                }
            }
        }
    }

    let auto_apply = should_auto_apply_brain_proposal(&request);
    let proposal = BrainUpdateProposal {
        proposal_id: proposal_id.clone(),
        workspace_id: request.scope.workspace_id.clone(),
        kind: request.kind,
        status: if auto_apply {
            BrainProposalStatus::Accepted
        } else {
            BrainProposalStatus::PendingReview
        },
        actor: request.actor.clone(),
        scope: BrainScope::Project,
        title: request.title.trim().to_string(),
        body: request.body.trim().to_string(),
        target_node_id,
        target_source_id: request.target_source_id.clone(),
        relation_kind,
        source_refs,
        node_refs,
        evidence_refs,
        proposal_payload: request.proposal_payload.clone(),
        created_at,
    };
    let mut event = brain_event_for_proposal(&proposal)?;
    if auto_apply {
        event.policy_result = "auto_applied".into();
    }
    let proposal_path = writer.write_proposal(&proposal)?;
    writer.append_event(&event)?;
    if auto_apply {
        if proposal.kind == BrainProposalKind::SourceNote {
            writer.apply_source_note_metadata(&proposal, &request)?;
        } else if matches!(
            proposal.kind,
            BrainProposalKind::Node
                | BrainProposalKind::Claim
                | BrainProposalKind::Link
                | BrainProposalKind::Memory
        ) {
            if proposal.kind == BrainProposalKind::Memory {
                let memory = memory_record_for_proposal(&proposal);
                writer.upsert_memory_record(memory)?;
                let accepted_event = brain_memory_accepted_event(&proposal)?;
                writer.append_event(&accepted_event)?;
            } else {
                writer.apply_accepted_proposal(&proposal)?;
            }
            writer.append_event(&brain_graph_mutation_applied_event(&proposal)?)?;
        } else {
            let memory = memory_record_for_proposal(&proposal);
            writer.upsert_memory_record(memory)?;
            let accepted_event = brain_memory_accepted_event(&proposal)?;
            writer.append_event(&accepted_event)?;
        }
    }

    Ok(ProposeBrainUpdateResponseData {
        proposal,
        event,
        proposal_path: proposal_path.display().to_string(),
    })
}

pub(crate) fn handle_list_brain_review_items(
    request: ListBrainReviewItemsRequest,
) -> Result<ListBrainReviewItemsResponseData> {
    let root = resolve_brain_workspace_root(&request.scope)?;
    Ok(ListBrainReviewItemsResponseData {
        items: list_pending_brain_review_items(&root, &request.scope.workspace_id)?,
    })
}
pub(crate) fn handle_resolve_brain_review_item(
    request: ResolveBrainReviewItemRequest,
) -> Result<ResolveBrainReviewItemResponseData> {
    let root = resolve_brain_workspace_root(&request.scope)?;
    let writer = BrainWorkspaceWriter::open(root)?;
    let proposal_path = writer.proposal_path(&request.proposal_id);
    let mut proposal: BrainUpdateProposal = read_json_artifact(&proposal_path)
        .with_context(|| format!("failed reading review proposal {}", request.proposal_id))?;
    if proposal.workspace_id != request.scope.workspace_id {
        bail!(
            "proposal workspace_id {} does not match requested workspace {}",
            proposal.workspace_id,
            request.scope.workspace_id
        );
    }
    if !is_reviewable_brain_proposal(&proposal) {
        bail!(
            "proposal {} is not a pending claim or link review item",
            proposal.proposal_id
        );
    }
    proposal.status = match request.decision {
        BrainReviewDecision::Accept => BrainProposalStatus::Accepted,
        BrainReviewDecision::Reject => BrainProposalStatus::Rejected,
    };
    if request.decision == BrainReviewDecision::Accept {
        writer.apply_accepted_proposal(&proposal)?;
    }
    let proposal_path = writer.write_proposal(&proposal)?;
    let event = brain_review_resolved_event(&proposal, &request)?;
    writer.append_event(&event)?;
    Ok(ResolveBrainReviewItemResponseData {
        proposal,
        event,
        proposal_path: proposal_path.display().to_string(),
    })
}

pub(crate) fn list_pending_brain_review_items(
    root: &Path,
    workspace_id: &str,
) -> Result<Vec<BrainReviewItem>> {
    let mut items = read_brain_update_proposals(root)?
        .into_iter()
        .filter(|(proposal, _)| proposal.workspace_id == workspace_id)
        .filter(|(proposal, _)| is_reviewable_brain_proposal(proposal))
        .map(|(proposal, path)| brain_review_item_for_proposal(&proposal, &path))
        .collect::<Vec<_>>();
    items.sort_by(|left, right| {
        right
            .created_at
            .cmp(&left.created_at)
            .then_with(|| left.proposal_id.cmp(&right.proposal_id))
    });
    Ok(items)
}

pub(crate) fn read_brain_update_proposals(
    root: &Path,
) -> Result<Vec<(BrainUpdateProposal, PathBuf)>> {
    let proposals_dir = root.join("reviews/proposed-updates");
    if !proposals_dir.exists() {
        return Ok(Vec::new());
    }
    let mut proposals = Vec::new();
    for entry in fs::read_dir(&proposals_dir)
        .with_context(|| format!("failed reading {}", proposals_dir.display()))?
    {
        let entry = entry.with_context(|| format!("failed reading {}", proposals_dir.display()))?;
        let path = entry.path();
        if path.extension().and_then(|value| value.to_str()) != Some("json") {
            continue;
        }
        proposals.push((read_json_artifact(&path)?, path));
    }
    Ok(proposals)
}

pub(crate) fn is_reviewable_brain_proposal(proposal: &BrainUpdateProposal) -> bool {
    proposal.status == BrainProposalStatus::PendingReview
        && matches!(
            proposal.kind,
            BrainProposalKind::Node
                | BrainProposalKind::Claim
                | BrainProposalKind::Link
                | BrainProposalKind::WikiPage
        )
}

pub(crate) fn brain_review_item_for_proposal(
    proposal: &BrainUpdateProposal,
    path: &Path,
) -> BrainReviewItem {
    BrainReviewItem {
        review_id: proposal.proposal_id.clone(),
        proposal_id: proposal.proposal_id.clone(),
        workspace_id: proposal.workspace_id.clone(),
        kind: proposal.kind,
        status: proposal.status,
        title: proposal.title.clone(),
        body: proposal.body.clone(),
        proposal_path: path.display().to_string(),
        source_refs: proposal.source_refs.clone(),
        node_refs: proposal.node_refs.clone(),
        evidence_refs: proposal.evidence_refs.clone(),
        created_at: proposal.created_at,
    }
}
