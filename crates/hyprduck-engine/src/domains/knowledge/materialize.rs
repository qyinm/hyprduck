use super::origin::*;
use super::replay::*;
use super::*;
use crate::graph_history::{graph_snapshot_source_ingest_id, latest_graph_materialized_event};

mod source_helpers;

pub(crate) use source_helpers::*;

pub(crate) fn build_brain_repo_snapshot(
    workspace_id: &str,
    rows: &[(StoredSourceRow, Option<KnowledgeProject>)],
    aggregate: &KnowledgeProject,
    corrections: &[WorkspaceCorrection],
    existing_memories: &[MemoryRecord],
    existing_nodes: &[BrainNodeRecord],
    existing_relations: &[BrainRelationRecord],
) -> BrainRepoSnapshot {
    let generated_at = unix_timestamp_seconds();
    let sources = rows
        .iter()
        .map(|(row, _)| SourceRecord {
            source_id: row.summary.source_id.clone(),
            workspace_id: row.summary.workspace_id.clone(),
            original_path: row.summary.original_path.clone(),
            source_path: row.summary.source_path.clone(),
            markdown_path: row.summary.markdown_path.clone(),
            format: document_format_slug(&row.summary.format).into(),
            status: ingest_status_slug(&row.summary.status).into(),
            page_count: row.summary.page_count,
            description: row.summary.description.clone(),
            user_context: row.summary.user_context.clone(),
            ingest_instruction: row.summary.ingest_instruction.clone(),
            updated_at: row.summary.updated_at,
        })
        .collect::<Vec<_>>();

    let mut evidence_by_id = BTreeMap::<String, EvidenceRef>::new();
    for detail in aggregate.details_by_node_id.values() {
        for evidence in &detail.evidence {
            evidence_by_id.insert(evidence.id.clone(), evidence.clone());
        }
    }
    for detail in aggregate.edge_details_by_id.values() {
        for evidence in &detail.evidence {
            evidence_by_id.insert(evidence.id.clone(), evidence.clone());
        }
    }
    let existing_node_by_id = existing_nodes
        .iter()
        .map(|node| (node.node_id.as_str(), node))
        .collect::<BTreeMap<_, _>>();
    let nodes = aggregate
        .details_by_node_id
        .values()
        .map(|detail| {
            let evidence_ids = detail
                .evidence
                .iter()
                .map(|evidence| evidence.id.clone())
                .collect::<BTreeSet<_>>()
                .into_iter()
                .collect::<Vec<_>>();
            let source_ids = detail
                .evidence
                .iter()
                .filter_map(|evidence| evidence.source_id.clone())
                .collect::<BTreeSet<_>>()
                .into_iter()
                .collect::<Vec<_>>();
            let mut node = BrainNodeRecord {
                node_id: detail.node.id.clone(),
                kind: brain_node_kind_for_graph_kind(detail.node.kind),
                label: detail.canonical_name.clone(),
                scope: BrainScope::Project,
                aliases: detail.aliases.clone(),
                evidence_ids,
                source_ids,
                confidence: detail.node.confidence,
                updated_at: generated_at,
            };
            if let Some(existing) = existing_node_by_id.get(node.node_id.as_str()) {
                if brain_node_record_content_matches(existing, &node) {
                    node.updated_at = existing.updated_at;
                }
            }
            node
        })
        .collect::<Vec<_>>();

    let entities = nodes
        .iter()
        .filter(|node| matches!(node.kind, BrainNodeKind::Concept | BrainNodeKind::Topic))
        .map(|node| EntityRecord {
            entity_id: format!("ent-{}", node.node_id),
            workspace_id: workspace_id.to_string(),
            kind: node.kind,
            name: node.label.clone(),
            aliases: node.aliases.clone(),
            source_refs: node.source_ids.clone(),
            evidence_refs: node.evidence_ids.clone(),
            updated_at: generated_at,
        })
        .collect::<Vec<_>>();

    let claims = aggregate
        .details_by_node_id
        .values()
        .filter(|detail| {
            matches!(
                detail.node.kind,
                GraphNodeKind::Concept | GraphNodeKind::Page
            )
        })
        .filter(|detail| !detail.evidence.is_empty())
        .map(|detail| {
            let evidence_refs = detail
                .evidence
                .iter()
                .map(|evidence| evidence.id.clone())
                .collect::<BTreeSet<_>>()
                .into_iter()
                .collect::<Vec<_>>();
            let source_refs = detail
                .evidence
                .iter()
                .filter_map(|evidence| evidence.source_id.clone())
                .collect::<BTreeSet<_>>()
                .into_iter()
                .collect::<Vec<_>>();
            let first_evidence = detail
                .evidence
                .first()
                .map(|evidence| evidence.snippet.trim())
                .filter(|snippet| !snippet.is_empty())
                .unwrap_or(&detail.description);
            ClaimRecord {
                claim_id: format!("claim-{}", detail.node.id),
                workspace_id: workspace_id.to_string(),
                statement: format!(
                    "{} is evidence-backed by: {}",
                    detail.canonical_name, first_evidence
                ),
                topic_refs: vec![detail.node.id.clone()],
                source_refs,
                evidence_refs,
                status: "supported".into(),
                updated_at: generated_at,
            }
        })
        .collect::<Vec<_>>();

    let memories = build_durable_memory_records(existing_memories);
    let existing_relation_by_id = existing_relations
        .iter()
        .map(|relation| (relation.relation_id.as_str(), relation))
        .collect::<BTreeMap<_, _>>();
    let relations = aggregate
        .edges
        .iter()
        .filter_map(|edge| {
            let evidence_ids = aggregate
                .edge_details_by_id
                .get(&edge.id)
                .map(|detail| {
                    detail
                        .evidence
                        .iter()
                        .map(|evidence| evidence.id.clone())
                        .collect::<BTreeSet<_>>()
                        .into_iter()
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default();
            if evidence_ids.is_empty() {
                return None;
            }
            let mut relation = BrainRelationRecord {
                relation_id: edge.id.clone(),
                kind: brain_relation_kind_for_edge(edge),
                source_node_id: edge.source_node_id.clone(),
                target_node_id: edge.target_node_id.clone(),
                label: edge.label.clone(),
                evidence_ids,
                confidence: edge.confidence,
                updated_at: generated_at,
            };
            if let Some(existing) = existing_relation_by_id.get(relation.relation_id.as_str()) {
                if brain_relation_record_content_matches(existing, &relation) {
                    relation.updated_at = existing.updated_at;
                }
            }
            Some(relation)
        })
        .collect::<Vec<_>>();

    let extractions = Vec::new();
    let wiki_pages = build_materialized_wiki_pages(workspace_id, &sources, &nodes, generated_at);
    let mut events = vec![
        BrainEvent {
            event_id: format!("evt-{workspace_id}-graph-materialized-{generated_at}"),
            schema_version: BRAIN_EVENT_SCHEMA_VERSION,
            workspace_id: workspace_id.to_string(),
            scope: BrainScope::Project,
            event_type: BrainEventKind::GraphMaterialized,
            operation_type: Some("graph_materialized".into()),
            actor: BrainActor {
                actor_type: BrainActorType::System,
                actor_id: "hyprduck-engine".into(),
            },
            source_refs: sources
                .iter()
                .map(|source| source.source_id.clone())
                .collect::<Vec<_>>(),
            source_markdown_refs: sources
                .iter()
                .map(|source| source.markdown_path.clone())
                .collect::<Vec<_>>(),
            node_refs: nodes.iter().map(|node| node.node_id.clone()).collect(),
            relation_refs: relations
                .iter()
                .map(|relation| relation.relation_id.clone())
                .collect(),
            claim_refs: claims.iter().map(|claim| claim.claim_id.clone()).collect(),
            memory_refs: memories
                .iter()
                .map(|memory| memory.memory_id.clone())
                .collect(),
            target_node_ids: nodes.iter().map(|node| node.node_id.clone()).collect(),
            target_edge_ids: relations
                .iter()
                .map(|relation| relation.relation_id.clone())
                .collect(),
            target_claim_ids: claims.iter().map(|claim| claim.claim_id.clone()).collect(),
            target_memory_ids: memories
                .iter()
                .map(|memory| memory.memory_id.clone())
                .collect(),
            evidence_refs: evidence_by_id.keys().cloned().collect(),
            payload_json: materialized_graph_event_payload_json(
                generated_at,
                &sources,
                &nodes,
                &relations,
                &evidence_by_id.values().cloned().collect::<Vec<_>>(),
                &memories,
                &wiki_pages,
                &entities,
                &claims,
                &extractions,
            )
            .unwrap_or_else(|_| {
                format!(
                    "{{\"nodeCount\":{},\"relationCount\":{},\"sourceCount\":{}}}",
                    nodes.len(),
                    relations.len(),
                    sources.len()
                )
            }),
            causality: BrainEventCausality {
                caused_by_source_ids: sources
                    .iter()
                    .map(|source| source.source_id.clone())
                    .collect(),
                snapshot_id: Some(format!("snapshot-{workspace_id}-{generated_at}")),
                materialized_version: Some(generated_at),
                ..Default::default()
            },
            confidence: None,
            policy_result: "materialized".into(),
            created_at: generated_at,
        },
        BrainEvent {
            event_id: format!("evt-{workspace_id}-wiki-materialized-{generated_at}"),
            schema_version: BRAIN_EVENT_SCHEMA_VERSION,
            workspace_id: workspace_id.to_string(),
            scope: BrainScope::Project,
            event_type: BrainEventKind::WikiMaterialized,
            operation_type: Some("wiki_materialized".into()),
            actor: BrainActor {
                actor_type: BrainActorType::System,
                actor_id: "hyprduck-engine".into(),
            },
            source_refs: Vec::new(),
            source_markdown_refs: Vec::new(),
            node_refs: wiki_pages
                .iter()
                .flat_map(|page| page.node_refs.clone())
                .collect(),
            relation_refs: Vec::new(),
            claim_refs: Vec::new(),
            memory_refs: Vec::new(),
            target_node_ids: wiki_pages
                .iter()
                .flat_map(|page| page.node_refs.clone())
                .collect(),
            target_edge_ids: Vec::new(),
            target_claim_ids: Vec::new(),
            target_memory_ids: Vec::new(),
            evidence_refs: wiki_pages
                .iter()
                .flat_map(|page| page.evidence_refs.clone())
                .collect(),
            payload_json: format!("{{\"pageCount\":{}}}", wiki_pages.len()),
            causality: BrainEventCausality {
                snapshot_id: Some(format!("snapshot-{workspace_id}-{generated_at}")),
                materialized_version: Some(generated_at),
                ..Default::default()
            },
            confidence: None,
            policy_result: "materialized".into(),
            created_at: generated_at,
        },
    ];
    events.extend(memories.iter().map(memory_record_auto_accepted_event));
    events.extend(corrections.iter().map(|correction| {
        BrainEvent {
            event_id: format!("evt-{}", correction.id),
            schema_version: BRAIN_EVENT_SCHEMA_VERSION,
            workspace_id: correction.workspace_id.clone(),
            scope: BrainScope::Project,
            event_type: BrainEventKind::CorrectionApplied,
            operation_type: Some(correction_kind_slug(&correction.kind).into()),
            actor: BrainActor {
                actor_type: BrainActorType::User,
                actor_id: "local-user".into(),
            },
            source_refs: Vec::new(),
            source_markdown_refs: Vec::new(),
            node_refs: std::iter::once(correction.aggregate_node_id.clone())
                .chain(correction.target_node_id.clone())
                .chain(correction.source_node_ids.clone())
                .collect(),
            relation_refs: Vec::new(),
            claim_refs: Vec::new(),
            memory_refs: Vec::new(),
            target_node_ids: std::iter::once(correction.aggregate_node_id.clone())
                .chain(correction.target_node_id.clone())
                .collect(),
            target_edge_ids: Vec::new(),
            target_claim_ids: Vec::new(),
            target_memory_ids: Vec::new(),
            evidence_refs: correction.evidence_ids.clone(),
            payload_json: format!(
                "{{\"kind\":\"{}\",\"value\":{}}}",
                correction_kind_slug(&correction.kind),
                correction
                    .value
                    .as_ref()
                    .map(|value| json!(value).to_string())
                    .unwrap_or_else(|| "null".into())
            ),
            causality: BrainEventCausality {
                materialized_version: Some(correction.created_at),
                ..Default::default()
            },
            confidence: None,
            policy_result: "applied".into(),
            created_at: correction.created_at,
        }
    }));

    BrainRepoSnapshot {
        workspace_id: workspace_id.to_string(),
        generated_at,
        sources,
        nodes,
        relations,
        evidence: evidence_by_id.into_values().collect(),
        memories,
        wiki_pages,
        entities,
        claims,
        extractions,
        events,
    }
}

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
            actor_id: "hyprduck-agent-ingest".into(),
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

pub(crate) fn build_structured_extraction_artifacts(
    workspace_id: &str,
    rows: &[(StoredSourceRow, Option<KnowledgeProject>)],
    generated_at: u64,
) -> Vec<StructuredExtractionArtifact> {
    rows.iter()
        .map(|(row, project)| {
            build_structured_extraction_artifact_for_source(
                workspace_id,
                row,
                project.as_ref(),
                generated_at,
            )
        })
        .collect()
}

pub(crate) fn build_structured_extraction_artifact_for_source(
    workspace_id: &str,
    row: &StoredSourceRow,
    project: Option<&KnowledgeProject>,
    generated_at: u64,
) -> StructuredExtractionArtifact {
    let source_id = row.summary.source_id.clone();
    let source_refs = vec![source_id.clone()];
    let Some(project) = project else {
        return StructuredExtractionArtifact {
            artifact_id: format!("extraction-{source_id}"),
            workspace_id: workspace_id.into(),
            source_id,
            extractor: "source-evidence-packager".into(),
            extractor_model: None,
            source_refs,
            page_refs: Vec::new(),
            entities: Vec::new(),
            topics: Vec::new(),
            claims: Vec::new(),
            relations: Vec::new(),
            memories: Vec::new(),
            evidence_refs: Vec::new(),
            confidence: Some(0.0),
            provenance: "No compiled project snapshot was available for this source.".into(),
            created_at: generated_at,
        };
    };

    let mut evidence_by_id = BTreeMap::<String, EvidenceRef>::new();
    for detail in project.details_by_node_id.values() {
        for evidence in &detail.evidence {
            if evidence.source_id.as_deref() == Some(source_id.as_str()) {
                evidence_by_id.insert(evidence.id.clone(), evidence.clone());
            }
        }
    }
    for detail in project.edge_details_by_id.values() {
        for evidence in &detail.evidence {
            if evidence.source_id.as_deref() == Some(source_id.as_str()) {
                evidence_by_id.insert(evidence.id.clone(), evidence.clone());
            }
        }
    }
    let claim_candidates = read_markdown_claim_candidates_for_row(row);
    for candidate in &claim_candidates {
        evidence_by_id
            .entry(candidate.evidence_id.clone())
            .or_insert_with(|| EvidenceRef {
                id: candidate.evidence_id.clone(),
                page_label: "Imported text".into(),
                page_index: Some(0),
                snippet: candidate.evidence_snippet.clone(),
                source_path: Some(candidate.source_path.clone()),
                source_id: candidate
                    .source_id
                    .clone()
                    .or_else(|| Some(source_id.clone())),
                markdown_path: Some(row.summary.markdown_path.clone()),
                image_path: None,
                provenance: Some(format!(
                    "Claim candidate extracted from markdown line {} during autonomous ingest.",
                    candidate.line_start
                )),
            });
    }

    let page_refs = page_refs_from_evidence(evidence_by_id.values());
    let entities = project
        .details_by_node_id
        .values()
        .filter(|detail| detail.node.kind == GraphNodeKind::Concept)
        .filter_map(|detail| {
            let evidence = evidence_for_source(&detail.evidence, &source_id);
            let evidence_refs = evidence
                .iter()
                .map(|evidence| evidence.id.clone())
                .collect::<Vec<_>>();
            if evidence_refs.is_empty() {
                return None;
            }
            Some(StructuredExtractionEntity {
                entity_id: format!("ent-{}", detail.node.id),
                kind: BrainNodeKind::Concept,
                name: detail.canonical_name.clone(),
                aliases: detail.aliases.clone(),
                source_refs: source_refs_from_evidence(&evidence, &source_id),
                evidence_refs,
                page_refs: page_refs_from_evidence(evidence.iter().copied()),
                confidence: detail.node.confidence,
                provenance: format!(
                    "Source evidence packager preserved concept node '{}' from accepted graph state.",
                    detail.canonical_name
                ),
            })
        })
        .collect::<Vec<_>>();

    let topics = project
        .details_by_node_id
        .values()
        .filter(|detail| detail.node.kind == GraphNodeKind::Concept)
        .filter_map(|detail| {
            let evidence = evidence_for_source(&detail.evidence, &source_id);
            let evidence_refs = evidence
                .iter()
                .map(|evidence| evidence.id.clone())
                .collect::<Vec<_>>();
            if evidence_refs.is_empty() {
                return None;
            }
            Some(StructuredExtractionTopic {
                topic_id: detail.node.id.clone(),
                title: detail.canonical_name.clone(),
                source_refs: source_refs_from_evidence(&evidence, &source_id),
                evidence_refs,
                page_refs: page_refs_from_evidence(evidence.iter().copied()),
                confidence: detail.node.confidence,
                provenance: format!(
                    "Source evidence packager preserved '{}' as a source-backed topic from accepted graph state.",
                    detail.canonical_name
                ),
            })
        })
        .collect::<Vec<_>>();

    let mut claims = project
        .details_by_node_id
        .values()
        .filter(|detail| matches!(detail.node.kind, GraphNodeKind::Concept | GraphNodeKind::Page))
        .filter_map(|detail| {
            let evidence = evidence_for_source(&detail.evidence, &source_id);
            let evidence_refs = evidence
                .iter()
                .map(|evidence| evidence.id.clone())
                .collect::<Vec<_>>();
            if evidence_refs.is_empty() {
                return None;
            }
            let first_evidence = evidence
                .first()
                .map(|evidence| evidence.snippet.trim())
                .filter(|snippet| !snippet.is_empty())
                .unwrap_or(&detail.description);
            Some(StructuredExtractionClaim {
                claim_id: format!("claim-{}", detail.node.id),
                statement: format!(
                    "{} is evidence-backed by: {}",
                    detail.canonical_name, first_evidence
                ),
                subject_refs: vec![detail.node.id.clone()],
                source_refs: source_refs_from_evidence(&evidence, &source_id),
                evidence_refs,
                page_refs: page_refs_from_evidence(evidence.iter().copied()),
                confidence: detail.node.confidence,
                status: "supported".into(),
                provenance: format!(
                    "Source evidence packager preserved this claim because '{}' has direct source evidence.",
                    detail.canonical_name
                ),
            })
        })
        .collect::<Vec<_>>();
    let memories = claim_candidates
        .iter()
        .filter(|candidate| candidate.durable && candidate.memory_candidate)
        .map(|candidate| {
            let page_refs = evidence_by_id
                .get(&candidate.evidence_id)
                .map(|evidence| page_refs_from_evidence(std::iter::once(evidence)))
                .unwrap_or_default();
            StructuredExtractionMemoryCandidate {
                memory_id: memory_id_for_claim_candidate(candidate),
                title: durable_memory_title(candidate),
                body: candidate.statement.clone(),
                kind: markdown_claim_classification_slug(candidate.classification).into(),
                source_refs: if candidate.source_refs.is_empty() {
                    vec![source_id.clone()]
                } else {
                    candidate.source_refs.clone()
                },
                evidence_refs: vec![candidate.evidence_id.clone()],
                page_refs,
                confidence: Some(candidate.confidence),
                status: "auto_apply_candidate".into(),
                provenance: format!(
                    "Autonomous markdown ingest promoted this {} into a memory candidate at line {} because {}.",
                    markdown_claim_classification_slug(candidate.classification),
                    candidate.line_start,
                    candidate.reason
                ),
            }
        })
        .collect::<Vec<_>>();

    claims.extend(claim_candidates.into_iter().map(|candidate| {
        let page_refs = evidence_by_id
            .get(&candidate.evidence_id)
            .map(|evidence| page_refs_from_evidence(std::iter::once(evidence)))
            .unwrap_or_default();
        StructuredExtractionClaim {
            claim_id: format!("claim-{}", bounded_artifact_key(&candidate.statement, 80)),
            statement: candidate.statement,
            subject_refs: candidate.subject_refs,
            source_refs: if candidate.source_refs.is_empty() {
                vec![source_id.clone()]
            } else {
                candidate.source_refs
            },
            evidence_refs: vec![candidate.evidence_id],
            page_refs,
            confidence: Some(candidate.confidence),
            status: "candidate".into(),
            provenance: format!(
                "Autonomous markdown ingest extracted this source claim at line {} because {}.",
                candidate.line_start, candidate.reason
            ),
        }
    }));

    let relations = project
        .edges
        .iter()
        .filter_map(|edge| {
            let evidence = project
                .edge_details_by_id
                .get(&edge.id)
                .map(|detail| evidence_for_source(&detail.evidence, &source_id))
                .unwrap_or_default();
            let evidence_refs = evidence
                .iter()
                .map(|evidence| evidence.id.clone())
                .collect::<Vec<_>>();
            if evidence_refs.is_empty() {
                return None;
            }
            Some(StructuredExtractionRelation {
                relation_id: edge.id.clone(),
                kind: brain_relation_kind_for_edge(edge),
                source_node_id: edge.source_node_id.clone(),
                target_node_id: edge.target_node_id.clone(),
                label: edge.label.clone(),
                source_refs: source_refs_from_evidence(&evidence, &source_id),
                evidence_refs,
                page_refs: page_refs_from_evidence(evidence.iter().copied()),
                confidence: edge.confidence,
                provenance: format!(
                    "Source evidence packager preserved this relation because it has source evidence in {}.",
                    source_id
                ),
            })
        })
        .collect::<Vec<_>>();

    StructuredExtractionArtifact {
        artifact_id: format!("extraction-{source_id}"),
        workspace_id: workspace_id.into(),
        source_id,
        extractor: "source-evidence-packager".into(),
        extractor_model: None,
        source_refs,
        page_refs,
        entities,
        topics,
        claims,
        relations,
        memories,
        evidence_refs: evidence_by_id.into_values().collect(),
        confidence: Some(if project.summary.status == ProjectStatus::Ready {
            0.7
        } else {
            0.4
        }),
        provenance:
            "Structured extraction artifact packaged from source evidence and accepted graph state."
                .into(),
        created_at: generated_at,
    }
}

pub(crate) fn evidence_for_source<'a>(
    evidence: &'a [EvidenceRef],
    source_id: &str,
) -> Vec<&'a EvidenceRef> {
    evidence
        .iter()
        .filter(|evidence| evidence.source_id.as_deref() == Some(source_id))
        .collect()
}

