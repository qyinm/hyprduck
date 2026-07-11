use super::*;

pub(crate) fn merge_matching_claim_records(claims: Vec<ClaimRecord>) -> Vec<ClaimRecord> {
    let mut merged = BTreeMap::<String, ClaimRecord>::new();
    for claim in claims {
        match merged.get_mut(&claim.claim_id) {
            Some(existing) => merge_claim_record(existing, claim),
            None => {
                merged.insert(claim.claim_id.clone(), claim);
            }
        }
    }
    merged.into_values().collect()
}

pub(crate) fn merge_claim_record(existing: &mut ClaimRecord, incoming: ClaimRecord) {
    existing.topic_refs = merge_string_refs(&existing.topic_refs, &incoming.topic_refs);
    existing.source_refs = merge_string_refs(&existing.source_refs, &incoming.source_refs);
    existing.evidence_refs = merge_string_refs(&existing.evidence_refs, &incoming.evidence_refs);
    if claim_status_rank(&incoming.status) > claim_status_rank(&existing.status) {
        existing.status = incoming.status;
    }
    existing.updated_at = existing.updated_at.max(incoming.updated_at);
}

pub(crate) fn merge_string_refs(left: &[String], right: &[String]) -> Vec<String> {
    left.iter()
        .chain(right.iter())
        .cloned()
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

pub(crate) fn claim_status_rank(status: &str) -> u8 {
    match status {
        "supported" => 3,
        "accepted" => 2,
        "candidate" => 1,
        _ => 0,
    }
}

pub(crate) fn build_durable_memory_records(
    existing_memories: &[MemoryRecord],
) -> Vec<MemoryRecord> {
    existing_memories.to_vec()
}

pub(crate) fn matching_memory_id_for_candidate<'a>(
    candidate: &MarkdownClaimCandidate,
    existing_memories: &'a [MemoryRecord],
    generated_memories: impl Iterator<Item = &'a MemoryRecord>,
) -> Option<String> {
    let incoming = MemoryRecord {
        memory_id: memory_id_for_claim_candidate(candidate),
        workspace_id: String::new(),
        scope: BrainScope::Project,
        title: durable_memory_title(candidate),
        body: candidate.statement.clone(),
        source_refs: candidate.source_refs.clone(),
        evidence_refs: vec![candidate.evidence_id.clone()],
        created_at: 0,
        updated_at: 0,
    };
    generated_memories
        .chain(existing_memories.iter())
        .filter_map(|memory| {
            memory_match_score(&incoming, memory)
                .map(|score| (score, memory.updated_at, memory.memory_id.clone()))
        })
        .max_by(|left, right| {
            left.0
                .cmp(&right.0)
                .then_with(|| left.1.cmp(&right.1))
                .then_with(|| right.2.cmp(&left.2))
        })
        .map(|(_, _, memory_id)| memory_id)
}

pub(crate) fn memory_match_score(incoming: &MemoryRecord, existing: &MemoryRecord) -> Option<u16> {
    if incoming.memory_id == existing.memory_id {
        return Some(1000);
    }
    if normalize_key(&incoming.body) == normalize_key(&existing.body) {
        return Some(950);
    }
    if normalize_key(&incoming.title) == normalize_key(&existing.title) {
        return Some(900);
    }
    if !incoming.evidence_refs.is_empty()
        && incoming
            .evidence_refs
            .iter()
            .any(|evidence_id| existing.evidence_refs.contains(evidence_id))
    {
        return Some(850);
    }

    let incoming_terms = search_terms(&format!("{} {}", incoming.title, incoming.body));
    let existing_terms = search_terms(&format!("{} {}", existing.title, existing.body));
    if incoming_terms.is_empty() || existing_terms.is_empty() {
        return None;
    }
    let existing_terms = existing_terms.into_iter().collect::<BTreeSet<_>>();
    let shared = incoming_terms
        .iter()
        .filter(|term| existing_terms.contains(*term))
        .count();
    let smaller = incoming_terms.len().min(existing_terms.len());
    let larger = incoming_terms.len().max(existing_terms.len());
    if shared < 4 {
        return None;
    }
    let smaller_coverage = shared * 100 / smaller;
    let larger_coverage = shared * 100 / larger;
    (smaller_coverage >= 80 && larger_coverage >= 65).then_some(700 + larger_coverage as u16)
}

