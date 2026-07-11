use super::origin::*;
use super::*;

#[derive(Debug, Clone, Default)]
pub(crate) struct ProtectedMaterializedRecordKeys {
    nodes: BTreeSet<String>,
    evidence: BTreeSet<String>,
    relations: BTreeSet<String>,
    claims: BTreeSet<String>,
    memories: BTreeSet<String>,
    wiki_page_ids: BTreeSet<String>,
    wiki_paths: BTreeSet<String>,
}

impl ProtectedMaterializedRecordKeys {
    pub(super) fn from_snapshot(
        snapshot: &BrainRepoSnapshot,
        previous_origins: &MaterializedRecordOrigins,
    ) -> Self {
        let valid_source_ids = snapshot
            .sources
            .iter()
            .map(|source| source.source_id.clone())
            .collect::<BTreeSet<_>>();
        let valid_node_ids = snapshot
            .nodes
            .iter()
            .map(|node| node.node_id.clone())
            .collect::<BTreeSet<_>>();
        let valid_evidence_ids = snapshot
            .evidence
            .iter()
            .map(|evidence| evidence.id.clone())
            .collect::<BTreeSet<_>>();
        let protected_relations = snapshot
            .relations
            .iter()
            .filter(|relation| {
                relation_refs_are_current(relation, &valid_node_ids, &valid_evidence_ids)
                    && !record_has_workspace_linking_origin(
                        &previous_origins.relations,
                        &relation.relation_id,
                    )
            })
            .collect::<Vec<_>>();
        let protected_claims = snapshot
            .claims
            .iter()
            .filter(|claim| {
                claim_refs_are_current(
                    claim,
                    &valid_source_ids,
                    &valid_evidence_ids,
                    &valid_node_ids,
                ) && !record_has_workspace_linking_origin(&previous_origins.claims, &claim.claim_id)
            })
            .collect::<Vec<_>>();
        let protected_memories = snapshot
            .memories
            .iter()
            .filter(|memory| {
                source_and_evidence_refs_are_current(
                    &memory.source_refs,
                    &memory.evidence_refs,
                    &valid_source_ids,
                    &valid_evidence_ids,
                ) && !record_has_workspace_linking_origin(
                    &previous_origins.memories,
                    &memory.memory_id,
                )
            })
            .collect::<Vec<_>>();
        let protected_wiki_pages = snapshot
            .wiki_pages
            .iter()
            .filter(|page| {
                wiki_page_refs_are_current(
                    page,
                    &valid_source_ids,
                    &valid_evidence_ids,
                    &valid_node_ids,
                ) && !record_has_workspace_linking_origin(
                    &previous_origins.wiki_pages_by_id,
                    &page.page_id,
                ) && !record_has_workspace_linking_origin(
                    &previous_origins.wiki_pages_by_path,
                    &page.path,
                )
            })
            .collect::<Vec<_>>();
        let mut nodes = BTreeSet::new();
        let mut evidence = BTreeSet::new();
        for relation in &protected_relations {
            nodes.insert(relation.source_node_id.clone());
            nodes.insert(relation.target_node_id.clone());
            evidence.extend(relation.evidence_ids.iter().cloned());
        }
        for claim in &protected_claims {
            nodes.extend(claim.topic_refs.iter().cloned());
            evidence.extend(claim.evidence_refs.iter().cloned());
        }
        for memory in &protected_memories {
            evidence.extend(memory.evidence_refs.iter().cloned());
        }
        for page in &protected_wiki_pages {
            nodes.extend(page.node_refs.iter().cloned());
            evidence.extend(page.evidence_refs.iter().cloned());
        }
        Self {
            nodes,
            evidence,
            relations: snapshot
                .relations
                .iter()
                .filter(|relation| protected_relations.contains(relation))
                .map(|relation| relation.relation_id.clone())
                .collect(),
            claims: snapshot
                .claims
                .iter()
                .filter(|claim| protected_claims.contains(claim))
                .map(|claim| claim.claim_id.clone())
                .collect(),
            memories: snapshot
                .memories
                .iter()
                .filter(|memory| protected_memories.contains(memory))
                .map(|memory| memory.memory_id.clone())
                .collect(),
            wiki_page_ids: snapshot
                .wiki_pages
                .iter()
                .filter(|page| protected_wiki_pages.contains(page))
                .map(|page| page.page_id.clone())
                .collect(),
            wiki_paths: snapshot
                .wiki_pages
                .iter()
                .filter(|page| protected_wiki_pages.contains(page))
                .map(|page| page.path.clone())
                .collect(),
        }
    }
}