pub(crate) fn source_refs_from_evidence(
    evidence: &[&EvidenceRef],
    fallback_source_id: &str,
) -> Vec<String> {
    let mut refs = evidence
        .iter()
        .filter_map(|evidence| evidence.source_id.clone())
        .collect::<BTreeSet<_>>();
    if refs.is_empty() {
        refs.insert(fallback_source_id.into());
    }
    refs.into_iter().collect()
}

pub(crate) fn page_refs_from_evidence<'a>(
    evidence: impl IntoIterator<Item = &'a EvidenceRef>,
) -> Vec<StructuredExtractionPageRef> {
    let mut refs = BTreeMap::<(String, Option<usize>), StructuredExtractionPageRef>::new();
    for evidence in evidence {
        refs.entry((evidence.page_label.clone(), evidence.page_index))
            .or_insert_with(|| StructuredExtractionPageRef {
                page_label: evidence.page_label.clone(),
                page_index: evidence.page_index,
                markdown_path: evidence.markdown_path.clone(),
                image_path: evidence.image_path.clone(),
            });
    }
    refs.into_values().collect()
}

pub(crate) fn brain_node_kind_for_graph_kind(kind: GraphNodeKind) -> BrainNodeKind {
    match kind {
        GraphNodeKind::Source | GraphNodeKind::Document => BrainNodeKind::Source,
        GraphNodeKind::Page => BrainNodeKind::Topic,
        GraphNodeKind::Concept => BrainNodeKind::Concept,
    }
}

