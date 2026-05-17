use super::cleanup::*;
use super::origin::*;
use super::*;

struct MaterializedOverlayReplayContext<'a> {
    valid_source_ids: &'a BTreeSet<String>,
    valid_evidence_ids: &'a BTreeSet<String>,
    valid_node_ids: &'a BTreeSet<String>,
    evidence_source_ids: &'a BTreeMap<String, String>,
    require_cross_source: bool,
    event: &'a BrainEvent,
}

pub(super) fn is_replayable_materialized_graph_event(event: &BrainEvent) -> bool {
    event.event_type == BrainEventKind::GraphMaterialized
        && matches!(
            event.operation_type.as_deref(),
            Some("full_workspace_rebuild" | "source_graph_build" | "workspace_linking")
        )
        && event.policy_result == "materialized"
}

pub(super) fn replay_preserved_materialized_graph_events(
    snapshot: &mut BrainRepoSnapshot,
    previous_origins: &MaterializedRecordOrigins,
    protected_current_records: &ProtectedMaterializedRecordKeys,
) -> Result<MaterializedRecordOrigins> {
    let replayable_events = snapshot
        .events
        .iter()
        .filter(|event| is_replayable_materialized_graph_event(event))
        .cloned()
        .collect::<Vec<_>>();
    replay_materialized_graph_overlay_events(
        snapshot,
        &replayable_events,
        previous_origins,
        protected_current_records,
    )
}

pub(crate) fn replay_materialized_graph_overlay_events(
    snapshot: &mut BrainRepoSnapshot,
    replayable_events: &[BrainEvent],
    previous_origins: &MaterializedRecordOrigins,
    protected_current_records: &ProtectedMaterializedRecordKeys,
) -> Result<MaterializedRecordOrigins> {
    let mut events = latest_replayable_materialized_graph_events(replayable_events);
    events.sort_by(|left, right| {
        left.causality
            .materialized_version
            .unwrap_or(left.created_at)
            .cmp(
                &right
                    .causality
                    .materialized_version
                    .unwrap_or(right.created_at),
            )
            .then_with(|| left.created_at.cmp(&right.created_at))
            .then_with(|| left.event_id.cmp(&right.event_id))
    });
    let mut origins = MaterializedRecordOrigins {
        schema_version: BRAIN_EVENT_SCHEMA_VERSION,
        ..Default::default()
    };
    clear_replayable_provider_overlay_records(
        snapshot,
        replayable_events,
        previous_origins,
        protected_current_records,
    );
    for event in events {
        let payload = serde_json::from_str::<MaterializedGraphEventPayload>(&event.payload_json)
            .with_context(|| {
                format!(
                    "failed decoding materialized graph event {}",
                    event.event_id
                )
            })?;
        if let Some(materialized_graph) = payload.materialized_graph {
            apply_filtered_materialized_graph_overlay(
                snapshot,
                materialized_graph,
                &event,
                &mut origins,
            );
        }
    }
    carry_forward_valid_workspace_linking_origins(snapshot, previous_origins, &mut origins);
    Ok(origins)
}

fn latest_replayable_materialized_graph_events(events: &[BrainEvent]) -> Vec<BrainEvent> {
    let mut latest_by_key = BTreeMap::<String, BrainEvent>::new();
    for event in events {
        let key = materialized_graph_replay_key(event);
        match latest_by_key.get(&key) {
            Some(existing) if replay_event_order_key(existing) >= replay_event_order_key(event) => {
            }
            _ => {
                latest_by_key.insert(key, event.clone());
            }
        }
    }
    latest_by_key.into_values().collect()
}

fn replay_event_order_key(event: &BrainEvent) -> (u64, u64, &str) {
    (
        event
            .causality
            .materialized_version
            .unwrap_or(event.created_at),
        event.created_at,
        event.event_id.as_str(),
    )
}

