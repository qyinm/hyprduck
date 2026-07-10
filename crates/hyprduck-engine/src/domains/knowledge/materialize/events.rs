use super::*;

pub(crate) fn merge_preserved_brain_events(
    mut materialized_events: Vec<BrainEvent>,
    existing_events: &[BrainEvent],
) -> Vec<BrainEvent> {
    let mut seen = materialized_events
        .iter()
        .map(|event| event.event_id.clone())
        .collect::<BTreeSet<_>>();
    for event in existing_events {
        if (is_preserved_brain_event(event.event_type)
            || is_replayable_materialized_graph_event(event))
            && seen.insert(event.event_id.clone())
        {
            materialized_events.push(event.clone());
        }
    }
    materialized_events.sort_by(|left, right| {
        left.created_at
            .cmp(&right.created_at)
            .then_with(|| left.event_id.cmp(&right.event_id))
    });
    materialized_events
}

pub(crate) fn is_preserved_brain_event(event_type: BrainEventKind) -> bool {
    matches!(
        event_type,
        BrainEventKind::SourceIngestQueued
            | BrainEventKind::SourceCompiled
            | BrainEventKind::ObservationAppended
            | BrainEventKind::MemoryAccepted
            | BrainEventKind::BrainMaintenanceRun
    )
}

pub(crate) fn refresh_current_materialized_events(snapshot: &mut BrainRepoSnapshot) -> Result<()> {
    let generated_at = unix_timestamp_seconds().max(snapshot.generated_at);
    snapshot.generated_at = generated_at;
    let graph_event_id = format!(
        "evt-{}-graph-materialized-{generated_at}",
        snapshot.workspace_id
    );
    let wiki_event_id = format!(
        "evt-{}-wiki-materialized-{generated_at}",
        snapshot.workspace_id
    );
    snapshot.events.retain(|event| {
        !((event.event_type == BrainEventKind::GraphMaterialized
            && event.operation_type.as_deref() == Some("graph_materialized"))
            || (event.event_type == BrainEventKind::WikiMaterialized
                && event.operation_type.as_deref() == Some("wiki_materialized")))
    });
    snapshot.events.push(final_graph_materialized_event(
        snapshot,
        &graph_event_id,
        generated_at,
    )?);
    snapshot.events.push(final_wiki_materialized_event(
        snapshot,
        &wiki_event_id,
        generated_at,
    )?);
    snapshot.events.sort_by(|left, right| {
        left.created_at
            .cmp(&right.created_at)
            .then_with(|| left.event_id.cmp(&right.event_id))
    });
    Ok(())
}

fn final_graph_materialized_event(
    snapshot: &BrainRepoSnapshot,
    event_id: &str,
    generated_at: u64,
) -> Result<BrainEvent> {
    Ok(BrainEvent {
        event_id: event_id.to_string(),
        schema_version: BRAIN_EVENT_SCHEMA_VERSION,
        workspace_id: snapshot.workspace_id.clone(),
        scope: BrainScope::Project,
        event_type: BrainEventKind::GraphMaterialized,
        operation_type: Some("graph_materialized".into()),
        actor: BrainActor {
            actor_type: BrainActorType::System,
            actor_id: "hyprduck-engine".into(),
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
        payload_json: materialized_graph_event_payload_json(
            generated_at,
            &snapshot.sources,
            &snapshot.nodes,
            &snapshot.relations,
            &snapshot.evidence,
            &snapshot.memories,
            &snapshot.wiki_pages,
            &snapshot.entities,
            &snapshot.claims,
            &snapshot.extractions,
        )?,
        causality: BrainEventCausality {
            caused_by_source_ids: snapshot
                .sources
                .iter()
                .map(|source| source.source_id.clone())
                .collect(),
            snapshot_id: Some(format!("snapshot-{}-{generated_at}", snapshot.workspace_id)),
            materialized_version: Some(generated_at),
            ..Default::default()
        },
        confidence: None,
        policy_result: "materialized".into(),
        created_at: generated_at,
    })
}

fn final_wiki_materialized_event(
    snapshot: &BrainRepoSnapshot,
    event_id: &str,
    generated_at: u64,
) -> Result<BrainEvent> {
    Ok(BrainEvent {
        event_id: event_id.to_string(),
        schema_version: BRAIN_EVENT_SCHEMA_VERSION,
        workspace_id: snapshot.workspace_id.clone(),
        scope: BrainScope::Project,
        event_type: BrainEventKind::WikiMaterialized,
        operation_type: Some("wiki_materialized".into()),
        actor: BrainActor {
            actor_type: BrainActorType::System,
            actor_id: "hyprduck-engine".into(),
        },
        source_refs: Vec::new(),
        source_markdown_refs: Vec::new(),
        node_refs: snapshot
            .wiki_pages
            .iter()
            .flat_map(|page| page.node_refs.clone())
            .collect(),
        relation_refs: Vec::new(),
        claim_refs: Vec::new(),
        memory_refs: Vec::new(),
        target_node_ids: snapshot
            .wiki_pages
            .iter()
            .flat_map(|page| page.node_refs.clone())
            .collect(),
        target_edge_ids: Vec::new(),
        target_claim_ids: Vec::new(),
        target_memory_ids: Vec::new(),
        evidence_refs: snapshot
            .wiki_pages
            .iter()
            .flat_map(|page| page.evidence_refs.clone())
            .collect(),
        payload_json: format!("{{\"pageCount\":{}}}", snapshot.wiki_pages.len()),
        causality: BrainEventCausality {
            snapshot_id: Some(format!("snapshot-{}-{generated_at}", snapshot.workspace_id)),
            materialized_version: Some(generated_at),
            ..Default::default()
        },
        confidence: None,
        policy_result: "materialized".into(),
        created_at: generated_at,
    })
}