pub(crate) fn brain_relation_kind_for_edge(edge: &RelationEdgeSummary) -> BrainRelationKind {
    match edge.kind {
        RelationKind::SourceDocument => BrainRelationKind::DerivedFrom,
        RelationKind::RelatedTo => match edge.label.as_str() {
            "Supports" => BrainRelationKind::Supports,
            "Contradicts" => BrainRelationKind::Contradicts,
            "Supersedes" => BrainRelationKind::Supersedes,
            "Same as" => BrainRelationKind::SameAs,
            "Depends on" => BrainRelationKind::DependsOn,
            _ => BrainRelationKind::RelatedTo,
        },
    }
}

pub(crate) fn build_materialized_wiki_pages(
    workspace_id: &str,
    sources: &[SourceRecord],
    nodes: &[BrainNodeRecord],
    generated_at: u64,
) -> Vec<WikiPage> {
    let mut pages = vec![
        WikiPage {
            page_id: "wiki-overview".into(),
            workspace_id: workspace_id.into(),
            path: "wiki/overview.md".into(),
            title: "Workspace Overview".into(),
            body: format!(
                "# Workspace Overview\n\n- Workspace: `{workspace_id}`\n- Sources: {}\n- Nodes: {}\n",
                sources.len(),
                nodes.len()
            ),
            node_refs: nodes.iter().map(|node| node.node_id.clone()).collect(),
            source_refs: sources
                .iter()
                .map(|source| source.source_id.clone())
                .collect(),
            evidence_refs: nodes
                .iter()
                .flat_map(|node| node.evidence_ids.clone())
                .collect::<BTreeSet<_>>()
                .into_iter()
                .collect(),
            updated_at: generated_at,
        },
        WikiPage {
            page_id: "wiki-index".into(),
            workspace_id: workspace_id.into(),
            path: "wiki/index.md".into(),
            title: "Brain Index".into(),
            body: String::new(),
            node_refs: nodes.iter().map(|node| node.node_id.clone()).collect(),
            source_refs: sources
                .iter()
                .map(|source| source.source_id.clone())
                .collect(),
            evidence_refs: Vec::new(),
            updated_at: generated_at,
        },
        WikiPage {
            page_id: "wiki-log".into(),
            workspace_id: workspace_id.into(),
            path: "wiki/log.md".into(),
            title: "Brain Log".into(),
            body: String::new(),
            node_refs: Vec::new(),
            source_refs: Vec::new(),
            evidence_refs: Vec::new(),
            updated_at: generated_at,
        },
    ];
    pages.extend(sources.iter().map(|source| WikiPage {
        page_id: format!("wiki-source-{}", source.source_id),
        workspace_id: workspace_id.into(),
        path: format!("wiki/sources/{}.md", sanitize_name(&source.source_id)),
        title: source.source_id.clone(),
        body: format!(
            "# {}\n\n- Original: `{}`\n- Source: `{}`\n- Markdown: `{}`\n- Status: `{}`\n",
            source.source_id,
            source.original_path,
            source.source_path,
            source.markdown_path,
            source.status
        ),
        node_refs: Vec::new(),
        source_refs: vec![source.source_id.clone()],
        evidence_refs: Vec::new(),
        updated_at: generated_at,
    }));
    pages.extend(
        nodes
            .iter()
            .filter(|node| node.kind == BrainNodeKind::Concept)
            .map(|node| WikiPage {
                page_id: format!("wiki-topic-{}", node.node_id),
                workspace_id: workspace_id.into(),
                path: format!("wiki/topics/{}.md", sanitize_name(&node.node_id)),
                title: node.label.clone(),
                body: format!(
                    "# {}\n\n- Node: `{}`\n- Sources: {}\n- Evidence refs: {}\n",
                    node.label,
                    node.node_id,
                    node.source_ids.join(", "),
                    node.evidence_ids.len()
                ),
                node_refs: vec![node.node_id.clone()],
                source_refs: node.source_ids.clone(),
                evidence_refs: node.evidence_ids.clone(),
                updated_at: generated_at,
            }),
    );
    pages
}