pub(crate) fn durable_memory_title(candidate: &MarkdownClaimCandidate) -> String {
    let prefix = match candidate.classification {
        MarkdownClaimClassification::Decision => "Decision",
        MarkdownClaimClassification::DurableFact => "Fact",
    };
    format!("{}: {}", prefix, excerpt(&candidate.statement, 72))
}

pub(crate) fn memory_id_for_claim_candidate(candidate: &MarkdownClaimCandidate) -> String {
    format!("memory-{}", bounded_artifact_key(&candidate.statement, 96))
}

pub(crate) fn markdown_claim_classification_slug(
    classification: MarkdownClaimClassification,
) -> &'static str {
    match classification {
        MarkdownClaimClassification::Decision => "decision",
        MarkdownClaimClassification::DurableFact => "durable_fact",
    }
}

pub(crate) fn merge_memory_record(existing: &mut MemoryRecord, incoming: MemoryRecord) {
    existing.source_refs = merge_string_refs(&existing.source_refs, &incoming.source_refs);
    existing.evidence_refs = merge_string_refs(&existing.evidence_refs, &incoming.evidence_refs);
    existing.created_at = existing.created_at.min(incoming.created_at);
    existing.updated_at = existing.updated_at.max(incoming.updated_at);
}

pub(crate) fn memory_record_content_matches(left: &MemoryRecord, right: &MemoryRecord) -> bool {
    left.workspace_id == right.workspace_id
        && left.scope == right.scope
        && left.title == right.title
        && left.body == right.body
        && merge_string_refs(&left.source_refs, &[]) == merge_string_refs(&right.source_refs, &[])
        && merge_string_refs(&left.evidence_refs, &[])
            == merge_string_refs(&right.evidence_refs, &[])
}

pub(crate) fn memory_record_auto_accepted_event(memory: &MemoryRecord) -> BrainEvent {
    BrainEvent {
        event_id: format!("evt-{}-accepted", memory.memory_id),
        schema_version: BRAIN_EVENT_SCHEMA_VERSION,
        workspace_id: memory.workspace_id.clone(),
        scope: memory.scope,
        event_type: BrainEventKind::MemoryAccepted,
        operation_type: Some("new_memory".into()),
        actor: BrainActor {
            actor_type: BrainActorType::Agent,
            actor_id: "etyma-agent-ingest".into(),
        },
        source_refs: memory.source_refs.clone(),
        source_markdown_refs: Vec::new(),
        node_refs: Vec::new(),
        relation_refs: Vec::new(),
        claim_refs: Vec::new(),
        memory_refs: vec![memory.memory_id.clone()],
        target_node_ids: Vec::new(),
        target_edge_ids: Vec::new(),
        target_claim_ids: Vec::new(),
        target_memory_ids: vec![memory.memory_id.clone()],
        evidence_refs: memory.evidence_refs.clone(),
        payload_json: serde_json::to_string(memory).unwrap_or_else(|_| "{}".into()),
        causality: BrainEventCausality {
            caused_by_source_ids: memory.source_refs.clone(),
            materialized_version: Some(memory.updated_at),
            ..Default::default()
        },
        confidence: Some("0.78".into()),
        policy_result: "auto_applied".into(),
        created_at: memory.updated_at,
    }
}

pub(crate) fn merge_materialized_memory_records(
    generated: Vec<MemoryRecord>,
    existing: Vec<MemoryRecord>,
) -> Vec<MemoryRecord> {
    let mut merged = BTreeMap::<String, MemoryRecord>::new();
    for memory in generated.into_iter().chain(existing) {
        match merged.get_mut(&memory.memory_id) {
            Some(existing) => merge_memory_record(existing, memory),
            None => {
                merged.insert(memory.memory_id.clone(), memory);
            }
        }
    }
    let mut memories = merged.into_values().collect::<Vec<_>>();
    memories.sort_by(|left, right| {
        right
            .updated_at
            .cmp(&left.updated_at)
            .then_with(|| left.memory_id.cmp(&right.memory_id))
    });
    memories
}