pub(super) fn discard_stale_workspace_linking_memory_collisions(
    existing_memories: Vec<MemoryRecord>,
    generated_snapshot: &BrainRepoSnapshot,
    origins: &MaterializedRecordOrigins,
) -> Vec<MemoryRecord> {
    let generated_memory_ids = generated_snapshot
        .memories
        .iter()
        .map(|memory| memory.memory_id.as_str())
        .collect::<BTreeSet<_>>();
    let valid_source_ids = generated_snapshot
        .sources
        .iter()
        .map(|source| source.source_id.clone())
        .collect::<BTreeSet<_>>();
    let valid_evidence_ids = generated_snapshot
        .evidence
        .iter()
        .map(|evidence| evidence.id.clone())
        .collect::<BTreeSet<_>>();
    existing_memories
        .into_iter()
        .filter(|memory| {
            if !generated_memory_ids.contains(memory.memory_id.as_str()) {
                return true;
            }
            let invalid_existing_refs = !source_and_evidence_refs_are_current(
                &memory.source_refs,
                &memory.evidence_refs,
                &valid_source_ids,
                &valid_evidence_ids,
            );
            !(record_has_workspace_linking_origin(&origins.memories, &memory.memory_id)
                || invalid_existing_refs)
        })
        .collect()
}

pub(super) fn clear_replayable_provider_overlay_records(
    snapshot: &mut BrainRepoSnapshot,
    replayable_events: &[BrainEvent],
    previous_origins: &MaterializedRecordOrigins,
    protected_current_records: &ProtectedMaterializedRecordKeys,
) {
    if replayable_events.is_empty() {
        return;
    }
    let target_node_ids = replayable_events
        .iter()
        .filter(|event| event.operation_type.as_deref() != Some("workspace_linking"))
        .flat_map(|event| event.target_node_ids.iter().cloned())
        .collect::<BTreeSet<_>>();
    let target_edge_ids = replayable_events
        .iter()
        .filter(|event| event.operation_type.as_deref() != Some("workspace_linking"))
        .flat_map(|event| event.target_edge_ids.iter().cloned())
        .collect::<BTreeSet<_>>();
    let target_claim_ids = replayable_events
        .iter()
        .filter(|event| event.operation_type.as_deref() != Some("workspace_linking"))
        .flat_map(|event| event.target_claim_ids.iter().cloned())
        .collect::<BTreeSet<_>>();
    let target_memory_ids = replayable_events
        .iter()
        .filter(|event| event.operation_type.as_deref() != Some("workspace_linking"))
        .flat_map(|event| event.target_memory_ids.iter().cloned())
        .collect::<BTreeSet<_>>();
    let provider_overlay_refs =
        replayable_events
            .iter()
            .fold(ProviderOverlayRefs::default(), |mut refs, event| {
                refs.extend_from_event(event);
                refs
            });
    let deterministic_source_evidence_ids = snapshot
        .nodes
        .iter()
        .filter(|node| node.kind == BrainNodeKind::Source)
        .flat_map(|node| node.evidence_ids.iter().cloned())
        .collect::<BTreeSet<_>>();

    let node_invalidation_events =
        target_invalidation_events_by_id(replayable_events, |event| event.target_node_ids.iter());
    for node in &mut snapshot.nodes {
        if node.kind == BrainNodeKind::Source
            || protected_current_records.nodes.contains(&node.node_id)
            || !target_node_ids.contains(&node.node_id)
            || node.valid_to.is_some()
        {
            continue;
        }
        if let Some((event_id, materialized_version)) = node_invalidation_events.get(&node.node_id)
        {
            node.valid_to = Some(*materialized_version);
            node.superseded_by = Some(event_id.clone());
        }
    }
    let relation_invalidation_events =
        target_invalidation_events_by_id(replayable_events, |event| event.target_edge_ids.iter());
    snapshot.relations.retain(|relation| {
        !record_has_workspace_linking_origin(&previous_origins.relations, &relation.relation_id)
            || protected_current_records
                .relations
                .contains(&relation.relation_id)
    });
    for relation in &mut snapshot.relations {
        let is_replayable_overlay = target_edge_ids.contains(&relation.relation_id)
            || provider_overlay_refs
                .relation_ids
                .contains(&relation.relation_id);
        if !is_replayable_overlay
            || protected_current_records
                .relations
                .contains(&relation.relation_id)
            || relation.valid_to.is_some()
        {
            continue;
        }
        if let Some((event_id, materialized_version)) =
            relation_invalidation_events.get(&relation.relation_id)
        {
            relation.valid_to = Some(*materialized_version);
            relation.superseded_by = Some(event_id.clone());
        }
    }
    snapshot.claims.retain(|claim| {
        !target_claim_ids.contains(&claim.claim_id)
            && !provider_overlay_refs.claim_ids.contains(&claim.claim_id)
            && !record_has_workspace_linking_origin(&previous_origins.claims, &claim.claim_id)
            || protected_current_records.claims.contains(&claim.claim_id)
    });
    snapshot.memories.retain(|memory| {
        !target_memory_ids.contains(&memory.memory_id)
            && !provider_overlay_refs.memory_ids.contains(&memory.memory_id)
            && !record_has_workspace_linking_origin(&previous_origins.memories, &memory.memory_id)
            || protected_current_records
                .memories
                .contains(&memory.memory_id)
    });
    let graph_record_evidence_ids = snapshot
        .nodes
        .iter()
        .flat_map(|node| node.evidence_ids.iter().cloned())
        .chain(
            snapshot
                .relations
                .iter()
                .flat_map(|relation| relation.evidence_ids.iter().cloned()),
        )
        .collect::<BTreeSet<_>>();
    snapshot.evidence.retain(|evidence| {
        protected_current_records.evidence.contains(&evidence.id)
            || !provider_overlay_refs.evidence_ids.contains(&evidence.id)
            || deterministic_source_evidence_ids.contains(&evidence.id)
            || graph_record_evidence_ids.contains(&evidence.id)
    });
    snapshot
        .entities
        .retain(|entity| !provider_overlay_refs.entity_ids.contains(&entity.entity_id));
    snapshot.wiki_pages.retain(|page| {
        !provider_overlay_refs.wiki_page_ids.contains(&page.page_id)
            && !provider_overlay_refs.wiki_paths.contains(&page.path)
            && !record_has_workspace_linking_origin(
                &previous_origins.wiki_pages_by_id,
                &page.page_id,
            )
            && !record_has_workspace_linking_origin(
                &previous_origins.wiki_pages_by_path,
                &page.path,
            )
            || protected_current_records
                .wiki_page_ids
                .contains(&page.page_id)
            || protected_current_records.wiki_paths.contains(&page.path)
    });
    snapshot.extractions.retain(|extraction| {
        !provider_overlay_refs
            .extraction_artifact_ids
            .contains(&extraction.artifact_id)
    });
}