pub(crate) fn ensure_materialized_brain_repo_dirs(root: &Path) -> Result<()> {
    let wiki_root = root.join("wiki");
    for dir in [
        root.join("graph"),
        root.join("artifacts"),
        root.join("events"),
        root.join("memory"),
        root.join("state"),
        wiki_root.join("sources"),
        wiki_root.join("entities"),
        wiki_root.join("topics"),
        wiki_root.join("claims"),
        wiki_root.join("questions"),
    ] {
        fs::create_dir_all(&dir).with_context(|| format!("failed creating {}", dir.display()))?;
    }

    fs::write(root.join("memory/.gitkeep"), "").context("failed writing memory placeholder")?;
    Ok(())
}

pub(crate) fn publish_latest_readable_graph_snapshot_marker(
    root: &Path,
    snapshot: &BrainRepoSnapshot,
) -> Result<Option<LatestReadableGraphSnapshotMarker>> {
    validate_latest_readable_materialized_files(root, snapshot)?;
    let Some(event) = latest_graph_materialized_event(&snapshot.events, &snapshot.workspace_id)
    else {
        return Ok(None);
    };
    let materialized_at = event
        .causality
        .materialized_version
        .unwrap_or(event.created_at);
    let snapshot_id = event
        .causality
        .snapshot_id
        .clone()
        .unwrap_or_else(|| format!("snapshot-{}-{materialized_at}", snapshot.workspace_id));
    let marker = LatestReadableGraphSnapshotMarker {
        schema_version: BRAIN_EVENT_SCHEMA_VERSION,
        workspace_id: snapshot.workspace_id.clone(),
        snapshot_id,
        event_id: event.event_id.clone(),
        source_ingest_id: graph_snapshot_source_ingest_id(event),
        materialized_at,
        published_at: unix_timestamp_seconds(),
        source_markdown_refs: event.source_markdown_refs.clone(),
        materialized_files: latest_readable_materialized_file_refs(snapshot),
    };
    write_json_pretty(&root.join(LATEST_READABLE_SNAPSHOT_PATH), &marker)?;
    Ok(Some(marker))
}

