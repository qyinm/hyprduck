use super::*;

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
                valid_from: generated_at,
                valid_to: None,
                superseded_by: None,
            };
            if let Some(existing) = existing_node_by_id.get(node.node_id.as_str()) {
                if brain_node_record_content_matches(existing, &node) {
                    node.updated_at = existing.updated_at;
                    node.valid_from = existing.valid_from;
                    node.valid_to = existing.valid_to;
                    node.superseded_by = existing.superseded_by.clone();
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
                valid_from: generated_at,
                valid_to: None,
                superseded_by: None,
            };
            if let Some(existing) = existing_relation_by_id.get(relation.relation_id.as_str()) {
                if brain_relation_record_content_matches(existing, &relation) {
                    relation.updated_at = existing.updated_at;
                    relation.valid_from = existing.valid_from;
                    relation.valid_to = existing.valid_to;
                    relation.superseded_by = existing.superseded_by.clone();
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
                actor_id: "etyma-engine".into(),
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
                actor_id: "etyma-engine".into(),
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