fn materialized_graph_replay_key(event: &BrainEvent) -> String {
    let operation_type = event
        .operation_type
        .as_deref()
        .unwrap_or("graph_materialized");
    if operation_type == "full_workspace_rebuild" {
        return format!(
            "{}:{:?}:{operation_type}:workspace",
            event.workspace_id, event.scope
        );
    }
    let mut source_ids = if event.causality.caused_by_source_ids.is_empty() {
        event.source_refs.clone()
    } else {
        event.causality.caused_by_source_ids.clone()
    };
    source_ids.sort();
    source_ids.dedup();
    format!(
        "{}:{:?}:{}:{}",
        event.workspace_id,
        event.scope,
        operation_type,
        source_ids.join("+")
    )
}

fn carry_forward_valid_workspace_linking_origins(
    snapshot: &BrainRepoSnapshot,
    previous_origins: &MaterializedRecordOrigins,
    origins: &mut MaterializedRecordOrigins,
) {
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
    let evidence_source_ids = snapshot
        .evidence
        .iter()
        .filter_map(|evidence| {
            evidence
                .source_id
                .as_ref()
                .map(|source_id| (evidence.id.clone(), source_id.clone()))
        })
        .collect::<BTreeMap<_, _>>();

    for relation in &snapshot.relations {
        let Some(origin) = previous_origins.relations.get(&relation.relation_id) else {
            continue;
        };
        if origin.is_workspace_linking()
            && relation_refs_are_current(relation, &valid_node_ids, &valid_evidence_ids)
            && has_cross_source_evidence_refs(&relation.evidence_ids, &evidence_source_ids)
        {
            origins
                .relations
                .entry(relation.relation_id.clone())
                .or_insert_with(|| origin.clone());
        }
    }
    for claim in &snapshot.claims {
        let Some(origin) = previous_origins.claims.get(&claim.claim_id) else {
            continue;
        };
        if origin.is_workspace_linking()
            && claim_refs_are_current(
                claim,
                &valid_source_ids,
                &valid_evidence_ids,
                &valid_node_ids,
            )
            && has_cross_source_refs(&claim.source_refs)
            && has_cross_source_evidence_refs(&claim.evidence_refs, &evidence_source_ids)
        {
            origins
                .claims
                .entry(claim.claim_id.clone())
                .or_insert_with(|| origin.clone());
        }
    }
    for memory in &snapshot.memories {
        let Some(origin) = previous_origins.memories.get(&memory.memory_id) else {
            continue;
        };
        if origin.is_workspace_linking()
            && source_and_evidence_refs_are_current(
                &memory.source_refs,
                &memory.evidence_refs,
                &valid_source_ids,
                &valid_evidence_ids,
            )
            && has_cross_source_refs(&memory.source_refs)
            && has_cross_source_evidence_refs(&memory.evidence_refs, &evidence_source_ids)
        {
            origins
                .memories
                .entry(memory.memory_id.clone())
                .or_insert_with(|| origin.clone());
        }
    }
    for page in &snapshot.wiki_pages {
        let origin = previous_origins
            .wiki_pages_by_id
            .get(&page.page_id)
            .or_else(|| previous_origins.wiki_pages_by_path.get(&page.path));
        let Some(origin) = origin else {
            continue;
        };
        if origin.is_workspace_linking()
            && wiki_page_refs_are_current(
                page,
                &valid_source_ids,
                &valid_evidence_ids,
                &valid_node_ids,
            )
            && has_cross_source_refs(&page.source_refs)
            && has_cross_source_evidence_refs(&page.evidence_refs, &evidence_source_ids)
        {
            origins
                .wiki_pages_by_id
                .entry(page.page_id.clone())
                .or_insert_with(|| origin.clone());
            origins
                .wiki_pages_by_path
                .entry(page.path.clone())
                .or_insert_with(|| origin.clone());
        }
    }
}