pub(crate) fn read_latest_readable_graph_snapshot_marker(
    root: &Path,
) -> Result<Option<LatestReadableGraphSnapshotMarker>> {
    let path = root.join(LATEST_READABLE_SNAPSHOT_PATH);
    if !path.exists() {
        return Ok(None);
    }
    read_json_artifact(&path).map(Some)
}

pub(crate) fn validate_latest_readable_materialized_files(
    root: &Path,
    snapshot: &BrainRepoSnapshot,
) -> Result<()> {
    let manifest: BrainRepoSnapshot = read_json_artifact(&root.join("brain-manifest.json"))?;
    if manifest.workspace_id != snapshot.workspace_id {
        bail!(
            "materialized brain manifest workspace_id {} does not match {}",
            manifest.workspace_id,
            snapshot.workspace_id
        );
    }
    let nodes: Vec<BrainNodeRecord> = read_json_artifact(&root.join("graph/nodes.json"))?;
    let edges: Vec<BrainRelationRecord> = read_json_artifact(&root.join("graph/edges.json"))?;
    let claims: Vec<ClaimRecord> = read_json_artifact(&root.join("graph/claims.json"))?;
    let memories = read_memory_records(root)?;
    let events = read_brain_events_jsonl(&root.join("events/brain_events.jsonl"))?;
    if nodes != snapshot.nodes {
        bail!("materialized graph/nodes.json does not match the completed snapshot");
    }
    if edges != snapshot.relations {
        bail!("materialized graph/edges.json does not match the completed snapshot");
    }
    if claims != snapshot.claims {
        bail!("materialized graph/claims.json does not match the completed snapshot");
    }
    if memories != snapshot.memories {
        bail!("materialized memory/records.json does not match the completed snapshot");
    }
    if events != snapshot.events {
        bail!("materialized events/brain_events.jsonl does not match the completed snapshot");
    }
    for page in &snapshot.wiki_pages {
        let page_body = fs::read_to_string(root.join(&page.path))
            .with_context(|| format!("failed reading materialized wiki page {}", page.path))?;
        let expected = materialized_wiki_page_body(page, snapshot);
        if page_body != expected {
            bail!(
                "materialized wiki page {} does not match the completed snapshot",
                page.path
            );
        }
    }
    Ok(())
}