fn target_invalidation_events_by_id<'a, I>(
    replayable_events: &'a [BrainEvent],
    target_ids: impl Fn(&'a BrainEvent) -> I,
) -> BTreeMap<String, (String, u64)>
where
    I: Iterator<Item = &'a String>,
{
    let mut by_id = BTreeMap::<String, (String, u64)>::new();
    for event in replayable_events {
        if event.operation_type.as_deref() == Some("workspace_linking") {
            continue;
        }
        let materialized_version = event
            .causality
            .materialized_version
            .unwrap_or(event.created_at);
        for target_id in target_ids(event) {
            let entry = by_id
                .entry(target_id.clone())
                .or_insert_with(|| (event.event_id.clone(), materialized_version));
            if materialized_version >= entry.1 {
                *entry = (event.event_id.clone(), materialized_version);
            }
        }
    }
    by_id
}

#[derive(Debug, Default)]
struct ProviderOverlayRefs {
    evidence_ids: BTreeSet<String>,
    relation_ids: BTreeSet<String>,
    claim_ids: BTreeSet<String>,
    memory_ids: BTreeSet<String>,
    entity_ids: BTreeSet<String>,
    wiki_page_ids: BTreeSet<String>,
    wiki_paths: BTreeSet<String>,
    extraction_artifact_ids: BTreeSet<String>,
}

impl ProviderOverlayRefs {
    fn extend_from_event(&mut self, event: &BrainEvent) {
        let is_workspace_linking = event.operation_type.as_deref() == Some("workspace_linking");
        let Ok(payload) =
            serde_json::from_str::<MaterializedGraphEventPayload>(&event.payload_json)
        else {
            return;
        };
        let Some(materialized_graph) = payload.materialized_graph else {
            return;
        };
        if !is_workspace_linking {
            self.evidence_ids.extend(
                materialized_graph
                    .evidence
                    .into_iter()
                    .map(|evidence| evidence.id),
            );
        }
        if !is_workspace_linking {
            self.relation_ids.extend(
                materialized_graph
                    .relations
                    .into_iter()
                    .map(|relation| relation.relation_id),
            );
            self.claim_ids.extend(
                materialized_graph
                    .claims
                    .into_iter()
                    .map(|claim| claim.claim_id),
            );
            self.memory_ids.extend(
                materialized_graph
                    .memories
                    .into_iter()
                    .map(|memory| memory.memory_id),
            );
        }
        if !is_workspace_linking {
            self.entity_ids.extend(
                materialized_graph
                    .entities
                    .into_iter()
                    .map(|entity| entity.entity_id),
            );
        }
        for page in materialized_graph.wiki_pages {
            if !is_workspace_linking {
                self.wiki_page_ids.insert(page.page_id);
                self.wiki_paths.insert(page.path);
            }
        }
        if !is_workspace_linking {
            self.extraction_artifact_ids.extend(
                materialized_graph
                    .extractions
                    .into_iter()
                    .map(|extraction| extraction.artifact_id),
            );
        }
    }
}