fn apply_filtered_materialized_graph_overlay(
    snapshot: &mut BrainRepoSnapshot,
    materialized_graph: MaterializedGraphPayload,
    event: &BrainEvent,
    origins: &mut MaterializedRecordOrigins,
) {
    let valid_source_ids = snapshot
        .sources
        .iter()
        .map(|source| source.source_id.clone())
        .collect::<BTreeSet<_>>();
    if valid_source_ids.is_empty() {
        return;
    }

    let require_cross_source_artifacts =
        event.operation_type.as_deref() == Some("workspace_linking");
    if !require_cross_source_artifacts {
        merge_filtered_evidence(snapshot, materialized_graph.evidence, &valid_source_ids);
    }
    let valid_evidence_ids = snapshot
        .evidence
        .iter()
        .map(|evidence| evidence.id.clone())
        .collect::<BTreeSet<_>>();
    let source_labels = snapshot
        .sources
        .iter()
        .map(|source| {
            (
                source.source_id.clone(),
                source_label_from_source_record(source),
            )
        })
        .collect::<BTreeMap<_, _>>();
    let evidence_by_source = snapshot
        .evidence
        .iter()
        .filter_map(|evidence| {
            evidence
                .source_id
                .as_ref()
                .map(|source_id| (source_id.clone(), evidence.id.clone()))
        })
        .fold(
            BTreeMap::<String, Vec<String>>::new(),
            |mut by_source, (source_id, evidence_id)| {
                by_source.entry(source_id).or_default().push(evidence_id);
                by_source
            },
        );
    if !require_cross_source_artifacts {
        merge_filtered_nodes(
            snapshot,
            materialized_graph.nodes,
            &valid_source_ids,
            &valid_evidence_ids,
            &source_labels,
            &evidence_by_source,
        );
    }
    let valid_node_ids = snapshot
        .nodes
        .iter()
        .map(|node| node.node_id.clone())
        .collect::<BTreeSet<_>>();
    let evidence_source_ids = snapshot
        .evidence
        .iter()
        .filter_map(|evidence| {
            evidence
                .source_id
                .as_ref()
                .map(|source_id| (evidence.id.clone(), source_id.clone()))
        })
        .collect::<BTreeMap<_, _>>();
    let replay_context = MaterializedOverlayReplayContext {
        valid_source_ids: &valid_source_ids,
        valid_evidence_ids: &valid_evidence_ids,
        valid_node_ids: &valid_node_ids,
        evidence_source_ids: &evidence_source_ids,
        require_cross_source: require_cross_source_artifacts,
        event,
    };
    merge_filtered_relations(
        snapshot,
        materialized_graph.relations,
        &replay_context,
        origins,
    );
    merge_filtered_claims(
        snapshot,
        materialized_graph.claims,
        &replay_context,
        origins,
    );
    merge_filtered_memories(
        snapshot,
        materialized_graph.memories,
        &replay_context,
        origins,
    );
    if !require_cross_source_artifacts {
        merge_filtered_entities(
            snapshot,
            materialized_graph.entities,
            &valid_source_ids,
            &valid_evidence_ids,
        );
        merge_filtered_extractions(snapshot, materialized_graph.extractions, &valid_source_ids);
    }
    merge_filtered_wiki_pages(
        snapshot,
        materialized_graph.wiki_pages,
        &replay_context,
        origins,
    );
    snapshot.generated_at = snapshot.generated_at.max(
        materialized_graph
            .generated_at
            .or(event.causality.materialized_version)
            .unwrap_or(event.created_at),
    );
}

fn merge_filtered_evidence(
    snapshot: &mut BrainRepoSnapshot,
    evidence: Vec<EvidenceRef>,
    valid_source_ids: &BTreeSet<String>,
) {
    let mut by_id = snapshot
        .evidence
        .iter()
        .cloned()
        .map(|evidence| (evidence.id.clone(), evidence))
        .collect::<BTreeMap<_, _>>();
    for evidence in evidence {
        if evidence
            .source_id
            .as_ref()
            .is_some_and(|source_id| valid_source_ids.contains(source_id))
        {
            by_id.insert(evidence.id.clone(), evidence);
        }
    }
    snapshot.evidence = by_id.into_values().collect();
}