pub(crate) fn latest_readable_materialized_file_refs(snapshot: &BrainRepoSnapshot) -> Vec<String> {
    let mut files = vec![
        "brain-manifest.json".to_string(),
        "graph/nodes.json".to_string(),
        "graph/edges.json".to_string(),
        "graph/claims.json".to_string(),
        "memory/records.json".to_string(),
        "events/brain_events.jsonl".to_string(),
        MATERIALIZED_RECORD_ORIGINS_PATH.to_string(),
    ];
    files.extend(snapshot.wiki_pages.iter().map(|page| page.path.clone()));
    files.sort();
    files.dedup();
    files
}

pub(crate) fn write_structured_extraction_artifacts(
    root: &Path,
    extractions: &[StructuredExtractionArtifact],
) -> Result<()> {
    for extraction in extractions {
        write_json_pretty(
            &root
                .join("artifacts")
                .join(sanitize_name(&extraction.source_id))
                .join("extraction.json"),
            extraction,
        )?;
    }
    Ok(())
}

pub(crate) fn persist_materialized_graph_and_wiki_state(
    root: &Path,
    snapshot: &BrainRepoSnapshot,
) -> Result<()> {
    remove_stale_materialized_wiki_files(root, snapshot)?;
    write_json_pretty(&root.join("brain-manifest.json"), snapshot)?;
    write_json_pretty(&root.join("graph/nodes.json"), &snapshot.nodes)?;
    write_json_pretty(&root.join("graph/edges.json"), &snapshot.relations)?;
    write_json_pretty(&root.join("graph/evidence.json"), &snapshot.evidence)?;
    write_json_pretty(&root.join("graph/entities.json"), &snapshot.entities)?;
    write_json_pretty(&root.join("graph/claims.json"), &snapshot.claims)?;

    for page in &snapshot.wiki_pages {
        let path = root.join(&page.path);
        write_file_atomic(
            &path,
            materialized_wiki_page_body(page, snapshot).as_bytes(),
        )?;
    }
    Ok(())
}

fn remove_stale_materialized_wiki_files(root: &Path, snapshot: &BrainRepoSnapshot) -> Result<()> {
    let next_wiki_paths = snapshot
        .wiki_pages
        .iter()
        .map(|page| page.path.as_str())
        .collect::<BTreeSet<_>>();
    for relative_path in existing_wiki_markdown_files(root)? {
        if next_wiki_paths.contains(relative_path.as_str()) {
            continue;
        }
        let path = root.join(&relative_path);
        if path.exists() {
            fs::remove_file(&path)
                .with_context(|| format!("failed removing stale wiki page {}", path.display()))?;
        }
    }
    Ok(())
}

fn existing_wiki_markdown_files(root: &Path) -> Result<Vec<String>> {
    let wiki_root = root.join("wiki");
    if !wiki_root.exists() {
        return Ok(Vec::new());
    }
    let mut stack = vec![wiki_root];
    let mut files = Vec::new();
    while let Some(dir) = stack.pop() {
        for entry in
            fs::read_dir(&dir).with_context(|| format!("failed reading {}", dir.display()))?
        {
            let entry = entry.with_context(|| format!("failed reading {}", dir.display()))?;
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
                continue;
            }
            let Ok(relative_path) = path.strip_prefix(root) else {
                continue;
            };
            let relative_path = relative_path.to_string_lossy().replace('\\', "/");
            if is_wiki_markdown_ref(&relative_path) {
                files.push(relative_path);
            }
        }
    }
    Ok(files)
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

pub(crate) fn read_structured_extraction_artifacts(
    root: &Path,
    sources: &[SourceRecord],
) -> Result<Vec<StructuredExtractionArtifact>> {
    let mut artifacts = Vec::new();
    for source in sources {
        let path = root
            .join("artifacts")
            .join(sanitize_name(&source.source_id))
            .join("extraction.json");
        if path.exists() {
            artifacts.push(
                read_json_artifact(&path)
                    .with_context(|| format!("failed reading {}", path.display()))?,
            );
        }
    }
    Ok(artifacts)
}

pub(crate) fn read_markdown_claim_candidates_for_row(
    row: &StoredSourceRow,
) -> Vec<MarkdownClaimCandidate> {
    Path::new(&row.manifest_path)
        .parent()
        .map(|artifact_root| artifact_root.join("claim-candidates.json"))
        .filter(|path| path.exists())
        .and_then(|path| read_json_artifact::<Vec<MarkdownClaimCandidate>>(&path).ok())
        .unwrap_or_default()
}

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