fn merge_filtered_nodes(
    snapshot: &mut BrainRepoSnapshot,
    nodes: Vec<BrainNodeRecord>,
    valid_source_ids: &BTreeSet<String>,
    valid_evidence_ids: &BTreeSet<String>,
    source_labels: &BTreeMap<String, String>,
    evidence_by_source: &BTreeMap<String, Vec<String>>,
) {
    let mut by_id = snapshot
        .nodes
        .iter()
        .cloned()
        .map(|node| (node.node_id.clone(), node))
        .collect::<BTreeMap<_, _>>();
    for mut node in nodes {
        node.source_ids
            .retain(|source_id| valid_source_ids.contains(source_id));
        node.evidence_ids
            .retain(|evidence_id| valid_evidence_ids.contains(evidence_id));
        if node.kind == BrainNodeKind::Source {
            let source_id_from_node = node.node_id.strip_prefix("source:");
            if node.source_ids.is_empty()
                && source_id_from_node.is_some_and(|source_id| valid_source_ids.contains(source_id))
            {
                node.source_ids
                    .push(source_id_from_node.unwrap().to_string());
            }
            if node.source_ids.is_empty() {
                continue;
            }
            let source_id = node.source_ids[0].clone();
            if let Some(label) = source_labels.get(&source_id) {
                node.label = label.clone();
            }
            if let Some(evidence_ids) = evidence_by_source.get(&source_id) {
                node.evidence_ids = evidence_ids.clone();
            }
        } else if node.source_ids.is_empty() && node.evidence_ids.is_empty() {
            continue;
        }
        node.scope = BrainScope::Project;
        normalize_materialized_node_label(&mut node);
        by_id.insert(node.node_id.clone(), node);
    }
    snapshot.nodes = by_id.into_values().collect();
}

fn normalize_materialized_node_label(node: &mut BrainNodeRecord) {
    if !is_machine_materialized_label(&node.label, &node.node_id) {
        return;
    }
    if let Some(alias) = node
        .aliases
        .iter()
        .map(|alias| alias.trim().to_string())
        .find(|alias| !alias.is_empty() && !is_machine_materialized_label(alias, &node.node_id))
    {
        node.label = alias;
    } else {
        node.label = readable_label_from_node_id(&node.node_id);
    }
}

fn is_machine_materialized_label(label: &str, node_id: &str) -> bool {
    let label = label.trim();
    label.is_empty()
        || label == node_id
        || label.starts_with("source-")
        || label.starts_with("concept:")
        || label.starts_with("source:")
}