pub(crate) fn materialized_wiki_page_body(page: &WikiPage, snapshot: &BrainRepoSnapshot) -> String {
    if page.path == "wiki/index.md" {
        let source_links = snapshot
            .sources
            .iter()
            .map(|source| {
                format!(
                    "- [{}](sources/{}.md)",
                    source.source_id,
                    sanitize_name(&source.source_id)
                )
            })
            .collect::<Vec<_>>()
            .join("\n");
        let topic_links = snapshot
            .nodes
            .iter()
            .filter(|node| node.kind == BrainNodeKind::Concept)
            .map(|node| {
                format!(
                    "- [{}](topics/{}.md)",
                    node.label,
                    sanitize_name(&node.node_id)
                )
            })
            .collect::<Vec<_>>()
            .join("\n");
        return format!(
            "# Brain Index\n\n## Sources\n\n{}\n\n## Topics\n\n{}\n",
            source_links, topic_links
        );
    }
    if page.path == "wiki/log.md" {
        return snapshot
            .events
            .iter()
            .map(|event| {
                format!(
                    "- {} `{}` by `{}`",
                    event.created_at, event.event_id, event.actor.actor_id
                )
            })
            .collect::<Vec<_>>()
            .join("\n");
    }
    if page.path.starts_with("wiki/topics/") {
        let page_node_ids = page.node_refs.iter().collect::<BTreeSet<_>>();
        let evidence_by_id = snapshot
            .evidence
            .iter()
            .map(|evidence| (evidence.id.as_str(), evidence))
            .collect::<BTreeMap<_, _>>();
        let node_descriptions = snapshot
            .nodes
            .iter()
            .filter(|node| page_node_ids.contains(&node.node_id))
            .filter_map(|node| {
                let evidence = node
                    .evidence_ids
                    .iter()
                    .filter_map(|evidence_id| evidence_by_id.get(evidence_id.as_str()).copied())
                    .find(|evidence| !evidence.snippet.trim().is_empty())?;
                Some(format!(
                    "- `{}`: {} _(source: {}; evidence: `{}`)_",
                    node.node_id,
                    evidence.snippet.trim(),
                    evidence
                        .source_id
                        .as_deref()
                        .or(evidence.source_path.as_deref())
                        .unwrap_or("unknown"),
                    evidence.id
                ))
            })
            .collect::<Vec<_>>();
        let attached_claims = snapshot
            .claims
            .iter()
            .filter(|claim| {
                claim
                    .topic_refs
                    .iter()
                    .any(|node_id| page_node_ids.contains(node_id))
            })
            .map(|claim| {
                format!(
                    "- `{}` {} _(sources: {}; evidence: {})_",
                    claim.status,
                    claim.statement,
                    join_or_none(&claim.source_refs),
                    join_or_none(&claim.evidence_refs)
                )
            })
            .collect::<Vec<_>>();
        let node_labels = snapshot
            .nodes
            .iter()
            .map(|node| (node.node_id.as_str(), node.label.as_str()))
            .collect::<BTreeMap<_, _>>();
        let topic_page_paths = snapshot
            .wiki_pages
            .iter()
            .filter(|page| page.path.starts_with("wiki/topics/"))
            .flat_map(|page| {
                page.node_refs
                    .iter()
                    .map(move |node_id| (node_id.as_str(), page.path.as_str()))
            })
            .collect::<BTreeMap<_, _>>();
        let attached_relations = snapshot
            .relations
            .iter()
            .filter(|relation| {
                page_node_ids.contains(&relation.source_node_id)
                    || page_node_ids.contains(&relation.target_node_id)
            })
            .map(|relation| {
                let source_label = node_labels
                    .get(relation.source_node_id.as_str())
                    .copied()
                    .unwrap_or(relation.source_node_id.as_str());
                let target_label = node_labels
                    .get(relation.target_node_id.as_str())
                    .copied()
                    .unwrap_or(relation.target_node_id.as_str());
                let source_link = topic_node_wiki_link(
                    source_label,
                    &relation.source_node_id,
                    &page.path,
                    &topic_page_paths,
                );
                let target_link = topic_node_wiki_link(
                    target_label,
                    &relation.target_node_id,
                    &page.path,
                    &topic_page_paths,
                );
                let relation_source_refs =
                    source_refs_for_evidence_ids(&relation.evidence_ids, &evidence_by_id);
                format!(
                    "- `{}` {} -> {} _(relation: {}; sources: {}; evidence: {})_",
                    relation.relation_id,
                    source_link,
                    target_link,
                    relation.label,
                    join_or_none(&relation_source_refs),
                    join_or_none(&relation.evidence_ids)
                )
            })
            .collect::<Vec<_>>();
        let source_references = topic_source_references_markdown(page, snapshot, &evidence_by_id);
        if !node_descriptions.is_empty()
            || !attached_claims.is_empty()
            || !attached_relations.is_empty()
            || !source_references.is_empty()
        {
            let mut body = page.body.trim_end().to_string();
            if !source_references.is_empty() {
                body.push_str("\n\n## Source References\n\n");
                body.push_str(&source_references.join("\n"));
                body.push('\n');
            }
            if !node_descriptions.is_empty() {
                body.push_str("\n\n## Node Description\n\n");
                body.push_str(&node_descriptions.join("\n"));
                body.push('\n');
            }
            if !attached_claims.is_empty() {
                body.push_str(
                    "\n\n## Claims\n\n_Source-backed claims linked to materialized evidence._\n\n",
                );
                body.push_str(&attached_claims.join("\n"));
                body.push('\n');
            }
            if !attached_relations.is_empty() {
                body.push_str("\n\n## Relations\n\n");
                body.push_str(&attached_relations.join("\n"));
                body.push('\n');
            }
            return body;
        }
    }
    page.body.clone()
}

pub(crate) fn topic_source_references_markdown(
    page: &WikiPage,
    snapshot: &BrainRepoSnapshot,
    evidence_by_id: &BTreeMap<&str, &EvidenceRef>,
) -> Vec<String> {
    let mut source_refs = page.source_refs.iter().cloned().collect::<BTreeSet<_>>();
    for evidence_id in &page.evidence_refs {
        if let Some(evidence) = evidence_by_id.get(evidence_id.as_str()) {
            if let Some(source_id) = evidence.source_id.as_deref() {
                if !source_id.trim().is_empty() {
                    source_refs.insert(source_id.to_string());
                }
            }
        }
    }
    let sources_by_id = snapshot
        .sources
        .iter()
        .map(|source| (source.source_id.as_str(), source))
        .collect::<BTreeMap<_, _>>();
    source_refs
        .into_iter()
        .map(|source_ref| {
            if let Some(source) = sources_by_id.get(source_ref.as_str()) {
                format!(
                    "- [{}](../sources/{}.md) _(source: `{}`; markdown: `{}`)_",
                    source.source_id,
                    sanitize_name(&source.source_id),
                    source.source_path,
                    source.markdown_path
                )
            } else {
                format!("- `{source_ref}`")
            }
        })
        .collect()
}

pub(crate) fn source_refs_for_evidence_ids(
    evidence_ids: &[String],
    evidence_by_id: &BTreeMap<&str, &EvidenceRef>,
) -> Vec<String> {
    evidence_ids
        .iter()
        .filter_map(|evidence_id| evidence_by_id.get(evidence_id.as_str()).copied())
        .filter_map(|evidence| {
            evidence
                .source_id
                .as_deref()
                .or(evidence.source_path.as_deref())
        })
        .filter(|source_ref| !source_ref.trim().is_empty())
        .map(ToString::to_string)
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

pub(crate) fn topic_node_wiki_link(
    label: &str,
    node_id: &str,
    current_page_path: &str,
    topic_page_paths: &BTreeMap<&str, &str>,
) -> String {
    let Some(target_path) = topic_page_paths.get(node_id).copied() else {
        return label.to_string();
    };
    if target_path == current_page_path {
        return label.to_string();
    }
    let relative_path = target_path
        .strip_prefix("wiki/topics/")
        .unwrap_or(target_path);
    format!("[{}]({})", label, relative_path)
}