fn readable_label_from_node_id(node_id: &str) -> String {
    let suffix = node_id
        .rsplit_once(':')
        .map(|(_, suffix)| suffix)
        .unwrap_or(node_id);
    suffix
        .split(['-', '_'])
        .filter(|part| !part.trim().is_empty())
        .map(|part| {
            let mut chars = part.chars();
            match chars.next() {
                Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
                None => String::new(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn source_label_from_source_record(source: &SourceRecord) -> String {
    Path::new(&source.original_path)
        .file_name()
        .or_else(|| Path::new(&source.source_path).file_name())
        .map(|value| value.to_string_lossy().into_owned())
        .unwrap_or_else(|| source.source_id.clone())
}

fn merge_filtered_relations(
    snapshot: &mut BrainRepoSnapshot,
    relations: Vec<BrainRelationRecord>,
    replay_context: &MaterializedOverlayReplayContext<'_>,
    origins: &mut MaterializedRecordOrigins,
) {
    let mut by_id = snapshot
        .relations
        .iter()
        .cloned()
        .map(|relation| (relation.relation_id.clone(), relation))
        .collect::<BTreeMap<_, _>>();
    for mut relation in relations {
        if !replay_context
            .valid_node_ids
            .contains(&relation.source_node_id)
            || !replay_context
                .valid_node_ids
                .contains(&relation.target_node_id)
        {
            continue;
        }
        relation
            .evidence_ids
            .retain(|evidence_id| replay_context.valid_evidence_ids.contains(evidence_id));
        if relation.evidence_ids.is_empty() {
            continue;
        }
        if replay_context.require_cross_source
            && !has_cross_source_evidence_refs(
                &relation.evidence_ids,
                replay_context.evidence_source_ids,
            )
        {
            continue;
        }
        if replay_context.require_cross_source && by_id.contains_key(&relation.relation_id) {
            continue;
        }
        let relation_id = relation.relation_id.clone();
        by_id.insert(relation_id.clone(), relation);
        origins.relations.insert(
            relation_id,
            MaterializedRecordOrigin::from_event(replay_context.event),
        );
    }
    snapshot.relations = by_id.into_values().collect();
}

fn merge_filtered_claims(
    snapshot: &mut BrainRepoSnapshot,
    claims: Vec<ClaimRecord>,
    replay_context: &MaterializedOverlayReplayContext<'_>,
    origins: &mut MaterializedRecordOrigins,
) {
    let mut by_id = snapshot
        .claims
        .iter()
        .cloned()
        .map(|claim| (claim.claim_id.clone(), claim))
        .collect::<BTreeMap<_, _>>();
    for mut claim in claims {
        claim
            .source_refs
            .retain(|source_id| replay_context.valid_source_ids.contains(source_id));
        claim
            .evidence_refs
            .retain(|evidence_id| replay_context.valid_evidence_ids.contains(evidence_id));
        claim
            .topic_refs
            .retain(|node_id| replay_context.valid_node_ids.contains(node_id));
        if claim.source_refs.is_empty() && claim.evidence_refs.is_empty() {
            continue;
        }
        if claim.topic_refs.is_empty() {
            continue;
        }
        if replay_context.require_cross_source
            && (!has_cross_source_refs(&claim.source_refs)
                || !has_cross_source_evidence_refs(
                    &claim.evidence_refs,
                    replay_context.evidence_source_ids,
                ))
        {
            continue;
        }
        if replay_context.require_cross_source && by_id.contains_key(&claim.claim_id) {
            continue;
        }
        let claim_id = claim.claim_id.clone();
        by_id.insert(claim_id.clone(), claim);
        origins.claims.insert(
            claim_id,
            MaterializedRecordOrigin::from_event(replay_context.event),
        );
    }
    snapshot.claims = by_id.into_values().collect();
}

fn merge_filtered_memories(
    snapshot: &mut BrainRepoSnapshot,
    memories: Vec<MemoryRecord>,
    replay_context: &MaterializedOverlayReplayContext<'_>,
    origins: &mut MaterializedRecordOrigins,
) {
    let mut by_id = snapshot
        .memories
        .iter()
        .cloned()
        .map(|memory| (memory.memory_id.clone(), memory))
        .collect::<BTreeMap<_, _>>();
    for mut memory in memories {
        memory
            .source_refs
            .retain(|source_id| replay_context.valid_source_ids.contains(source_id));
        memory
            .evidence_refs
            .retain(|evidence_id| replay_context.valid_evidence_ids.contains(evidence_id));
        if memory.source_refs.is_empty() && memory.evidence_refs.is_empty() {
            continue;
        }
        if replay_context.require_cross_source
            && (!has_cross_source_refs(&memory.source_refs)
                || !has_cross_source_evidence_refs(
                    &memory.evidence_refs,
                    replay_context.evidence_source_ids,
                ))
        {
            continue;
        }
        if replay_context.require_cross_source && by_id.contains_key(&memory.memory_id) {
            continue;
        }
        let memory_id = memory.memory_id.clone();
        by_id.insert(memory_id.clone(), memory);
        origins.memories.insert(
            memory_id,
            MaterializedRecordOrigin::from_event(replay_context.event),
        );
    }
    snapshot.memories = by_id.into_values().collect();
}

fn merge_filtered_entities(
    snapshot: &mut BrainRepoSnapshot,
    entities: Vec<EntityRecord>,
    valid_source_ids: &BTreeSet<String>,
    valid_evidence_ids: &BTreeSet<String>,
) {
    let mut by_id = snapshot
        .entities
        .iter()
        .cloned()
        .map(|entity| (entity.entity_id.clone(), entity))
        .collect::<BTreeMap<_, _>>();
    for mut entity in entities {
        entity
            .source_refs
            .retain(|source_id| valid_source_ids.contains(source_id));
        entity
            .evidence_refs
            .retain(|evidence_id| valid_evidence_ids.contains(evidence_id));
        if entity.source_refs.is_empty() && entity.evidence_refs.is_empty() {
            continue;
        }
        by_id.insert(entity.entity_id.clone(), entity);
    }
    snapshot.entities = by_id.into_values().collect();
}

fn merge_filtered_extractions(
    snapshot: &mut BrainRepoSnapshot,
    extractions: Vec<StructuredExtractionArtifact>,
    valid_source_ids: &BTreeSet<String>,
) {
    let mut by_id = snapshot
        .extractions
        .iter()
        .cloned()
        .map(|extraction| (extraction.artifact_id.clone(), extraction))
        .collect::<BTreeMap<_, _>>();
    for extraction in extractions {
        if valid_source_ids.contains(&extraction.source_id) {
            by_id.insert(extraction.artifact_id.clone(), extraction);
        }
    }
    snapshot.extractions = by_id.into_values().collect();
}

fn merge_filtered_wiki_pages(
    snapshot: &mut BrainRepoSnapshot,
    pages: Vec<WikiPage>,
    replay_context: &MaterializedOverlayReplayContext<'_>,
    origins: &mut MaterializedRecordOrigins,
) {
    let mut by_path = snapshot
        .wiki_pages
        .iter()
        .cloned()
        .map(|page| (page.path.clone(), page))
        .collect::<BTreeMap<_, _>>();
    for mut page in pages {
        page.source_refs
            .retain(|source_id| replay_context.valid_source_ids.contains(source_id));
        page.evidence_refs
            .retain(|evidence_id| replay_context.valid_evidence_ids.contains(evidence_id));
        page.node_refs
            .retain(|node_id| replay_context.valid_node_ids.contains(node_id));
        if page.path.starts_with("wiki/sources/")
            && !page
                .source_refs
                .iter()
                .any(|source_id| replay_context.valid_source_ids.contains(source_id))
        {
            continue;
        }
        if page.path.starts_with("wiki/topics/") && page.node_refs.is_empty() {
            continue;
        }
        if replay_context.require_cross_source
            && (!has_cross_source_refs(&page.source_refs)
                || !has_cross_source_evidence_refs(
                    &page.evidence_refs,
                    replay_context.evidence_source_ids,
                ))
        {
            continue;
        }
        if replay_context.require_cross_source
            && (by_path.contains_key(&page.path)
                || by_path
                    .values()
                    .any(|existing| existing.page_id == page.page_id))
        {
            continue;
        }
        let page_id = page.page_id.clone();
        let path = page.path.clone();
        by_path.insert(path.clone(), page);
        let origin = MaterializedRecordOrigin::from_event(replay_context.event);
        origins.wiki_pages_by_id.insert(page_id, origin.clone());
        origins.wiki_pages_by_path.insert(path, origin);
    }
    snapshot.wiki_pages = by_path.into_values().collect();
}

fn has_cross_source_refs(source_refs: &[String]) -> bool {
    source_refs
        .iter()
        .map(|source_ref| source_ref.trim())
        .filter(|source_ref| !source_ref.is_empty())
        .collect::<BTreeSet<_>>()
        .len()
        >= 2
}

fn has_cross_source_evidence_refs(
    evidence_refs: &[String],
    evidence_source_ids: &BTreeMap<String, String>,
) -> bool {
    evidence_refs
        .iter()
        .filter_map(|evidence_ref| evidence_source_ids.get(evidence_ref))
        .collect::<BTreeSet<_>>()
        .len()
        >= 2
}
