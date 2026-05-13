use super::*;

pub(super) struct PageSection {
    pub(super) page_index: usize,
    pub(super) page_label: String,
    pub(super) content: String,
    pub(super) markdown_path: Option<String>,
    pub(super) image_path: Option<String>,
}

#[derive(Debug, Clone)]
pub(super) struct ConceptAccumulator {
    pub(super) id: String,
    pub(super) label: String,
    pub(super) aliases: BTreeSet<String>,
    pub(super) evidence: Vec<EvidenceRef>,
    pub(super) page_labels: BTreeSet<String>,
}

#[derive(Debug, Clone)]
pub(super) struct ExtractionEvidenceRef {
    pub(super) id: String,
    pub(super) page_index: usize,
    pub(super) page_label: String,
    pub(super) snippet: String,
    pub(super) source_path: String,
    pub(super) source_id: Option<String>,
    pub(super) markdown_path: Option<String>,
    pub(super) image_path: Option<String>,
    pub(super) provenance: String,
}

#[derive(Debug, Clone)]
pub(super) struct ExtractedConcept {
    pub(super) id: String,
    pub(super) label: String,
    pub(super) aliases: BTreeSet<String>,
    pub(super) evidence_ids: Vec<String>,
    pub(super) page_labels: BTreeSet<String>,
}

#[derive(Debug, Clone)]
pub(super) struct ExtractedClaim {
    pub(super) id: String,
    pub(super) text: String,
    pub(super) subject_concept_id: String,
    pub(super) evidence_id: String,
}

#[derive(Debug, Clone)]
pub(super) struct ExtractedRelation {
    pub(super) source_concept_id: String,
    pub(super) target_concept_id: String,
    pub(super) relation_kind: BrainRelationKind,
    pub(super) confidence: f32,
    pub(super) evidence_ids: Vec<String>,
    pub(super) page_labels: BTreeSet<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct MarkdownNodeCandidate {
    pub(super) candidate_id: String,
    pub(super) label: String,
    pub(super) kind: BrainNodeKind,
    pub(super) source_path: String,
    pub(super) line_start: usize,
    pub(super) evidence_snippet: String,
    pub(super) confidence: f32,
    pub(super) reason: String,
    #[serde(default)]
    pub(super) matched_node_id: Option<String>,
    #[serde(default)]
    pub(super) matched_node_label: Option<String>,
    #[serde(default)]
    pub(super) match_score: Option<f32>,
    #[serde(default)]
    pub(super) match_reason: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct MarkdownRelationshipEvidence {
    pub(super) candidate_id: String,
    pub(super) evidence_id: String,
    pub(super) source_path: String,
    #[serde(default)]
    pub(super) source_id: Option<String>,
    #[serde(default)]
    pub(super) source_refs: Vec<String>,
    pub(super) line_start: usize,
    pub(super) snippet: String,
    pub(super) source_label: String,
    pub(super) target_label: String,
    pub(super) relation_kind: BrainRelationKind,
    pub(super) relation_label: String,
    pub(super) confidence: f32,
    pub(super) reason: String,
    #[serde(default)]
    pub(super) matched_source_node_id: Option<String>,
    #[serde(default)]
    pub(super) matched_target_node_id: Option<String>,
    #[serde(default)]
    pub(super) resolved_source_node_id: Option<String>,
    #[serde(default)]
    pub(super) resolved_target_node_id: Option<String>,
    #[serde(default)]
    pub(super) endpoint_resolution: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct MarkdownClaimCandidate {
    pub(super) candidate_id: String,
    pub(super) evidence_id: String,
    pub(super) statement: String,
    #[serde(default)]
    pub(super) classification: MarkdownClaimClassification,
    #[serde(default)]
    pub(super) durable: bool,
    #[serde(default)]
    pub(super) memory_candidate: bool,
    pub(super) source_path: String,
    #[serde(default)]
    pub(super) source_id: Option<String>,
    #[serde(default)]
    pub(super) source_refs: Vec<String>,
    pub(super) line_start: usize,
    pub(super) line_end: usize,
    pub(super) char_start: usize,
    pub(super) char_end: usize,
    pub(super) evidence_span: MarkdownEvidenceSpan,
    pub(super) evidence_snippet: String,
    #[serde(default)]
    pub(super) subject_labels: Vec<String>,
    #[serde(default)]
    pub(super) subject_refs: Vec<String>,
    pub(super) confidence: f32,
    pub(super) reason: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct MarkdownSignalArtifact {
    pub(super) source_path: String,
    #[serde(default)]
    pub(super) source_id: Option<String>,
    #[serde(default)]
    pub(super) source_refs: Vec<String>,
    #[serde(default)]
    pub(super) title: Option<String>,
    #[serde(default)]
    pub(super) headings: Vec<MarkdownHeadingSignal>,
    #[serde(default)]
    pub(super) links: Vec<MarkdownLinkSignal>,
    #[serde(default)]
    pub(super) entities: Vec<MarkdownEntitySignal>,
    #[serde(default)]
    pub(super) keywords: Vec<MarkdownKeywordSignal>,
    #[serde(default)]
    pub(super) related_pages: Vec<MarkdownRelatedPageSignal>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct MarkdownHeadingSignal {
    pub(super) text: String,
    pub(super) level: usize,
    pub(super) line_start: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct MarkdownLinkSignal {
    pub(super) label: String,
    pub(super) target: String,
    pub(super) kind: String,
    pub(super) line_start: usize,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct MarkdownEntitySignal {
    pub(super) label: String,
    pub(super) line_start: usize,
    pub(super) confidence: f32,
    pub(super) reason: String,
    #[serde(default)]
    pub(super) matched_node_id: Option<String>,
    #[serde(default)]
    pub(super) matched_node_label: Option<String>,
    #[serde(default)]
    pub(super) match_score: Option<f32>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct MarkdownKeywordSignal {
    pub(super) term: String,
    pub(super) count: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct MarkdownRelatedPageSignal {
    pub(super) page_id: String,
    pub(super) path: String,
    pub(super) title: String,
    pub(super) score: usize,
    pub(super) matched_terms: Vec<String>,
    pub(super) reason: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct MarkdownEvidenceSpan {
    pub(super) source_path: String,
    #[serde(default)]
    pub(super) source_id: Option<String>,
    pub(super) line_start: usize,
    pub(super) line_end: usize,
    pub(super) char_start: usize,
    pub(super) char_end: usize,
    pub(super) snippet: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum MarkdownClaimClassification {
    DurableFact,
    Decision,
}

impl Default for MarkdownClaimClassification {
    fn default() -> Self {
        Self::DurableFact
    }
}

#[derive(Debug, Clone)]
pub(super) struct ExtractionArtifact {
    pub(super) concepts: Vec<ExtractedConcept>,
    pub(super) claims: Vec<ExtractedClaim>,
    pub(super) relations: Vec<ExtractedRelation>,
    pub(super) evidence_refs: BTreeMap<String, ExtractionEvidenceRef>,
}

#[derive(Debug, Clone)]
pub(super) struct PageConceptSet {
    pub(super) page_index: usize,
    pub(super) page_label: String,
    pub(super) concept_ids: Vec<String>,
    pub(super) snippet: String,
    pub(super) markdown_path: Option<String>,
    pub(super) image_path: Option<String>,
}

#[derive(Debug, Clone)]
pub(super) struct CollectedConcepts {
    pub(super) concepts: Vec<ConceptAccumulator>,
    pub(super) page_concepts: Vec<PageConceptSet>,
    pub(super) relation_candidates: Vec<RelationCandidateAccumulator>,
}

#[derive(Debug, Clone)]
pub(super) struct EdgeAccumulator {
    pub(super) source_node_id: String,
    pub(super) target_node_id: String,
    pub(super) relation_kind: BrainRelationKind,
    pub(super) label: String,
    pub(super) confidence: Option<f32>,
    pub(super) evidence: Vec<EvidenceRef>,
    pub(super) page_labels: BTreeSet<String>,
}

#[derive(Debug, Clone)]
pub(super) struct RelationCandidateAccumulator {
    pub(super) source_node_id: String,
    pub(super) target_node_id: String,
    pub(super) relation_kind: BrainRelationKind,
    pub(super) confidence: f32,
    pub(super) evidence: Vec<EvidenceRef>,
    pub(super) page_labels: BTreeSet<String>,
}

#[derive(Debug, Clone)]
pub(super) struct StoredSourceRow {
    pub(super) summary: SourceSummary,
    pub(super) project_id: String,
    pub(super) manifest_path: String,
}

#[derive(Debug, Clone)]
pub(super) struct WorkspaceConceptAccumulator {
    pub(super) node_id: String,
    pub(super) canonical_name: String,
    pub(super) aliases: BTreeSet<String>,
    pub(super) evidence: Vec<EvidenceRef>,
    pub(super) confidence: Option<f32>,
}

pub(super) fn compile_knowledge_project(
    request: &CompileProjectRequest,
    markdown: &str,
    source_manifest: Option<&SourceArtifactManifest>,
) -> KnowledgeProject {
    let title = infer_markdown_title(&request.source_markdown_path, markdown);
    let mut page_sections = extract_page_sections(markdown);
    attach_page_artifacts_to_sections(&mut page_sections, source_manifest);
    let source_path = request
        .source_document_path
        .clone()
        .unwrap_or_else(|| request.source_markdown_path.clone());
    let source_node_id = source_manifest
        .map(|manifest| source_node_id(&manifest.source_id))
        .unwrap_or_else(|| "document".into());
    let source_label = source_manifest
        .map(source_label_from_manifest)
        .unwrap_or_else(|| title.clone());
    let source_path_for_evidence = source_manifest
        .map(|manifest| manifest.source_path.clone())
        .unwrap_or_else(|| source_path.clone());
    let source_id_for_evidence = source_manifest.map(|manifest| manifest.source_id.clone());
    let project_id = source_manifest
        .map(|manifest| build_source_backed_project_id(&manifest.workspace_id, &manifest.source_id))
        .unwrap_or_else(|| build_project_id(request));

    let node_candidates = source_manifest
        .filter(|manifest| manifest.format == DocumentFormat::Markdown)
        .map(|manifest| {
            extract_markdown_node_candidates_for_workspace(
                markdown,
                &source_path_for_evidence,
                workspace_root_from_manifest(manifest).as_path(),
            )
            .unwrap_or_else(|_| {
                extract_markdown_node_candidates(markdown, &source_path_for_evidence)
            })
        })
        .unwrap_or_default();
    let claim_candidates = extract_markdown_claim_candidates(
        markdown,
        &source_path_for_evidence,
        source_id_for_evidence.as_deref(),
        &node_candidates,
    );
    let extraction = build_extraction_artifact(
        &page_sections,
        markdown,
        &source_path_for_evidence,
        source_id_for_evidence.as_deref(),
        &node_candidates,
        &claim_candidates,
    );
    let collected = collected_concepts_from_artifact(&extraction);
    let concept_accumulators = collected.concepts;
    let concept_count = concept_accumulators.len();
    let mut document_node = GraphNodeSummary {
        id: source_node_id.clone(),
        label: source_label.clone(),
        kind: source_manifest
            .map(|_| GraphNodeKind::Source)
            .unwrap_or(GraphNodeKind::Document),
        confidence: Some(if concept_count > 0 { 0.78 } else { 0.42 }),
        related_count: 0,
        evidence_count: page_sections.len(),
        position: GraphNodePosition { x: 50.0, y: 14.0 },
    };

    let concept_positions = layout_concept_positions(concept_count.max(1));
    let mut concept_nodes = concept_accumulators
        .iter()
        .enumerate()
        .map(|(index, concept)| GraphNodeSummary {
            id: concept.id.clone(),
            label: concept.label.clone(),
            kind: GraphNodeKind::Concept,
            confidence: Some((0.62 + (concept.evidence.len().min(3) as f32 * 0.08)).min(0.91)),
            related_count: 0,
            evidence_count: concept.evidence.len(),
            position: concept_positions
                .get(index)
                .cloned()
                .unwrap_or(GraphNodePosition { x: 50.0, y: 54.0 }),
        })
        .collect::<Vec<_>>();

    let concept_by_id = concept_accumulators
        .iter()
        .map(|concept| (concept.id.clone(), concept.clone()))
        .collect::<BTreeMap<_, _>>();
    let mut details_by_node_id = BTreeMap::new();
    let mut edge_details_by_id = BTreeMap::new();
    let mut answer_by_node_id = BTreeMap::new();
    let document_evidence = page_sections
        .iter()
        .take(3)
        .enumerate()
        .map(|(index, section)| EvidenceRef {
            id: format!("ev-document-{}", index + 1),
            page_label: section.page_label.clone(),
            page_index: Some(section.page_index),
            snippet: excerpt(&section.content, 180),
            source_path: Some(source_path_for_evidence.clone()),
            source_id: source_id_for_evidence.clone(),
            markdown_path: section.markdown_path.clone(),
            image_path: section.image_path.clone(),
            provenance: Some(format!(
                "Document-level evidence extracted from {}.",
                section.page_label
            )),
        })
        .collect::<Vec<_>>();

    let (edges, built_edge_details_by_id, related_count_by_node_id, connected_node_ids_by_node_id) =
        build_relation_edges(
            &document_node,
            &concept_accumulators,
            &collected.page_concepts,
            &collected.relation_candidates,
            &source_path_for_evidence,
            source_id_for_evidence.as_deref(),
        );
    edge_details_by_id.extend(built_edge_details_by_id);
    document_node.related_count = related_count_by_node_id
        .get(document_node.id.as_str())
        .copied()
        .unwrap_or(0);
    for node in &mut concept_nodes {
        node.related_count = related_count_by_node_id
            .get(node.id.as_str())
            .copied()
            .unwrap_or(0);
    }

    details_by_node_id.insert(
        document_node.id.clone(),
        GraphNodeDetail {
            node: document_node.clone(),
            canonical_name: source_label.clone(),
            aliases: vec![if source_manifest.is_some() {
                "Immutable source".into()
            } else {
                "Imported document".into()
            }],
            description: format!(
                "HyprDuck compiled {} concept nodes from {} visible page sections. Every node below keeps direct evidence back to the imported document.",
                concept_count,
                page_sections.len()
            ),
            evidence: document_evidence.clone(),
            actions: Vec::new(),
            source: source_manifest.map(source_backing_from_manifest),
        },
    );
    answer_by_node_id.insert(
        document_node.id.clone(),
        AnswerResponse {
            status: if concept_count > 0 {
                AnswerStatus::Grounded
            } else {
                AnswerStatus::LowConfidence
            },
            text: Some(format!(
                "HyprDuck found {} concept nodes across {} page sections in this import.",
                concept_count,
                page_sections.len()
            )),
            explanation:
                "This document-level answer is grounded in the concept nodes and visible evidence HyprDuck compiled from the markdown package."
                    .into(),
            citations: document_evidence.clone(),
            related_node_ids: connected_node_ids_by_node_id
                .get(document_node.id.as_str())
                .map(|related| related.iter().cloned().collect())
                .unwrap_or_default(),
            suggested_actions: vec![
                SuggestedAction {
                    kind: SuggestedActionKind::InspectEvidence,
                    label: "Inspect evidence".into(),
                    description:
                        "Review the cited snippets before trusting the document-wide summary."
                            .into(),
                },
                SuggestedAction {
                    kind: SuggestedActionKind::AskDifferentQuestion,
                    label: "Ask a narrower question".into(),
                    description:
                        "Grounded answers get stronger when you focus on one concept at a time."
                            .into(),
                },
            ],
        },
    );

    for node in &concept_nodes {
        let concept = concept_by_id
            .get(&node.id)
            .expect("concept node should have backing accumulator");
        let aliases = concept
            .aliases
            .iter()
            .filter(|alias| alias.as_str() != concept.label)
            .cloned()
            .collect::<Vec<_>>();
        let actions = correction_actions_for_detail(&concept.label, &aliases);
        details_by_node_id.insert(
            node.id.clone(),
            GraphNodeDetail {
                node: node.clone(),
                canonical_name: concept.label.clone(),
                aliases,
                description: format!(
                    "Compiled from {} evidence refs across {} page(s). HyprDuck is still conservative and only shows evidence-backed concept nodes.",
                    concept.evidence.len(),
                    concept.page_labels.len()
                ),
                evidence: concept.evidence.clone(),
                actions,
                source: None,
            },
        );
        answer_by_node_id.insert(
            node.id.clone(),
            AnswerResponse {
                status: AnswerStatus::Grounded,
                text: Some(format!(
                    "{} appears in {} evidence refs across {} page(s).",
                    concept.label,
                    concept.evidence.len(),
                    concept.page_labels.len()
                )),
                explanation:
                    "This answer is grounded in the evidence attached to the selected concept node."
                        .into(),
                citations: concept.evidence.iter().take(3).cloned().collect(),
                related_node_ids: connected_node_ids_by_node_id
                    .get(node.id.as_str())
                    .map(|related| related.iter().cloned().collect())
                    .unwrap_or_else(|| vec![source_node_id.clone()]),
                suggested_actions: vec![SuggestedAction {
                    kind: SuggestedActionKind::InspectEvidence,
                    label: "Inspect evidence".into(),
                    description:
                        "Use the cited snippets to verify the concept before acting on it.".into(),
                }],
            },
        );
    }

    let mut nodes = Vec::with_capacity(concept_nodes.len() + 1);
    nodes.push(document_node.clone());
    nodes.extend(concept_nodes.iter().cloned());

    let evidence_count = concept_accumulators
        .iter()
        .map(|concept| concept.evidence.len())
        .sum::<usize>()
        + document_evidence.len()
        + edges.iter().map(|edge| edge.evidence_count).sum::<usize>();

    KnowledgeProject {
        summary: ProjectOverview {
            project_id,
            title,
            status: if concept_count > 0 {
                ProjectStatus::Ready
            } else {
                ProjectStatus::Degraded
            },
            stale: false,
            summary: format!(
                "Compiled {} concept nodes from {} page sections. HyprDuck only shows nodes with visible evidence.",
                concept_count,
                page_sections.len()
            ),
            document_count: 1,
            node_count: nodes.len(),
            relationship_count: edges.len(),
            evidence_count,
        },
        nodes,
        edges,
        details_by_node_id,
        edge_details_by_id,
        answer_by_node_id,
    }
}

pub(super) fn aggregate_workspace_project(
    workspace_id: &str,
    rows: Vec<(StoredSourceRow, Option<KnowledgeProject>)>,
) -> KnowledgeProject {
    let source_count = rows.len();
    let mut source_nodes = Vec::new();
    let mut source_details = BTreeMap::new();
    let mut source_answers = BTreeMap::new();
    let mut concept_accumulators = BTreeMap::<String, WorkspaceConceptAccumulator>::new();
    let mut aggregate_key_by_concept_key = BTreeMap::<String, String>::new();
    let mut source_concept_edges = BTreeMap::<String, StoredEdgeAccumulator>::new();
    let mut relation_edges = BTreeMap::<String, StoredEdgeAccumulator>::new();

    for (source_index, (row, project)) in rows.iter().enumerate() {
        let source_node_id = source_node_id(&row.summary.source_id);
        let source_label = source_label_from_summary(&row.summary);
        let source_detail_from_project = project.as_ref().and_then(|project| {
            project.details_by_node_id.get(&source_node_id).or_else(|| {
                project
                    .details_by_node_id
                    .values()
                    .find(|detail| is_source_like_node_kind(detail.node.kind))
            })
        });
        let source_evidence = source_detail_from_project
            .map(|detail| detail.evidence.clone())
            .unwrap_or_default();
        let source_node = GraphNodeSummary {
            id: source_node_id.clone(),
            label: source_label.clone(),
            kind: GraphNodeKind::Source,
            confidence: Some(if project.is_some() { 0.72 } else { 0.28 }),
            related_count: 0,
            evidence_count: source_evidence.len().max(row.summary.success_count),
            position: source_node_position(source_index, source_count),
        };
        source_nodes.push(source_node.clone());
        source_details.insert(
            source_node_id.clone(),
            GraphNodeDetail {
                node: source_node.clone(),
                canonical_name: source_label.clone(),
                aliases: vec!["Workspace source".into()],
                description: format!(
                    "Immutable source in workspace {workspace_id}. HyprDuck keeps source artifacts addressable while graph evidence is aggregated across the workspace."
                ),
                evidence: source_evidence.clone(),
                actions: Vec::new(),
                source: Some(source_backing_from_summary(&row.summary, &row.manifest_path)),
            },
        );
        source_answers.insert(
            source_node_id.clone(),
            AnswerResponse {
                status: if project.is_some() {
                    AnswerStatus::LowConfidence
                } else {
                    AnswerStatus::Blocked
                },
                text: None,
                explanation: if project.is_some() {
                    "This source contributes evidence to the workspace graph.".into()
                } else {
                    "This source is registered in the workspace, but no compiled graph snapshot was found yet.".into()
                },
                citations: source_evidence.iter().take(3).cloned().collect(),
                related_node_ids: Vec::new(),
                suggested_actions: vec![SuggestedAction {
                    kind: SuggestedActionKind::InspectEvidence,
                    label: "Inspect source artifacts".into(),
                    description:
                        "Open the source detail inspector to review copied source and derived artifacts."
                            .into(),
                }],
            },
        );

        let Some(project) = project else {
            continue;
        };

        let mut concept_id_map = BTreeMap::<String, String>::new();
        for detail in project.details_by_node_id.values() {
            if detail.node.kind != GraphNodeKind::Concept {
                continue;
            }
            let concept_keys = concept_identity_keys(detail);
            let Some(canonical_key) = concept_keys.first().cloned() else {
                continue;
            };
            let existing_aggregate_keys = concept_keys
                .iter()
                .filter_map(|key| aggregate_key_by_concept_key.get(key).cloned())
                .collect::<BTreeSet<_>>();
            let aggregate_key = existing_aggregate_keys
                .iter()
                .next()
                .cloned()
                .unwrap_or(canonical_key);
            if !existing_aggregate_keys.is_empty() {
                merge_workspace_concept_groups(
                    &aggregate_key,
                    &existing_aggregate_keys,
                    &mut concept_accumulators,
                    &mut aggregate_key_by_concept_key,
                );
            }
            for key in &concept_keys {
                aggregate_key_by_concept_key.insert(key.clone(), aggregate_key.clone());
            }
            let aggregate_node_id = format!("concept-{aggregate_key}");
            concept_id_map.insert(detail.node.id.clone(), aggregate_node_id.clone());
            let accumulator = concept_accumulators
                .entry(aggregate_key)
                .or_insert_with(|| WorkspaceConceptAccumulator {
                    node_id: aggregate_node_id.clone(),
                    canonical_name: detail.canonical_name.clone(),
                    aliases: BTreeSet::new(),
                    evidence: Vec::new(),
                    confidence: detail.node.confidence,
                });
            accumulator.aliases.extend(detail.aliases.iter().cloned());
            if accumulator.canonical_name != detail.canonical_name {
                accumulator.aliases.insert(detail.canonical_name.clone());
            }
            accumulator.evidence.extend(detail.evidence.iter().cloned());
            accumulator.confidence = match (accumulator.confidence, detail.node.confidence) {
                (Some(left), Some(right)) => Some(left.max(right).min(0.94)),
                (Some(left), None) => Some(left),
                (None, Some(right)) => Some(right),
                (None, None) => None,
            };

            let edge_id = relation_edge_id(
                RelationKind::SourceDocument,
                &source_node_id,
                &aggregate_node_id,
            );
            let edge_accumulator =
                source_concept_edges
                    .entry(edge_id)
                    .or_insert_with(|| StoredEdgeAccumulator {
                        kind: RelationKind::SourceDocument,
                        source_node_id: source_node_id.clone(),
                        target_node_id: aggregate_node_id.clone(),
                        label: "Compiled from source".into(),
                        confidence: Some(0.76),
                        evidence: Vec::new(),
                    });
            edge_accumulator
                .evidence
                .extend(detail.evidence.iter().cloned());
        }

        for edge in &project.edges {
            if edge.kind != RelationKind::RelatedTo {
                continue;
            }
            let Some(left) = concept_id_map.get(&edge.source_node_id).cloned() else {
                continue;
            };
            let Some(right) = concept_id_map.get(&edge.target_node_id).cloned() else {
                continue;
            };
            if left == right {
                continue;
            }
            let (source_node_id, target_node_id) = if left <= right {
                (left, right)
            } else {
                (right, left)
            };
            let edge_id =
                relation_edge_id(RelationKind::RelatedTo, &source_node_id, &target_node_id);
            let evidence = project
                .edge_details_by_id
                .get(&edge.id)
                .map(|detail| detail.evidence.clone())
                .unwrap_or_default();
            let accumulator =
                relation_edges
                    .entry(edge_id)
                    .or_insert_with(|| StoredEdgeAccumulator {
                        kind: RelationKind::RelatedTo,
                        source_node_id: source_node_id.clone(),
                        target_node_id: target_node_id.clone(),
                        label: normalized_edge_label(RelationKind::RelatedTo, &edge.label),
                        confidence: edge.confidence,
                        evidence: Vec::new(),
                    });
            accumulator.label =
                preferred_edge_label(&accumulator.label, &edge.label, RelationKind::RelatedTo);
            accumulator.confidence = match (accumulator.confidence, edge.confidence) {
                (Some(left), Some(right)) => Some(left.max(right)),
                (Some(left), None) => Some(left),
                (None, Some(right)) => Some(right),
                (None, None) => None,
            };
            accumulator.evidence.extend(evidence.into_iter());
        }
    }

    source_concept_edges =
        remap_workspace_edge_accumulators(source_concept_edges, &aggregate_key_by_concept_key);
    relation_edges =
        remap_workspace_edge_accumulators(relation_edges, &aggregate_key_by_concept_key);
    for accumulator in concept_accumulators.values_mut() {
        accumulator.evidence = dedupe_evidence(std::mem::take(&mut accumulator.evidence));
    }

    let concept_positions = layout_concept_positions(concept_accumulators.len().max(1));
    let mut concept_nodes = Vec::new();
    let mut details_by_node_id = source_details;
    let mut answer_by_node_id = source_answers;
    for (index, accumulator) in concept_accumulators.values().enumerate() {
        let aliases = accumulator
            .aliases
            .iter()
            .filter(|alias| alias.as_str() != accumulator.canonical_name)
            .cloned()
            .collect::<Vec<_>>();
        let source_ids = accumulator
            .evidence
            .iter()
            .filter_map(|evidence| evidence.source_id.clone())
            .collect::<BTreeSet<_>>();
        let node = GraphNodeSummary {
            id: accumulator.node_id.clone(),
            label: accumulator.canonical_name.clone(),
            kind: GraphNodeKind::Concept,
            confidence: accumulator.confidence,
            related_count: 0,
            evidence_count: accumulator.evidence.len(),
            position: concept_positions
                .get(index)
                .cloned()
                .unwrap_or(GraphNodePosition { x: 50.0, y: 54.0 }),
        };
        concept_nodes.push(node.clone());
        details_by_node_id.insert(
            node.id.clone(),
            GraphNodeDetail {
                node: node.clone(),
                canonical_name: accumulator.canonical_name.clone(),
                aliases,
                description: format!(
                    "Workspace concept compiled from {} evidence refs across {} source(s).",
                    accumulator.evidence.len(),
                    source_ids.len()
                ),
                evidence: accumulator.evidence.clone(),
                actions: Vec::new(),
                source: None,
            },
        );
        answer_by_node_id.insert(
            node.id.clone(),
            AnswerResponse {
                status: AnswerStatus::Grounded,
                text: Some(format!(
                    "{} appears in {} evidence refs across {} source(s).",
                    accumulator.canonical_name,
                    accumulator.evidence.len(),
                    source_ids.len()
                )),
                explanation:
                    "This workspace answer is grounded in evidence aggregated from source-backed imports."
                        .into(),
                citations: accumulator.evidence.iter().take(3).cloned().collect(),
                related_node_ids: Vec::new(),
                suggested_actions: vec![SuggestedAction {
                    kind: SuggestedActionKind::InspectEvidence,
                    label: "Inspect evidence".into(),
                    description:
                        "Use the cited snippets to verify the workspace concept before acting on it."
                            .into(),
                }],
            },
        );
    }

    let mut edges = Vec::new();
    let mut edge_details_by_id = BTreeMap::new();
    for accumulator in source_concept_edges
        .into_values()
        .chain(relation_edges.into_values())
    {
        let edge_id = relation_edge_id(
            accumulator.kind,
            &accumulator.source_node_id,
            &accumulator.target_node_id,
        );
        let edge = RelationEdgeSummary {
            id: edge_id.clone(),
            source_node_id: accumulator.source_node_id,
            target_node_id: accumulator.target_node_id,
            kind: accumulator.kind,
            label: accumulator.label,
            confidence: accumulator.confidence,
            evidence_count: accumulator.evidence.len(),
        };
        edge_details_by_id.insert(
            edge_id,
            RelationEdgeDetail {
                edge: edge.clone(),
                explanation: String::new(),
                evidence: accumulator.evidence,
            },
        );
        edges.push(edge);
    }

    let mut nodes = Vec::with_capacity(source_nodes.len() + concept_nodes.len());
    nodes.extend(source_nodes);
    nodes.extend(concept_nodes);
    finalize_workspace_project(
        workspace_id,
        nodes,
        edges,
        details_by_node_id,
        edge_details_by_id,
        answer_by_node_id,
        source_count,
    )
}

pub(super) fn merge_workspace_concept_groups(
    aggregate_key: &str,
    existing_aggregate_keys: &BTreeSet<String>,
    concept_accumulators: &mut BTreeMap<String, WorkspaceConceptAccumulator>,
    aggregate_key_by_concept_key: &mut BTreeMap<String, String>,
) {
    if existing_aggregate_keys.len() <= 1 {
        return;
    }

    let mut merged_accumulator = concept_accumulators
        .remove(aggregate_key)
        .unwrap_or_else(|| WorkspaceConceptAccumulator {
            node_id: format!("concept-{aggregate_key}"),
            canonical_name: aggregate_key.to_string(),
            aliases: BTreeSet::new(),
            evidence: Vec::new(),
            confidence: None,
        });

    for stale_key in existing_aggregate_keys {
        if stale_key == aggregate_key {
            continue;
        }
        if let Some(stale_accumulator) = concept_accumulators.remove(stale_key) {
            merged_accumulator
                .aliases
                .insert(stale_accumulator.canonical_name.clone());
            merged_accumulator
                .aliases
                .extend(stale_accumulator.aliases.into_iter());
            merged_accumulator
                .evidence
                .extend(stale_accumulator.evidence.into_iter());
            merged_accumulator.confidence =
                match (merged_accumulator.confidence, stale_accumulator.confidence) {
                    (Some(left), Some(right)) => Some(left.max(right).min(0.94)),
                    (Some(left), None) => Some(left),
                    (None, Some(right)) => Some(right),
                    (None, None) => None,
                };
        }
    }

    merged_accumulator.evidence = dedupe_evidence(merged_accumulator.evidence);
    for mapped_aggregate_key in aggregate_key_by_concept_key.values_mut() {
        if existing_aggregate_keys.contains(mapped_aggregate_key) {
            *mapped_aggregate_key = aggregate_key.to_string();
        }
    }
    concept_accumulators.insert(aggregate_key.to_string(), merged_accumulator);
}

pub(super) fn remap_workspace_edge_accumulators(
    accumulators: BTreeMap<String, StoredEdgeAccumulator>,
    aggregate_key_by_concept_key: &BTreeMap<String, String>,
) -> BTreeMap<String, StoredEdgeAccumulator> {
    let mut remapped = BTreeMap::<String, StoredEdgeAccumulator>::new();
    for mut accumulator in accumulators.into_values() {
        accumulator.source_node_id = remap_workspace_concept_node_id(
            &accumulator.source_node_id,
            aggregate_key_by_concept_key,
        );
        accumulator.target_node_id = remap_workspace_concept_node_id(
            &accumulator.target_node_id,
            aggregate_key_by_concept_key,
        );
        if accumulator.source_node_id == accumulator.target_node_id {
            continue;
        }
        let edge_id = relation_edge_id(
            accumulator.kind,
            &accumulator.source_node_id,
            &accumulator.target_node_id,
        );
        let existing = remapped
            .entry(edge_id)
            .or_insert_with(|| StoredEdgeAccumulator {
                kind: accumulator.kind,
                source_node_id: accumulator.source_node_id.clone(),
                target_node_id: accumulator.target_node_id.clone(),
                label: accumulator.label.clone(),
                confidence: accumulator.confidence,
                evidence: Vec::new(),
            });
        existing.label =
            preferred_edge_label(&existing.label, &accumulator.label, accumulator.kind);
        existing.confidence = match (existing.confidence, accumulator.confidence) {
            (Some(left), Some(right)) => Some(left.max(right).min(0.94)),
            (Some(left), None) => Some(left),
            (None, Some(right)) => Some(right),
            (None, None) => None,
        };
        existing.evidence.extend(accumulator.evidence.into_iter());
    }
    for accumulator in remapped.values_mut() {
        accumulator.evidence = dedupe_evidence(std::mem::take(&mut accumulator.evidence));
    }
    remapped
}

pub(super) fn remap_workspace_concept_node_id(
    node_id: &str,
    aggregate_key_by_concept_key: &BTreeMap<String, String>,
) -> String {
    let Some(key) = node_id.strip_prefix("concept-") else {
        return node_id.to_string();
    };
    aggregate_key_by_concept_key
        .get(key)
        .map(|aggregate_key| format!("concept-{aggregate_key}"))
        .unwrap_or_else(|| node_id.to_string())
}

pub(super) fn finalize_workspace_project(
    workspace_id: &str,
    mut nodes: Vec<GraphNodeSummary>,
    edges: Vec<RelationEdgeSummary>,
    mut details_by_node_id: BTreeMap<String, GraphNodeDetail>,
    mut edge_details_by_id: BTreeMap<String, RelationEdgeDetail>,
    mut answer_by_node_id: BTreeMap<String, AnswerResponse>,
    source_count: usize,
) -> KnowledgeProject {
    let mut related_count_by_node_id = BTreeMap::<String, usize>::new();
    let mut connected_node_ids_by_node_id = BTreeMap::<String, BTreeSet<String>>::new();
    for edge in &edges {
        note_relation(
            &mut related_count_by_node_id,
            &mut connected_node_ids_by_node_id,
            &edge.source_node_id,
            &edge.target_node_id,
        );
    }
    for node in &mut nodes {
        node.related_count = related_count_by_node_id.get(&node.id).copied().unwrap_or(0);
        if let Some(detail) = details_by_node_id.get_mut(&node.id) {
            detail.node = node.clone();
        }
        if let Some(answer) = answer_by_node_id.get_mut(&node.id) {
            answer.related_node_ids = connected_node_ids_by_node_id
                .get(&node.id)
                .map(|related| related.iter().cloned().collect())
                .unwrap_or_default();
        }
    }

    let label_by_node_id = nodes
        .iter()
        .map(|node| (node.id.clone(), node.label.clone()))
        .collect::<BTreeMap<_, _>>();
    for edge in &edges {
        if let Some(detail) = edge_details_by_id.get_mut(&edge.id) {
            detail.edge = edge.clone();
            detail.explanation = edge_explanation(edge, &label_by_node_id, &detail.evidence);
        }
    }

    let concept_count = nodes
        .iter()
        .filter(|node| node.kind == GraphNodeKind::Concept)
        .count();
    let evidence_count = details_by_node_id
        .values()
        .map(|detail| detail.evidence.len())
        .sum::<usize>()
        + edge_details_by_id
            .values()
            .map(|detail| detail.evidence.len())
            .sum::<usize>();

    KnowledgeProject {
        summary: ProjectOverview {
            project_id: workspace_project_id(workspace_id),
            title: "Workspace knowledge".into(),
            status: if concept_count > 0 {
                ProjectStatus::Ready
            } else {
                ProjectStatus::Degraded
            },
            stale: false,
            summary: format!(
                "Workspace contains {} sources, {} concept nodes, and {} evidence-backed relationships.",
                source_count,
                concept_count,
                edges.len()
            ),
            document_count: source_count,
            node_count: nodes.len(),
            relationship_count: edges.len(),
            evidence_count,
        },
        nodes,
        edges,
        details_by_node_id,
        edge_details_by_id,
        answer_by_node_id,
    }
}

pub(super) fn workspace_project_id(workspace_id: &str) -> String {
    format!("workspace:{workspace_id}")
}

pub(super) fn workspace_id_from_project_id(project_id: &str) -> Option<&str> {
    project_id
        .strip_prefix("workspace:")
        .filter(|workspace_id| !workspace_id.trim().is_empty())
}

pub(super) fn matching_source_concept_node_ids(
    project: &KnowledgeProject,
    aggregate_detail: &GraphNodeDetail,
) -> Vec<String> {
    let aggregate_keys = concept_identity_keys(aggregate_detail)
        .into_iter()
        .collect::<BTreeSet<_>>();
    let aggregate_source_ids = aggregate_detail
        .evidence
        .iter()
        .filter_map(|evidence| evidence.source_id.as_deref())
        .collect::<BTreeSet<_>>();

    project
        .details_by_node_id
        .values()
        .filter(|detail| detail.node.kind == GraphNodeKind::Concept)
        .filter(|detail| {
            concept_identity_keys(detail)
                .iter()
                .any(|key| aggregate_keys.contains(key))
                && (aggregate_source_ids.is_empty()
                    || detail.evidence.iter().any(|evidence| {
                        evidence
                            .source_id
                            .as_deref()
                            .is_some_and(|source_id| aggregate_source_ids.contains(source_id))
                    }))
        })
        .map(|detail| detail.node.id.clone())
        .collect()
}

pub(super) fn source_label_from_summary(summary: &SourceSummary) -> String {
    Path::new(&summary.original_path)
        .file_name()
        .or_else(|| Path::new(&summary.source_path).file_name())
        .map(|value| value.to_string_lossy().into_owned())
        .unwrap_or_else(|| summary.source_id.clone())
}

pub(super) fn source_backing_from_summary(
    summary: &SourceSummary,
    manifest_path: &str,
) -> SourceBacking {
    SourceBacking {
        workspace_id: summary.workspace_id.clone(),
        source_id: summary.source_id.clone(),
        original_path: summary.original_path.clone(),
        source_path: summary.source_path.clone(),
        markdown_path: summary.markdown_path.clone(),
        format: document_format_slug(&summary.format).into(),
        status: ingest_status_slug(&summary.status).into(),
        page_count: summary.page_count,
        success_count: summary.success_count,
        failed_count: summary.failed_count,
        description: summary.description.clone(),
        user_context: summary.user_context.clone(),
        ingest_instruction: summary.ingest_instruction.clone(),
        updated_at: summary.updated_at,
        manifest_path: Some(manifest_path.to_string()),
    }
}

pub(super) fn build_brain_repo_snapshot(
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
    for (row, _) in rows {
        for candidate in read_markdown_claim_candidates_for_row(row) {
            evidence_by_id
                .entry(candidate.evidence_id.clone())
                .or_insert_with(|| EvidenceRef {
                    id: candidate.evidence_id.clone(),
                    page_label: "Imported text".into(),
                    page_index: Some(0),
                    snippet: candidate.evidence_snippet,
                    source_path: Some(candidate.source_path),
                    source_id: candidate
                        .source_id
                        .or_else(|| Some(row.summary.source_id.clone())),
                    markdown_path: Some(row.summary.markdown_path.clone()),
                    image_path: None,
                    provenance: Some(format!(
                        "Claim candidate extracted from markdown line {} during autonomous ingest.",
                        candidate.line_start
                    )),
                });
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

    let mut claims = aggregate
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
    for (row, _) in rows {
        claims.extend(
            read_markdown_claim_candidates_for_row(row)
                .into_iter()
                .map(|candidate| ClaimRecord {
                    claim_id: format!("claim-{}", bounded_artifact_key(&candidate.statement, 80)),
                    workspace_id: workspace_id.to_string(),
                    statement: candidate.statement,
                    topic_refs: candidate.subject_refs,
                    source_refs: if candidate.source_refs.is_empty() {
                        vec![row.summary.source_id.clone()]
                    } else {
                        candidate.source_refs
                    },
                    evidence_refs: vec![candidate.evidence_id],
                    status: "candidate".into(),
                    updated_at: generated_at,
                }),
        );
    }
    claims = merge_matching_claim_records(claims);

    let memories =
        build_durable_memory_records(workspace_id, rows, generated_at, existing_memories);
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

    let extractions = build_structured_extraction_artifacts(workspace_id, rows, generated_at);
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
                actor_id: "duckdocs-engine".into(),
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
                actor_id: "duckdocs-engine".into(),
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

pub(super) fn merge_matching_claim_records(claims: Vec<ClaimRecord>) -> Vec<ClaimRecord> {
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

pub(super) fn merge_claim_record(existing: &mut ClaimRecord, incoming: ClaimRecord) {
    existing.topic_refs = merge_string_refs(&existing.topic_refs, &incoming.topic_refs);
    existing.source_refs = merge_string_refs(&existing.source_refs, &incoming.source_refs);
    existing.evidence_refs = merge_string_refs(&existing.evidence_refs, &incoming.evidence_refs);
    if claim_status_rank(&incoming.status) > claim_status_rank(&existing.status) {
        existing.status = incoming.status;
    }
    existing.updated_at = existing.updated_at.max(incoming.updated_at);
}

pub(super) fn merge_string_refs(left: &[String], right: &[String]) -> Vec<String> {
    left.iter()
        .chain(right.iter())
        .cloned()
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

pub(super) fn claim_status_rank(status: &str) -> u8 {
    match status {
        "supported" => 3,
        "accepted" => 2,
        "candidate" => 1,
        _ => 0,
    }
}

pub(super) fn build_durable_memory_records(
    workspace_id: &str,
    rows: &[(StoredSourceRow, Option<KnowledgeProject>)],
    generated_at: u64,
    existing_memories: &[MemoryRecord],
) -> Vec<MemoryRecord> {
    let mut memories = BTreeMap::<String, MemoryRecord>::new();
    for (row, _) in rows {
        for candidate in read_markdown_claim_candidates_for_row(row)
            .into_iter()
            .filter(|candidate| candidate.durable && candidate.memory_candidate)
        {
            let generated_memory_id = memory_id_for_claim_candidate(&candidate);
            let memory_id =
                matching_memory_id_for_candidate(&candidate, existing_memories, memories.values())
                    .unwrap_or(generated_memory_id);
            let mut memory = MemoryRecord {
                memory_id: memory_id.clone(),
                workspace_id: workspace_id.to_string(),
                scope: BrainScope::Project,
                title: durable_memory_title(&candidate),
                body: candidate.statement,
                source_refs: if candidate.source_refs.is_empty() {
                    vec![row.summary.source_id.clone()]
                } else {
                    candidate.source_refs
                },
                evidence_refs: vec![candidate.evidence_id],
                created_at: generated_at,
                updated_at: generated_at,
            };
            if let Some(existing) = existing_memories
                .iter()
                .find(|existing| existing.memory_id == memory_id)
            {
                if memory_record_content_matches(existing, &memory) {
                    memory.created_at = existing.created_at;
                    memory.updated_at = existing.updated_at;
                } else {
                    merge_memory_record(&mut memory, existing.clone());
                }
            }
            match memories.get_mut(&memory_id) {
                Some(existing) => merge_memory_record(existing, memory),
                None => {
                    memories.insert(memory_id, memory);
                }
            }
        }
    }
    memories.into_values().collect()
}

pub(super) fn matching_memory_id_for_candidate<'a>(
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

pub(super) fn memory_match_score(incoming: &MemoryRecord, existing: &MemoryRecord) -> Option<u16> {
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

pub(super) fn durable_memory_title(candidate: &MarkdownClaimCandidate) -> String {
    let prefix = match candidate.classification {
        MarkdownClaimClassification::Decision => "Decision",
        MarkdownClaimClassification::DurableFact => "Fact",
    };
    format!("{}: {}", prefix, excerpt(&candidate.statement, 72))
}

pub(super) fn memory_id_for_claim_candidate(candidate: &MarkdownClaimCandidate) -> String {
    format!("memory-{}", bounded_artifact_key(&candidate.statement, 96))
}

pub(super) fn markdown_claim_classification_slug(
    classification: MarkdownClaimClassification,
) -> &'static str {
    match classification {
        MarkdownClaimClassification::Decision => "decision",
        MarkdownClaimClassification::DurableFact => "durable_fact",
    }
}

pub(super) fn merge_memory_record(existing: &mut MemoryRecord, incoming: MemoryRecord) {
    existing.source_refs = merge_string_refs(&existing.source_refs, &incoming.source_refs);
    existing.evidence_refs = merge_string_refs(&existing.evidence_refs, &incoming.evidence_refs);
    existing.created_at = existing.created_at.min(incoming.created_at);
    existing.updated_at = existing.updated_at.max(incoming.updated_at);
}

pub(super) fn memory_record_content_matches(left: &MemoryRecord, right: &MemoryRecord) -> bool {
    left.workspace_id == right.workspace_id
        && left.scope == right.scope
        && left.title == right.title
        && left.body == right.body
        && merge_string_refs(&left.source_refs, &[]) == merge_string_refs(&right.source_refs, &[])
        && merge_string_refs(&left.evidence_refs, &[])
            == merge_string_refs(&right.evidence_refs, &[])
}

pub(super) fn memory_record_auto_accepted_event(memory: &MemoryRecord) -> BrainEvent {
    BrainEvent {
        event_id: format!("evt-{}-accepted", memory.memory_id),
        schema_version: BRAIN_EVENT_SCHEMA_VERSION,
        workspace_id: memory.workspace_id.clone(),
        scope: memory.scope,
        event_type: BrainEventKind::MemoryAccepted,
        operation_type: Some("new_memory".into()),
        actor: BrainActor {
            actor_type: BrainActorType::Agent,
            actor_id: "duckdocs-agent-ingest".into(),
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

pub(super) fn build_structured_extraction_artifacts(
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

pub(super) fn build_structured_extraction_artifact_for_source(
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
            extractor: "heuristic".into(),
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
                    "Heuristic extractor promoted concept node '{}' from source-backed evidence.",
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
                    "Heuristic extractor treated '{}' as a source-backed topic.",
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
                    "Heuristic extractor created this claim only because '{}' has direct source evidence.",
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
                    "Heuristic extractor kept this relation only because it has source evidence in {}.",
                    source_id
                ),
            })
        })
        .collect::<Vec<_>>();

    StructuredExtractionArtifact {
        artifact_id: format!("extraction-{source_id}"),
        workspace_id: workspace_id.into(),
        source_id,
        extractor: "heuristic".into(),
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
        provenance: "Structured extraction artifact generated from the current heuristic graph compiler fallback.".into(),
        created_at: generated_at,
    }
}

pub(super) fn evidence_for_source<'a>(
    evidence: &'a [EvidenceRef],
    source_id: &str,
) -> Vec<&'a EvidenceRef> {
    evidence
        .iter()
        .filter(|evidence| evidence.source_id.as_deref() == Some(source_id))
        .collect()
}

pub(super) fn source_refs_from_evidence(
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

pub(super) fn page_refs_from_evidence<'a>(
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

pub(super) fn brain_node_kind_for_graph_kind(kind: GraphNodeKind) -> BrainNodeKind {
    match kind {
        GraphNodeKind::Source | GraphNodeKind::Document => BrainNodeKind::Source,
        GraphNodeKind::Page => BrainNodeKind::Topic,
        GraphNodeKind::Concept => BrainNodeKind::Concept,
    }
}

pub(super) fn brain_relation_kind_for_edge(edge: &RelationEdgeSummary) -> BrainRelationKind {
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

pub(super) fn build_materialized_wiki_pages(
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

pub(super) fn write_materialized_brain_repo(
    root: &Path,
    snapshot: &BrainRepoSnapshot,
) -> Result<()> {
    let writer = BrainWorkspaceWriter::open(root.to_path_buf())?;
    let root = writer.root.as_path();
    ensure_materialized_brain_repo_dirs(root)?;
    let mut effective_snapshot = snapshot.clone();
    effective_snapshot.memories =
        merge_materialized_memory_records(snapshot.memories.clone(), read_memory_records(root)?);
    effective_snapshot.events = merge_preserved_brain_events(
        snapshot.events.clone(),
        &read_brain_events_jsonl(&root.join("events/brain_events.jsonl")).unwrap_or_default(),
    );
    apply_accepted_proposals_to_snapshot(root, &mut effective_snapshot)?;

    persist_materialized_graph_and_wiki_state(root, &effective_snapshot)?;
    write_json_pretty(
        &root.join("memory/records.json"),
        &effective_snapshot.memories,
    )?;
    write_structured_extraction_artifacts(root, &effective_snapshot.extractions)?;
    write_brain_events_jsonl(
        &root.join("events/brain_events.jsonl"),
        &effective_snapshot.events,
    )?;
    publish_latest_readable_graph_snapshot_marker(root, &effective_snapshot)?;

    Ok(())
}

pub(super) fn ensure_materialized_brain_repo_dirs(root: &Path) -> Result<()> {
    let wiki_root = root.join("wiki");
    for dir in [
        root.join("graph"),
        root.join("artifacts"),
        root.join("events"),
        root.join("memory"),
        root.join("state"),
        root.join("reviews/proposed-updates"),
        root.join("reviews/lint-reports"),
        wiki_root.join("sources"),
        wiki_root.join("entities"),
        wiki_root.join("topics"),
        wiki_root.join("claims"),
        wiki_root.join("questions"),
    ] {
        fs::create_dir_all(&dir).with_context(|| format!("failed creating {}", dir.display()))?;
    }

    fs::write(root.join("reviews/proposed-updates/.gitkeep"), "")
        .context("failed writing proposed updates placeholder")?;
    fs::write(root.join("reviews/lint-reports/.gitkeep"), "")
        .context("failed writing lint reports placeholder")?;
    fs::write(root.join("memory/.gitkeep"), "").context("failed writing memory placeholder")?;
    Ok(())
}

pub(super) fn publish_latest_readable_graph_snapshot_marker(
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

pub(super) fn read_latest_readable_graph_snapshot_marker(
    root: &Path,
) -> Result<Option<LatestReadableGraphSnapshotMarker>> {
    let path = root.join(LATEST_READABLE_SNAPSHOT_PATH);
    if !path.exists() {
        return Ok(None);
    }
    read_json_artifact(&path).map(Some)
}

pub(super) fn validate_latest_readable_materialized_files(
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

pub(super) fn latest_readable_materialized_file_refs(snapshot: &BrainRepoSnapshot) -> Vec<String> {
    let mut files = vec![
        "brain-manifest.json".to_string(),
        "graph/nodes.json".to_string(),
        "graph/edges.json".to_string(),
        "graph/claims.json".to_string(),
        "memory/records.json".to_string(),
        "events/brain_events.jsonl".to_string(),
    ];
    files.extend(snapshot.wiki_pages.iter().map(|page| page.path.clone()));
    files.sort();
    files.dedup();
    files
}

pub(super) fn write_structured_extraction_artifacts(
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

pub(super) fn persist_materialized_graph_and_wiki_state(
    root: &Path,
    snapshot: &BrainRepoSnapshot,
) -> Result<()> {
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

pub(super) fn merge_materialized_memory_records(
    generated: Vec<MemoryRecord>,
    existing: Vec<MemoryRecord>,
) -> Vec<MemoryRecord> {
    let mut merged = BTreeMap::<String, MemoryRecord>::new();
    for memory in generated.into_iter().chain(existing.into_iter()) {
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

pub(super) fn read_structured_extraction_artifacts(
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

pub(super) fn read_markdown_claim_candidates_for_row(
    row: &StoredSourceRow,
) -> Vec<MarkdownClaimCandidate> {
    Path::new(&row.manifest_path)
        .parent()
        .map(|artifact_root| artifact_root.join("claim-candidates.json"))
        .filter(|path| path.exists())
        .and_then(|path| read_json_artifact::<Vec<MarkdownClaimCandidate>>(&path).ok())
        .unwrap_or_default()
}

pub(super) fn merge_preserved_brain_events(
    mut materialized_events: Vec<BrainEvent>,
    existing_events: &[BrainEvent],
) -> Vec<BrainEvent> {
    let mut seen = materialized_events
        .iter()
        .map(|event| event.event_id.clone())
        .collect::<BTreeSet<_>>();
    for event in existing_events {
        if is_preserved_brain_event(event.event_type) && seen.insert(event.event_id.clone()) {
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

pub(super) fn is_preserved_brain_event(event_type: BrainEventKind) -> bool {
    matches!(
        event_type,
        BrainEventKind::NodeProposed
            | BrainEventKind::MemoryProposed
            | BrainEventKind::SourceIngestQueued
            | BrainEventKind::SourceCompiled
            | BrainEventKind::ClaimProposed
            | BrainEventKind::LinkProposed
            | BrainEventKind::ObservationAppended
            | BrainEventKind::SourceNoteProposed
            | BrainEventKind::WikiPageProposed
            | BrainEventKind::MemoryAccepted
            | BrainEventKind::ReviewCreated
            | BrainEventKind::ReviewResolved
            | BrainEventKind::BrainMaintenanceRun
    )
}

pub(super) fn materialized_wiki_page_body(page: &WikiPage, snapshot: &BrainRepoSnapshot) -> String {
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

pub(super) fn topic_source_references_markdown(
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

pub(super) fn source_refs_for_evidence_ids(
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

pub(super) fn topic_node_wiki_link(
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

pub(super) fn workspace_root_for_rows(
    rows: &[(StoredSourceRow, Option<KnowledgeProject>)],
) -> Option<PathBuf> {
    rows.iter()
        .find_map(|(row, _)| workspace_root_from_summary(&row.summary))
}

pub(super) fn workspace_root_from_summary(summary: &SourceSummary) -> Option<PathBuf> {
    workspace_root_from_path_segments(&summary.source_path, "sources", &summary.source_id).or_else(
        || {
            workspace_root_from_path_segments(
                &summary.markdown_path,
                "artifacts",
                &summary.source_id,
            )
        },
    )
}

pub(super) fn workspace_root_from_path_segments(
    path: &str,
    marker: &str,
    source_id: &str,
) -> Option<PathBuf> {
    let marker = format!("/{marker}/{source_id}");
    path.find(&marker)
        .filter(|index| *index > 0)
        .map(|index| PathBuf::from(&path[..index]))
}

pub(super) fn fallback_workspace_root(store_path: &Path, workspace_id: &str) -> PathBuf {
    store_path
        .parent()
        .map(|parent| parent.join(workspace_id))
        .unwrap_or_else(|| PathBuf::from(workspace_id))
}

pub(super) fn concept_identity_keys(detail: &GraphNodeDetail) -> Vec<String> {
    let mut keys = Vec::new();
    let canonical_key = normalize_key(&detail.canonical_name);
    if !canonical_key.is_empty() {
        keys.push(canonical_key);
    }
    for alias in &detail.aliases {
        let key = normalize_key(alias);
        if !key.is_empty() && !keys.contains(&key) {
            keys.push(key);
        }
    }
    keys
}

pub(super) fn source_node_position(index: usize, total: usize) -> GraphNodePosition {
    if total <= 1 {
        return GraphNodePosition { x: 50.0, y: 12.0 };
    }
    let x = 14.0 + (72.0 / (total.saturating_sub(1) as f32)) * (index as f32);
    GraphNodePosition { x, y: 12.0 }
}

pub(super) fn source_node_id(source_id: &str) -> String {
    format!("source:{source_id}")
}

pub(super) fn is_source_like_node_kind(kind: GraphNodeKind) -> bool {
    matches!(kind, GraphNodeKind::Source | GraphNodeKind::Document)
}

pub(super) fn source_label_from_manifest(manifest: &SourceArtifactManifest) -> String {
    Path::new(&manifest.original_path)
        .file_name()
        .or_else(|| Path::new(&manifest.source_path).file_name())
        .map(|value| value.to_string_lossy().into_owned())
        .unwrap_or_else(|| manifest.output_name.clone())
}

pub(super) fn source_backing_from_manifest(manifest: &SourceArtifactManifest) -> SourceBacking {
    SourceBacking {
        workspace_id: manifest.workspace_id.clone(),
        source_id: manifest.source_id.clone(),
        original_path: manifest.original_path.clone(),
        source_path: manifest.source_path.clone(),
        markdown_path: manifest.markdown_path.clone(),
        format: document_format_slug(&manifest.format).into(),
        status: ingest_status_slug(&manifest.status).into(),
        page_count: manifest.pages.len(),
        success_count: manifest
            .pages
            .iter()
            .filter(|page| page.error_message.is_none())
            .count(),
        failed_count: manifest
            .pages
            .iter()
            .filter(|page| page.error_message.is_some())
            .count(),
        description: manifest.description.clone(),
        user_context: manifest.user_context.clone(),
        ingest_instruction: manifest.ingest_instruction.clone(),
        updated_at: manifest.updated_at,
        manifest_path: Some(manifest.manifest_path.clone()),
    }
}

pub(super) fn workspace_root_from_manifest(manifest: &SourceArtifactManifest) -> PathBuf {
    Path::new(&manifest.artifact_root)
        .parent()
        .and_then(Path::parent)
        .map(Path::to_path_buf)
        .unwrap_or_else(|| {
            Path::new(&manifest.manifest_path)
                .parent()
                .and_then(Path::parent)
                .and_then(Path::parent)
                .map(Path::to_path_buf)
                .unwrap_or_else(|| PathBuf::from(&manifest.artifact_root))
        })
}

pub(super) fn extract_markdown_node_candidates(
    markdown: &str,
    source_path: &str,
) -> Vec<MarkdownNodeCandidate> {
    let mut candidates = Vec::<MarkdownNodeCandidate>::new();
    let mut seen = BTreeSet::<String>::new();
    let mut in_frontmatter = false;
    let mut frontmatter_closed = false;

    for (line_index, raw_line) in markdown.lines().enumerate() {
        let line_start = line_index + 1;
        let trimmed = raw_line.trim();
        if line_index == 0 && trimmed == "---" {
            in_frontmatter = true;
            continue;
        }
        if in_frontmatter {
            if trimmed == "---" {
                in_frontmatter = false;
                frontmatter_closed = true;
                continue;
            }
            if let Some(title) = frontmatter_title_candidate(trimmed) {
                push_markdown_node_candidate(
                    &mut candidates,
                    &mut seen,
                    title,
                    source_path,
                    line_start,
                    trimmed,
                    0.92,
                    "frontmatter title declares a stable node label",
                );
            }
            continue;
        }

        if !frontmatter_closed && trimmed == "---" {
            continue;
        }
        if let Some(heading) = markdown_heading_candidate(trimmed) {
            push_markdown_node_candidate(
                &mut candidates,
                &mut seen,
                heading,
                source_path,
                line_start,
                trimmed,
                0.88,
                "markdown heading declares a stable node label",
            );
            continue;
        }
        let cleaned = clean_candidate_line(trimmed);
        if let Some(label) = derive_concept_label(&cleaned) {
            push_markdown_node_candidate(
                &mut candidates,
                &mut seen,
                label,
                source_path,
                line_start,
                trimmed,
                0.68,
                "markdown body line produced a stable candidate label",
            );
        }
        if candidates.len() >= 24 {
            break;
        }
    }

    candidates
}

pub(super) fn extract_markdown_node_candidates_for_workspace(
    markdown: &str,
    source_path: &str,
    workspace_root: &Path,
) -> Result<Vec<MarkdownNodeCandidate>> {
    let candidates = extract_markdown_node_candidates(markdown, source_path);
    let existing_nodes = read_existing_graph_nodes(workspace_root)?;
    Ok(match_markdown_node_candidates(candidates, &existing_nodes))
}

pub(super) fn extract_markdown_relationship_evidence(
    markdown: &str,
    source_path: &str,
    source_id: Option<&str>,
    node_candidates: &[MarkdownNodeCandidate],
) -> Vec<MarkdownRelationshipEvidence> {
    let mut evidence = Vec::new();
    let mut seen = BTreeSet::<String>::new();
    let mut in_frontmatter = false;

    for (line_index, raw_line) in markdown.lines().enumerate() {
        let line_start = line_index + 1;
        let trimmed = raw_line.trim();
        if line_index == 0 && trimmed == "---" {
            in_frontmatter = true;
            continue;
        }
        if in_frontmatter {
            if trimmed == "---" {
                in_frontmatter = false;
            }
            continue;
        }
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }

        let relation_kind = infer_markdown_relation_kind(trimmed);
        if relation_kind.is_none() && !line_has_explicit_link_signal(trimmed) {
            continue;
        }

        let mentions = relationship_mentions_in_line(trimmed, node_candidates);
        if mentions.len() < 2 {
            continue;
        }

        for left_index in 0..mentions.len() {
            for right_index in (left_index + 1)..mentions.len() {
                let left = &mentions[left_index];
                let right = &mentions[right_index];
                let key = format!(
                    "{}:{}:{}",
                    line_start,
                    normalize_key(&left.label),
                    normalize_key(&right.label)
                );
                if !seen.insert(key.clone()) {
                    continue;
                }
                let relation_kind = relation_kind.unwrap_or(BrainRelationKind::RelatedTo);
                let candidate_id = format!("edge-candidate-{key}");
                evidence.push(MarkdownRelationshipEvidence {
                    candidate_id,
                    evidence_id: format!("ev-relation-{key}"),
                    source_path: source_path.to_string(),
                    source_id: source_id.map(ToString::to_string),
                    source_refs: source_id
                        .map(|source_id| vec![source_id.to_string()])
                        .unwrap_or_default(),
                    line_start,
                    snippet: excerpt(trimmed, 220),
                    source_label: left.label.clone(),
                    target_label: right.label.clone(),
                    relation_kind,
                    relation_label: markdown_relation_label(relation_kind),
                    confidence: if relation_kind == BrainRelationKind::RelatedTo {
                        0.74
                    } else {
                        0.82
                    },
                    reason: relationship_reason(trimmed, Some(relation_kind)),
                    matched_source_node_id: left.matched_node_id.clone(),
                    matched_target_node_id: right.matched_node_id.clone(),
                    resolved_source_node_id: Some(left.resolved_node_id.clone()),
                    resolved_target_node_id: Some(right.resolved_node_id.clone()),
                    endpoint_resolution: format!(
                        "{} -> {}; {} -> {}",
                        left.label,
                        left.endpoint_resolution,
                        right.label,
                        right.endpoint_resolution
                    ),
                });
                if evidence.len() >= 32 {
                    return evidence;
                }
            }
        }
    }

    evidence
}

pub(super) fn extract_markdown_claim_candidates(
    markdown: &str,
    source_path: &str,
    source_id: Option<&str>,
    node_candidates: &[MarkdownNodeCandidate],
) -> Vec<MarkdownClaimCandidate> {
    let mut candidates = Vec::new();
    let mut seen = BTreeSet::<String>::new();
    let mut in_frontmatter = false;

    for (line_index, raw_line) in markdown.lines().enumerate() {
        let line_start = line_index + 1;
        let trimmed = raw_line.trim();
        if line_index == 0 && trimmed == "---" {
            in_frontmatter = true;
            continue;
        }
        if in_frontmatter {
            if trimmed == "---" {
                in_frontmatter = false;
            }
            continue;
        }
        if trimmed.is_empty() || trimmed.starts_with('#') || trimmed.starts_with("![") {
            continue;
        }

        let Some(statement) = normalize_claim_statement(trimmed) else {
            continue;
        };
        let claim_key = bounded_artifact_key(&statement, 80);
        let evidence_scope_key = source_id
            .map(|source_id| bounded_artifact_key(source_id, 48))
            .unwrap_or_else(|| bounded_artifact_key(source_path, 48));
        if claim_key.is_empty() || !seen.insert(claim_key.clone()) {
            continue;
        }
        let char_start = raw_line
            .find(statement.as_str())
            .or_else(|| raw_line.find(trimmed))
            .unwrap_or(0);
        let char_end = char_start + statement.len();
        let evidence_snippet = excerpt(&statement, 220);
        let mentions = relationship_mentions_in_line(&statement, node_candidates);
        let subject_labels = mentions
            .iter()
            .take(4)
            .map(|mention| mention.label.clone())
            .collect::<Vec<_>>();
        let mut subject_refs = mentions
            .iter()
            .take(4)
            .map(|mention| mention.resolved_node_id.clone())
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect::<Vec<_>>();
        if subject_refs.is_empty() {
            if let Some(label) = derive_concept_label(&statement) {
                subject_refs.push(format!("concept-{}", normalize_key(&label)));
            }
        }

        let confidence = claim_candidate_confidence(&statement, !subject_refs.is_empty());
        let classification = classify_markdown_claim_statement(&statement);
        candidates.push(MarkdownClaimCandidate {
            candidate_id: format!("claim-candidate-{evidence_scope_key}-{line_start}-{claim_key}"),
            evidence_id: format!("ev-claim-{evidence_scope_key}-{line_start}-{claim_key}"),
            statement: statement.clone(),
            classification,
            durable: true,
            memory_candidate: markdown_claim_should_be_memory_candidate(&statement, classification),
            source_path: source_path.to_string(),
            source_id: source_id.map(ToString::to_string),
            source_refs: source_id
                .map(|source_id| vec![source_id.to_string()])
                .unwrap_or_default(),
            line_start,
            line_end: line_start,
            char_start,
            char_end,
            evidence_span: MarkdownEvidenceSpan {
                source_path: source_path.to_string(),
                source_id: source_id.map(ToString::to_string),
                line_start,
                line_end: line_start,
                char_start,
                char_end,
                snippet: evidence_snippet.clone(),
            },
            evidence_snippet,
            subject_labels,
            subject_refs,
            confidence,
            reason: claim_candidate_reason(&statement),
        });
        if candidates.len() >= 32 {
            break;
        }
    }

    candidates
}

pub(super) fn extract_markdown_signals(
    markdown: &str,
    source_path: &str,
    source_id: Option<&str>,
    node_candidates: &[MarkdownNodeCandidate],
) -> MarkdownSignalArtifact {
    let mut title = None;
    let mut headings = Vec::<MarkdownHeadingSignal>::new();
    let mut links = Vec::<MarkdownLinkSignal>::new();
    let mut keyword_counts = BTreeMap::<String, usize>::new();
    let mut in_frontmatter = false;

    for (line_index, raw_line) in markdown.lines().enumerate() {
        let line_start = line_index + 1;
        let trimmed = raw_line.trim();
        if line_index == 0 && trimmed == "---" {
            in_frontmatter = true;
            continue;
        }
        if in_frontmatter {
            if trimmed == "---" {
                in_frontmatter = false;
                continue;
            }
            if title.is_none() {
                title = frontmatter_title_candidate(trimmed);
            }
            continue;
        }
        if let Some(heading) = markdown_heading_signal(trimmed, line_start) {
            if title.is_none() && heading.level == 1 {
                title = Some(heading.text.clone());
            }
            headings.push(heading);
        }
        links.extend(markdown_link_signals(trimmed, line_start));
        for term in markdown_signal_terms(trimmed) {
            *keyword_counts.entry(term).or_default() += 1;
        }
    }

    let mut keywords = keyword_counts
        .into_iter()
        .filter(|(_, count)| *count >= 2)
        .map(|(term, count)| MarkdownKeywordSignal { term, count })
        .collect::<Vec<_>>();
    keywords.sort_by(|left, right| {
        right
            .count
            .cmp(&left.count)
            .then_with(|| left.term.cmp(&right.term))
    });
    keywords.truncate(16);

    MarkdownSignalArtifact {
        source_path: source_path.to_string(),
        source_id: source_id.map(ToString::to_string),
        source_refs: source_id
            .map(|source_id| vec![source_id.to_string()])
            .unwrap_or_default(),
        title,
        headings,
        links,
        entities: node_candidates
            .iter()
            .map(|candidate| MarkdownEntitySignal {
                label: candidate.label.clone(),
                line_start: candidate.line_start,
                confidence: candidate.confidence,
                reason: candidate.reason.clone(),
                matched_node_id: candidate.matched_node_id.clone(),
                matched_node_label: candidate.matched_node_label.clone(),
                match_score: candidate.match_score,
            })
            .collect(),
        keywords,
        related_pages: Vec::new(),
    }
}

pub(super) fn rank_related_wiki_pages_for_signals(
    workspace_root: &Path,
    workspace_id: &str,
    signals: &MarkdownSignalArtifact,
) -> Result<Vec<MarkdownRelatedPageSignal>> {
    if !workspace_root.join("brain-manifest.json").exists() {
        return Ok(Vec::new());
    }

    let snapshot = read_materialized_brain_snapshot(workspace_root, workspace_id)?;
    let weighted_terms = weighted_markdown_signal_terms(signals);
    if weighted_terms.is_empty() {
        return Ok(Vec::new());
    }

    let mut related_pages = snapshot
        .wiki_pages
        .iter()
        .filter_map(|page| {
            let metadata_text = wiki_page_metadata_text(page);
            let body = fs::read_to_string(workspace_root.join(&page.path))
                .unwrap_or_else(|_| materialized_wiki_page_body(page, &snapshot));
            let metadata_frequencies = search_token_frequencies(&metadata_text);
            let content_frequencies = search_token_frequencies(&body);
            let mut metadata_score = 0usize;
            let mut content_score = 0usize;
            let mut matched_terms = Vec::<String>::new();

            for (term, weight) in &weighted_terms {
                let metadata_count = metadata_frequencies.get(term).copied().unwrap_or(0);
                let content_count = content_frequencies.get(term).copied().unwrap_or(0);
                if metadata_count == 0 && content_count == 0 {
                    continue;
                }
                metadata_score += metadata_count.saturating_mul(*weight).saturating_mul(8);
                content_score += content_count.saturating_mul(*weight).saturating_mul(2);
                matched_terms.push(term.clone());
            }

            let score = metadata_score + content_score + matched_terms.len().saturating_mul(3);
            if score == 0 {
                return None;
            }
            let reason = match (metadata_score > 0, content_score > 0) {
                (true, true) => {
                    "ranked by overlap with existing wiki page metadata and content".into()
                }
                (true, false) => "ranked by overlap with existing wiki page metadata".into(),
                (false, true) => "ranked by overlap with existing wiki page content".into(),
                (false, false) => "ranked by signal overlap".into(),
            };
            Some(MarkdownRelatedPageSignal {
                page_id: page.page_id.clone(),
                path: page.path.clone(),
                title: page.title.clone(),
                score,
                matched_terms,
                reason,
            })
        })
        .collect::<Vec<_>>();

    related_pages.sort_by(|left, right| {
        right
            .score
            .cmp(&left.score)
            .then_with(|| left.path.cmp(&right.path))
    });
    related_pages.truncate(8);
    Ok(related_pages)
}

pub(super) fn weighted_markdown_signal_terms(
    signals: &MarkdownSignalArtifact,
) -> BTreeMap<String, usize> {
    let mut terms = BTreeMap::<String, usize>::new();
    if let Some(title) = &signals.title {
        add_weighted_terms(&mut terms, title, 8);
    }
    for heading in &signals.headings {
        let weight = if heading.level == 1 { 7 } else { 5 };
        add_weighted_terms(&mut terms, &heading.text, weight);
    }
    for entity in &signals.entities {
        add_weighted_terms(&mut terms, &entity.label, 6);
        if let Some(label) = &entity.matched_node_label {
            add_weighted_terms(&mut terms, label, 6);
        }
    }
    for link in &signals.links {
        add_weighted_terms(&mut terms, &link.label, 4);
        add_weighted_terms(&mut terms, &link.target, 3);
    }
    for keyword in &signals.keywords {
        *terms.entry(keyword.term.clone()).or_default() += keyword.count.min(4);
    }
    terms
}

pub(super) fn add_weighted_terms(terms: &mut BTreeMap<String, usize>, text: &str, weight: usize) {
    for term in markdown_signal_terms(text) {
        *terms.entry(term).or_default() += weight;
    }
}

pub(super) fn wiki_page_metadata_text(page: &WikiPage) -> String {
    [
        page.page_id.as_str(),
        page.path.as_str(),
        page.title.as_str(),
        &page.node_refs.join(" "),
        &page.source_refs.join(" "),
        &page.evidence_refs.join(" "),
    ]
    .join(" ")
}

pub(super) fn markdown_heading_signal(
    line: &str,
    line_start: usize,
) -> Option<MarkdownHeadingSignal> {
    if !line.starts_with('#') {
        return None;
    }
    let level = line.chars().take_while(|char| *char == '#').count();
    if level == 0 || level > 6 {
        return None;
    }
    let text = markdown_scalar_label(line[level..].trim())?;
    Some(MarkdownHeadingSignal {
        text,
        level,
        line_start,
    })
}

pub(super) fn markdown_link_signals(line: &str, line_start: usize) -> Vec<MarkdownLinkSignal> {
    let mut links = Vec::new();
    let mut rest = line;
    while let Some(start) = rest.find("[[") {
        let after_start = &rest[start + 2..];
        let Some(end) = after_start.find("]]") else {
            break;
        };
        let target = after_start[..end].trim();
        if !target.is_empty() {
            let label = target
                .split('|')
                .next_back()
                .unwrap_or(target)
                .trim()
                .to_string();
            links.push(MarkdownLinkSignal {
                label,
                target: target.to_string(),
                kind: "wiki".into(),
                line_start,
            });
        }
        rest = &after_start[end + 2..];
    }

    let mut rest = line;
    while let Some(label_start) = rest.find('[') {
        if rest[..label_start].ends_with('!') {
            rest = &rest[label_start + 1..];
            continue;
        }
        if rest[label_start..].starts_with("[[") {
            let after_wiki_start = &rest[label_start + 2..];
            rest = match after_wiki_start.find("]]") {
                Some(wiki_end) => &after_wiki_start[wiki_end + 2..],
                None => &rest[label_start + 2..],
            };
            continue;
        }
        let after_label_start = &rest[label_start + 1..];
        let Some(label_end) = after_label_start.find("](") else {
            break;
        };
        let label = after_label_start[..label_end].trim();
        let after_target_start = &after_label_start[label_end + 2..];
        let Some(target_end) = after_target_start.find(')') else {
            break;
        };
        let target = after_target_start[..target_end].trim();
        if !label.is_empty() && !target.is_empty() {
            links.push(MarkdownLinkSignal {
                label: label.to_string(),
                target: target.to_string(),
                kind: "markdown".into(),
                line_start,
            });
        }
        rest = &after_target_start[target_end + 1..];
    }

    links
}

pub(super) fn markdown_signal_terms(line: &str) -> Vec<String> {
    let line = strip_inline_markdown_targets(line);
    search_terms(&line)
        .into_iter()
        .filter(|term| !is_markdown_signal_stopword(term))
        .collect()
}

pub(super) fn strip_inline_markdown_targets(line: &str) -> String {
    line.replace("[[", " ")
        .replace("]]", " ")
        .replace("](", " ")
        .replace(['[', ']', '(', ')', '#', '`', '*'], " ")
}

pub(super) fn is_markdown_signal_stopword(term: &str) -> bool {
    matches!(
        term,
        "and"
            | "are"
            | "but"
            | "for"
            | "from"
            | "into"
            | "the"
            | "this"
            | "that"
            | "with"
            | "without"
            | "source"
            | "evidence"
            | "remain"
            | "keep"
            | "keeps"
            | "durable"
    )
}

pub(super) fn classify_markdown_claim_statement(statement: &str) -> MarkdownClaimClassification {
    let lower = format!(" {} ", statement.to_ascii_lowercase());
    if [
        " decision ",
        " decided ",
        " chose ",
        " chosen ",
        " approved ",
        " accepted ",
        " source of truth ",
        " must ",
        " should ",
        " will ",
        " no human approval ",
        " records approved ",
    ]
    .iter()
    .any(|marker| lower.contains(marker))
    {
        MarkdownClaimClassification::Decision
    } else {
        MarkdownClaimClassification::DurableFact
    }
}

pub(super) fn markdown_claim_should_be_memory_candidate(
    statement: &str,
    classification: MarkdownClaimClassification,
) -> bool {
    if classification == MarkdownClaimClassification::Decision {
        return true;
    }
    let lower = statement.to_ascii_lowercase();
    [
        "remember",
        "retain",
        "persistent",
        "durable memory",
        "agent memory",
        "memory candidate",
    ]
    .iter()
    .any(|marker| lower.contains(marker))
}

pub(super) fn normalize_claim_statement(line: &str) -> Option<String> {
    let statement = clean_candidate_line(line)
        .trim_start_matches('>')
        .trim_start_matches(|char: char| char == '-' || char == '*')
        .trim()
        .trim_end_matches(';')
        .trim()
        .to_string();
    if statement.len() < 18 || statement.split_whitespace().count() < 4 {
        return None;
    }
    if !line_looks_like_claim(&statement) {
        return None;
    }
    Some(statement)
}

pub(super) fn line_looks_like_claim(statement: &str) -> bool {
    let lower = format!(" {} ", statement.to_ascii_lowercase());
    [
        " is ",
        " are ",
        " was ",
        " were ",
        " has ",
        " have ",
        " had ",
        " can ",
        " should ",
        " must ",
        " will ",
        " remains ",
        " keeps ",
        " records ",
        " stores ",
        " supports ",
        " depends on ",
        " relies on ",
        " requires ",
        " enables ",
        " blocks ",
        " contradicts ",
        " supersedes ",
    ]
    .iter()
    .any(|marker| lower.contains(marker))
}

pub(super) fn claim_candidate_confidence(statement: &str, has_subject: bool) -> f32 {
    let lower = statement.to_ascii_lowercase();
    let explicit = lower.contains(" is ")
        || lower.contains(" are ")
        || lower.contains(" must ")
        || lower.contains(" should ")
        || lower.contains(" depends on ")
        || lower.contains(" supports ");
    match (explicit, has_subject) {
        (true, true) => 0.84,
        (true, false) => 0.76,
        (false, true) => 0.72,
        (false, false) => 0.64,
    }
}

pub(super) fn claim_candidate_reason(statement: &str) -> String {
    match classify_markdown_claim_statement(statement) {
        MarkdownClaimClassification::Decision => {
            return "the line states a durable decision or operating rule with source evidence"
                .into();
        }
        MarkdownClaimClassification::DurableFact => {}
    }
    if infer_markdown_relation_kind(statement).is_some() {
        return "the line states an explicit relation that can be audited as a claim".into();
    }
    "the line contains a factual modal or copular assertion with source evidence".into()
}

pub(super) fn bounded_artifact_key(value: &str, max_chars: usize) -> String {
    normalize_key(value)
        .chars()
        .take(max_chars)
        .collect::<String>()
        .trim_matches('-')
        .to_string()
}

#[derive(Debug, Clone)]
pub(super) struct RelationshipMention {
    pub(super) label: String,
    pub(super) position: usize,
    pub(super) matched_node_id: Option<String>,
    pub(super) resolved_node_id: String,
    pub(super) endpoint_resolution: String,
}

pub(super) fn relationship_mentions_in_line(
    line: &str,
    node_candidates: &[MarkdownNodeCandidate],
) -> Vec<RelationshipMention> {
    let lower_line = line.to_ascii_lowercase();
    let mut mentions = Vec::new();
    let mut seen = BTreeSet::<String>::new();
    for candidate in node_candidates {
        let labels = candidate
            .matched_node_label
            .iter()
            .chain(std::iter::once(&candidate.label));
        for label in labels {
            let needle = label.to_ascii_lowercase();
            if needle.len() < 4 {
                continue;
            }
            let Some(position) = lower_line.find(&needle) else {
                continue;
            };
            let key = candidate
                .matched_node_id
                .clone()
                .unwrap_or_else(|| normalize_key(label));
            if !seen.insert(key) {
                continue;
            }
            let resolved_node_id = candidate
                .matched_node_id
                .clone()
                .unwrap_or_else(|| format!("concept-{}", normalize_key(&candidate.label)));
            let endpoint_resolution = if candidate.matched_node_id.is_some() {
                "existing_node".into()
            } else {
                "proposed_node".into()
            };
            mentions.push(RelationshipMention {
                label: label.clone(),
                position,
                matched_node_id: candidate.matched_node_id.clone(),
                resolved_node_id,
                endpoint_resolution,
            });
        }
    }
    mentions.sort_by(|left, right| {
        left.position
            .cmp(&right.position)
            .then_with(|| left.label.cmp(&right.label))
    });
    mentions
}

pub(super) fn infer_markdown_relation_kind(line: &str) -> Option<BrainRelationKind> {
    let lower = line.to_ascii_lowercase();
    if lower.contains(" depends on ")
        || lower.contains(" relies on ")
        || lower.contains(" requires ")
        || lower.contains(" blocked by ")
    {
        return Some(BrainRelationKind::DependsOn);
    }
    if lower.contains(" supports ")
        || lower.contains(" enables ")
        || lower.contains(" grounds ")
        || lower.contains(" backs ")
        || lower.contains(" cites ")
    {
        return Some(BrainRelationKind::Supports);
    }
    if lower.contains(" contradicts ") || lower.contains(" conflicts with ") {
        return Some(BrainRelationKind::Contradicts);
    }
    if lower.contains(" supersedes ") || lower.contains(" replaces ") {
        return Some(BrainRelationKind::Supersedes);
    }
    if lower.contains(" same as ") || lower.contains(" alias of ") {
        return Some(BrainRelationKind::SameAs);
    }
    if line.contains("->") || line.contains("<->") || lower.contains(" links ") {
        return Some(BrainRelationKind::RelatedTo);
    }
    None
}

pub(super) fn markdown_relation_label(kind: BrainRelationKind) -> String {
    match kind {
        BrainRelationKind::Supports => "Supports".into(),
        BrainRelationKind::Contradicts => "Contradicts".into(),
        BrainRelationKind::Supersedes => "Supersedes".into(),
        BrainRelationKind::SameAs => "Same as".into(),
        BrainRelationKind::DependsOn => "Depends on".into(),
        _ => "Related in source".into(),
    }
}

pub(super) fn line_has_explicit_link_signal(line: &str) -> bool {
    line.contains("[[") || line.contains("](") || line.contains("->") || line.contains("<->")
}

pub(super) fn relationship_reason(line: &str, relation_kind: Option<BrainRelationKind>) -> String {
    if let Some(kind) = relation_kind {
        return format!("the line contains an explicit {:?} relationship cue", kind);
    }
    if line.contains("[[") {
        return "the line contains wiki-link syntax connecting mentioned nodes".into();
    }
    if line.contains("](") {
        return "the line contains markdown-link syntax connecting mentioned nodes".into();
    }
    "the line contains an explicit link signal connecting mentioned nodes".into()
}

pub(super) fn read_existing_graph_nodes(workspace_root: &Path) -> Result<Vec<BrainNodeRecord>> {
    read_optional_json_artifact(&workspace_root.join("graph/nodes.json"))
}

pub(super) fn read_existing_graph_relations(
    workspace_root: &Path,
) -> Result<Vec<BrainRelationRecord>> {
    read_optional_json_artifact(&workspace_root.join("graph/edges.json"))
}

pub(super) fn match_markdown_node_candidates(
    candidates: Vec<MarkdownNodeCandidate>,
    existing_nodes: &[BrainNodeRecord],
) -> Vec<MarkdownNodeCandidate> {
    candidates
        .into_iter()
        .map(|mut candidate| {
            if let Some(node_match) = best_existing_node_match(&candidate, existing_nodes) {
                candidate.matched_node_id = Some(node_match.node_id);
                candidate.matched_node_label = Some(node_match.label);
                candidate.match_score = Some(node_match.score);
                candidate.match_reason = Some(node_match.reason);
            }
            candidate
        })
        .collect()
}

#[derive(Debug, Clone, PartialEq)]
pub(super) struct ExistingNodeMatch {
    pub(super) node_id: String,
    pub(super) label: String,
    pub(super) score: f32,
    pub(super) reason: String,
}

pub(super) fn best_existing_node_match(
    candidate: &MarkdownNodeCandidate,
    existing_nodes: &[BrainNodeRecord],
) -> Option<ExistingNodeMatch> {
    existing_nodes
        .iter()
        .filter(|node| matches!(node.kind, BrainNodeKind::Concept | BrainNodeKind::Topic))
        .filter_map(|node| score_existing_node_match(candidate, node))
        .max_by(|left, right| {
            left.score
                .partial_cmp(&right.score)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| right.node_id.cmp(&left.node_id))
        })
        .filter(|node_match| node_match.score >= 0.72)
}

pub(super) fn score_existing_node_match(
    candidate: &MarkdownNodeCandidate,
    node: &BrainNodeRecord,
) -> Option<ExistingNodeMatch> {
    let candidate_key = normalize_key(&candidate.label);
    if candidate_key.is_empty() {
        return None;
    }

    let mut identity_labels = vec![node.label.as_str()];
    identity_labels.extend(node.aliases.iter().map(String::as_str));
    for label in &identity_labels {
        if normalize_key(label) == candidate_key {
            return Some(ExistingNodeMatch {
                node_id: node.node_id.clone(),
                label: node.label.clone(),
                score: 1.0,
                reason: "candidate label exactly matched an existing graph node label or alias"
                    .into(),
            });
        }
    }

    let candidate_terms = candidate_label_terms(&candidate.label);
    if candidate_terms.len() < 2 {
        return None;
    }
    let node_terms = identity_labels
        .iter()
        .flat_map(|label| candidate_label_terms(label))
        .collect::<BTreeSet<_>>();
    if node_terms.is_empty() {
        return None;
    }
    let intersection_count = candidate_terms.intersection(&node_terms).count();
    if intersection_count == 0 {
        return None;
    }
    let union_count = candidate_terms.union(&node_terms).count();
    let score = if candidate_terms.is_subset(&node_terms) || node_terms.is_subset(&candidate_terms)
    {
        0.86
    } else {
        intersection_count as f32 / union_count as f32
    };
    (score >= 0.72).then(|| ExistingNodeMatch {
        node_id: node.node_id.clone(),
        label: node.label.clone(),
        score,
        reason: "candidate label strongly overlapped an existing graph node label or alias".into(),
    })
}

pub(super) fn candidate_label_terms(label: &str) -> BTreeSet<String> {
    label
        .split(|char: char| !char.is_ascii_alphanumeric())
        .filter_map(normalize_search_token)
        .collect()
}

pub(super) fn push_markdown_node_candidate(
    candidates: &mut Vec<MarkdownNodeCandidate>,
    seen: &mut BTreeSet<String>,
    label: String,
    source_path: &str,
    line_start: usize,
    evidence: &str,
    confidence: f32,
    reason: &str,
) {
    let label = normalize_candidate_label(&label);
    let key = normalize_key(&label);
    if key.is_empty() || !seen.insert(key.clone()) {
        return;
    }
    candidates.push(MarkdownNodeCandidate {
        candidate_id: format!("candidate-{key}"),
        label,
        kind: BrainNodeKind::Concept,
        source_path: source_path.to_string(),
        line_start,
        evidence_snippet: excerpt(evidence, 180),
        confidence,
        reason: reason.into(),
        matched_node_id: None,
        matched_node_label: None,
        match_score: None,
        match_reason: None,
    });
}

pub(super) fn frontmatter_title_candidate(line: &str) -> Option<String> {
    let value = line.strip_prefix("title:")?.trim();
    markdown_scalar_label(value)
}

pub(super) fn markdown_heading_candidate(line: &str) -> Option<String> {
    if !line.starts_with('#') {
        return None;
    }
    let hash_count = line.chars().take_while(|char| *char == '#').count();
    if hash_count == 0 || hash_count > 4 {
        return None;
    }
    let value = line[hash_count..].trim();
    if value.to_ascii_lowercase().starts_with("page ") {
        return None;
    }
    markdown_scalar_label(value)
}

pub(super) fn markdown_scalar_label(value: &str) -> Option<String> {
    let label = normalize_candidate_label(value.trim_matches(['"', '\'']));
    let word_count = label.split_whitespace().count();
    (label.len() >= 4 && word_count <= 10).then_some(label)
}

pub(super) fn normalize_candidate_label(value: &str) -> String {
    value
        .trim()
        .trim_matches(|char: char| !char.is_alphanumeric())
        .replace('`', "")
        .replace('*', "")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

pub(super) fn page_section_for_candidate<'a>(
    sections: &'a [PageSection],
    candidate: &MarkdownNodeCandidate,
) -> Option<&'a PageSection> {
    sections.iter().find(|section| {
        section.content.contains(&candidate.evidence_snippet)
            || section.content.contains(&candidate.label)
    })
}

pub(super) fn page_section_for_line(
    sections: &[PageSection],
    _line_start: usize,
) -> Option<&PageSection> {
    sections.first()
}

pub(super) fn build_extraction_artifact(
    page_sections: &[PageSection],
    markdown: &str,
    source_path: &str,
    source_id: Option<&str>,
    node_candidates: &[MarkdownNodeCandidate],
    claim_candidates: &[MarkdownClaimCandidate],
) -> ExtractionArtifact {
    let mut concepts = BTreeMap::<String, ExtractedConcept>::new();
    let mut claims = Vec::new();
    let mut evidence_refs = BTreeMap::new();
    let mut concept_ids_by_page = Vec::<(String, Vec<String>, Vec<String>)>::new();

    for candidate in node_candidates.iter().take(20) {
        let matched_label = candidate
            .matched_node_label
            .as_deref()
            .unwrap_or(&candidate.label);
        let key = candidate
            .matched_node_id
            .as_deref()
            .and_then(|node_id| node_id.strip_prefix("concept-"))
            .map(ToString::to_string)
            .unwrap_or_else(|| normalize_key(matched_label));
        if key.is_empty() {
            continue;
        }
        let concept_id = candidate
            .matched_node_id
            .clone()
            .unwrap_or_else(|| format!("concept-{key}"));
        let evidence_id = format!("ev-candidate-{key}");
        let section =
            page_section_for_candidate(page_sections, candidate).or_else(|| page_sections.first());
        let page_index = section.map(|section| section.page_index).unwrap_or(0);
        let page_label = section
            .map(|section| section.page_label.clone())
            .unwrap_or_else(|| "Imported text".into());
        let markdown_path = section.and_then(|section| section.markdown_path.clone());
        let image_path = section.and_then(|section| section.image_path.clone());
        let concept = concepts
            .entry(key.clone())
            .or_insert_with(|| ExtractedConcept {
                id: concept_id.clone(),
                label: matched_label.to_string(),
                aliases: BTreeSet::new(),
                evidence_ids: Vec::new(),
                page_labels: BTreeSet::new(),
            });
        if concept.label != candidate.label {
            concept.aliases.insert(candidate.label.clone());
        }
        if let Some(matched_label) = &candidate.matched_node_label {
            if concept.label != *matched_label {
                concept.aliases.insert(matched_label.clone());
            }
        }
        concept.page_labels.insert(page_label.clone());
        if !concept.evidence_ids.iter().any(|id| id == &evidence_id) {
            concept.evidence_ids.push(evidence_id.clone());
        }
        evidence_refs
            .entry(evidence_id.clone())
            .or_insert_with(|| ExtractionEvidenceRef {
                id: evidence_id.clone(),
                page_index,
                page_label: page_label.clone(),
                snippet: candidate.evidence_snippet.clone(),
                source_path: source_path.to_string(),
                source_id: source_id.map(ToString::to_string),
                markdown_path,
                image_path,
                provenance: format!(
                    "Node candidate '{}' was extracted from the markdown source at line {} because {}{}.",
                    candidate.label,
                    candidate.line_start,
                    candidate.reason,
                    candidate
                        .matched_node_id
                        .as_ref()
                        .map(|node_id| format!(" It matched existing graph node {node_id}"))
                        .unwrap_or_default()
                ),
            });
        claims.push(ExtractedClaim {
            id: format!("claim-candidate-{}", key),
            text: matched_label.to_string(),
            subject_concept_id: concept_id.clone(),
            evidence_id: evidence_id.clone(),
        });
        concept_ids_by_page.push((page_label, vec![concept_id], vec![evidence_id]));
    }

    for section in page_sections {
        let mut seen_on_page = BTreeSet::new();
        let mut page_concept_ids = Vec::new();
        let mut page_evidence_ids = Vec::new();
        let candidates = concept_candidates(&section.content);
        for candidate in candidates {
            let key = normalize_key(&candidate);
            if key.is_empty() || !seen_on_page.insert(key.clone()) {
                continue;
            }
            let concept_id = format!("concept-{key}");
            let concept = concepts
                .entry(key.clone())
                .or_insert_with(|| ExtractedConcept {
                    id: concept_id.clone(),
                    label: candidate.clone(),
                    aliases: BTreeSet::new(),
                    evidence_ids: Vec::new(),
                    page_labels: BTreeSet::new(),
                });
            if concept.label != candidate {
                concept.aliases.insert(candidate.clone());
            }
            concept.page_labels.insert(section.page_label.clone());
            let evidence_id = format!("ev-{}-{}", key, concept.evidence_ids.len() + 1);
            evidence_refs.insert(
                evidence_id.clone(),
                ExtractionEvidenceRef {
                    id: evidence_id.clone(),
                    page_index: section.page_index,
                    page_label: section.page_label.clone(),
                    snippet: excerpt(&section.content, 180),
                    source_path: source_path.to_string(),
                    source_id: source_id.map(ToString::to_string),
                    markdown_path: section.markdown_path.clone(),
                    image_path: section.image_path.clone(),
                    provenance: format!(
                        "Concept '{}' was extracted from {} because the page text produced a stable candidate label.",
                        candidate, section.page_label
                    ),
                },
            );
            concept.evidence_ids.push(evidence_id.clone());
            page_evidence_ids.push(evidence_id.clone());
            claims.push(ExtractedClaim {
                id: format!("claim-{}-{}", key, claims.len() + 1),
                text: candidate.clone(),
                subject_concept_id: concept_id.clone(),
                evidence_id,
            });
            page_concept_ids.push(concept_id);
        }
        if !page_concept_ids.is_empty() {
            concept_ids_by_page.push((
                section.page_label.clone(),
                page_concept_ids,
                page_evidence_ids,
            ));
        }
    }

    for candidate in claim_candidates {
        let section = page_section_for_line(page_sections, candidate.line_start)
            .or_else(|| page_sections.first());
        let page_index = section.map(|section| section.page_index).unwrap_or(0);
        let page_label = section
            .map(|section| section.page_label.clone())
            .unwrap_or_else(|| "Imported text".into());
        let markdown_path = section.and_then(|section| section.markdown_path.clone());
        let image_path = section.and_then(|section| section.image_path.clone());
        evidence_refs
            .entry(candidate.evidence_id.clone())
            .or_insert_with(|| ExtractionEvidenceRef {
                id: candidate.evidence_id.clone(),
                page_index,
                page_label: page_label.clone(),
                snippet: candidate.evidence_snippet.clone(),
                source_path: source_path.to_string(),
                source_id: source_id.map(ToString::to_string),
                markdown_path,
                image_path,
                provenance: format!(
                    "Claim candidate was extracted from markdown line {} because {}.",
                    candidate.line_start, candidate.reason
                ),
            });

        let mut claim_subjects = candidate
            .subject_refs
            .iter()
            .filter(|subject_ref| concepts.values().any(|concept| &concept.id == *subject_ref))
            .cloned()
            .collect::<Vec<_>>();
        if claim_subjects.is_empty() {
            if let Some(label) = derive_concept_label(&candidate.statement) {
                let key = normalize_key(&label);
                let concept_id = format!("concept-{key}");
                if concepts.contains_key(&key) {
                    claim_subjects.push(concept_id);
                }
            }
        }
        for subject_concept_id in claim_subjects {
            claims.push(ExtractedClaim {
                id: candidate.candidate_id.clone(),
                text: candidate.statement.clone(),
                subject_concept_id,
                evidence_id: candidate.evidence_id.clone(),
            });
        }
    }

    if concepts.is_empty() {
        for (index, section) in page_sections.iter().enumerate() {
            let label = fallback_concept_label(&section.content, &section.page_label);
            let key = normalize_key(&label);
            let concept_id = format!("concept-{key}");
            concepts.insert(
                key.clone(),
                ExtractedConcept {
                    id: concept_id.clone(),
                    label,
                    aliases: BTreeSet::new(),
                    evidence_ids: vec![format!("ev-fallback-{}", index + 1)],
                    page_labels: [section.page_label.clone()].into_iter().collect(),
                },
            );
            let evidence_id = format!("ev-fallback-{}", index + 1);
            evidence_refs.insert(
                evidence_id.clone(),
                ExtractionEvidenceRef {
                    id: evidence_id.clone(),
                    page_index: section.page_index,
                    page_label: section.page_label.clone(),
                    snippet: excerpt(&section.content, 180),
                    source_path: source_path.to_string(),
                    source_id: source_id.map(ToString::to_string),
                    markdown_path: section.markdown_path.clone(),
                    image_path: section.image_path.clone(),
                    provenance: format!(
                        "Fallback concept extracted from {} because no stronger concept candidates were found.",
                        section.page_label
                    ),
                },
            );
            claims.push(ExtractedClaim {
                id: format!("claim-fallback-{}", index + 1),
                text: fallback_concept_label(&section.content, &section.page_label),
                subject_concept_id: concept_id.clone(),
                evidence_id: evidence_id.clone(),
            });
            concept_ids_by_page.push((
                section.page_label.clone(),
                vec![concept_id],
                vec![evidence_id],
            ));
        }
    }

    let concepts = concepts.into_values().take(20).collect::<Vec<_>>();
    let allowed_ids = concepts
        .iter()
        .map(|concept| concept.id.clone())
        .collect::<BTreeSet<_>>();
    let mut relations = Vec::new();
    let relationship_evidence =
        extract_markdown_relationship_evidence(markdown, source_path, source_id, node_candidates);
    for evidence in relationship_evidence {
        let source_key = normalize_key(&evidence.source_label);
        let target_key = normalize_key(&evidence.target_label);
        if source_key.is_empty() || target_key.is_empty() || source_key == target_key {
            continue;
        }
        let source_concept_id = evidence
            .resolved_source_node_id
            .clone()
            .or_else(|| evidence.matched_source_node_id.clone())
            .unwrap_or_else(|| format!("concept-{source_key}"));
        let target_concept_id = evidence
            .resolved_target_node_id
            .clone()
            .or_else(|| evidence.matched_target_node_id.clone())
            .unwrap_or_else(|| format!("concept-{target_key}"));
        if source_concept_id == target_concept_id {
            continue;
        }
        if !allowed_ids.contains(&source_concept_id) || !allowed_ids.contains(&target_concept_id) {
            continue;
        }
        let section = page_section_for_line(page_sections, evidence.line_start)
            .or_else(|| page_sections.first());
        let page_index = section.map(|section| section.page_index).unwrap_or(0);
        let page_label = section
            .map(|section| section.page_label.clone())
            .unwrap_or_else(|| "Imported text".into());
        let markdown_path = section.and_then(|section| section.markdown_path.clone());
        let image_path = section.and_then(|section| section.image_path.clone());
        evidence_refs
            .entry(evidence.evidence_id.clone())
            .or_insert_with(|| ExtractionEvidenceRef {
                id: evidence.evidence_id.clone(),
                page_index,
                page_label: page_label.clone(),
                snippet: evidence.snippet.clone(),
                source_path: source_path.to_string(),
                source_id: source_id.map(ToString::to_string),
                markdown_path,
                image_path,
                provenance: format!(
                    "Relationship evidence was extracted from markdown line {} because {}. Endpoints resolved as {}.",
                    evidence.line_start, evidence.reason, evidence.endpoint_resolution
                ),
            });
        relations.push(ExtractedRelation {
            source_concept_id,
            target_concept_id,
            relation_kind: evidence.relation_kind,
            confidence: evidence.confidence,
            evidence_ids: vec![evidence.evidence_id],
            page_labels: [page_label].into_iter().collect(),
        });
    }
    for (page_label, mut concept_ids, evidence_ids) in concept_ids_by_page {
        concept_ids.retain(|id| allowed_ids.contains(id));
        concept_ids.sort();
        concept_ids.dedup();
        for left_index in 0..concept_ids.len() {
            for right_index in (left_index + 1)..concept_ids.len() {
                let (source_concept_id, target_concept_id) =
                    if concept_ids[left_index] <= concept_ids[right_index] {
                        (
                            concept_ids[left_index].clone(),
                            concept_ids[right_index].clone(),
                        )
                    } else {
                        (
                            concept_ids[right_index].clone(),
                            concept_ids[left_index].clone(),
                        )
                    };
                relations.push(ExtractedRelation {
                    source_concept_id,
                    target_concept_id,
                    relation_kind: BrainRelationKind::RelatedTo,
                    confidence: 0.0,
                    evidence_ids: evidence_ids.clone(),
                    page_labels: [page_label.clone()].into_iter().collect(),
                });
            }
        }
    }

    ExtractionArtifact {
        concepts,
        claims,
        relations,
        evidence_refs,
    }
}

pub(super) fn collected_concepts_from_artifact(artifact: &ExtractionArtifact) -> CollectedConcepts {
    let allowed_ids = artifact
        .concepts
        .iter()
        .map(|concept| concept.id.clone())
        .collect::<BTreeSet<_>>();
    let concepts = artifact
        .concepts
        .iter()
        .map(|concept| ConceptAccumulator {
            id: concept.id.clone(),
            label: concept.label.clone(),
            aliases: concept.aliases.clone(),
            evidence: concept
                .evidence_ids
                .iter()
                .filter_map(|id| artifact.evidence_refs.get(id))
                .map(evidence_ref_from_extraction)
                .collect(),
            page_labels: concept.page_labels.clone(),
        })
        .collect::<Vec<_>>();
    let mut page_concepts_by_label = BTreeMap::<String, PageConceptSet>::new();
    let mut claims = artifact.claims.iter().collect::<Vec<_>>();
    claims.sort_by(|left, right| left.id.cmp(&right.id));
    for claim in claims {
        if !allowed_ids.contains(&claim.subject_concept_id) {
            continue;
        }
        let Some(evidence) = artifact.evidence_refs.get(&claim.evidence_id) else {
            continue;
        };
        let page = page_concepts_by_label
            .entry(evidence.page_label.clone())
            .or_insert_with(|| PageConceptSet {
                page_index: evidence.page_index,
                page_label: evidence.page_label.clone(),
                concept_ids: Vec::new(),
                snippet: evidence.snippet.clone(),
                markdown_path: evidence.markdown_path.clone(),
                image_path: evidence.image_path.clone(),
            });
        page.concept_ids.push(claim.subject_concept_id.clone());
        if claim.text.len() > page.snippet.len() {
            page.snippet = claim.text.clone();
        }
    }
    let mut relation_candidates = Vec::new();
    for relation in &artifact.relations {
        if !allowed_ids.contains(&relation.source_concept_id)
            || !allowed_ids.contains(&relation.target_concept_id)
        {
            continue;
        }
        let relation_evidence_refs = relation
            .evidence_ids
            .iter()
            .filter_map(|id| artifact.evidence_refs.get(id))
            .collect::<Vec<_>>();
        let relation_evidence = relation_evidence_refs
            .iter()
            .map(|evidence| evidence_ref_from_extraction(evidence))
            .collect::<Vec<_>>();
        relation_candidates.push(RelationCandidateAccumulator {
            source_node_id: relation.source_concept_id.clone(),
            target_node_id: relation.target_concept_id.clone(),
            relation_kind: relation.relation_kind,
            confidence: relation.confidence,
            evidence: relation_evidence,
            page_labels: relation.page_labels.clone(),
        });
        for page_label in &relation.page_labels {
            let relation_evidence = relation_evidence_refs
                .iter()
                .find(|evidence| &evidence.page_label == page_label)
                .copied()
                .or_else(|| relation_evidence_refs.first().copied());
            let Some(evidence) = relation_evidence else {
                continue;
            };
            let page = page_concepts_by_label
                .entry(page_label.clone())
                .or_insert_with(|| PageConceptSet {
                    page_index: evidence.page_index,
                    page_label: page_label.clone(),
                    concept_ids: Vec::new(),
                    snippet: evidence.snippet.clone(),
                    markdown_path: evidence.markdown_path.clone(),
                    image_path: evidence.image_path.clone(),
                });
            page.concept_ids.push(relation.source_concept_id.clone());
            page.concept_ids.push(relation.target_concept_id.clone());
            if evidence.snippet.len() > page.snippet.len()
                || evidence.id.starts_with("ev-relation-")
            {
                page.snippet = evidence.snippet.clone();
            }
        }
    }
    let page_concepts = page_concepts_by_label
        .into_values()
        .filter_map(|mut page| {
            page.concept_ids.sort();
            page.concept_ids.dedup();
            (!page.concept_ids.is_empty()).then_some(page)
        })
        .collect();
    CollectedConcepts {
        concepts,
        page_concepts,
        relation_candidates,
    }
}

pub(super) fn evidence_ref_from_extraction(evidence: &ExtractionEvidenceRef) -> EvidenceRef {
    EvidenceRef {
        id: evidence.id.clone(),
        page_label: evidence.page_label.clone(),
        page_index: Some(evidence.page_index),
        snippet: evidence.snippet.clone(),
        source_path: Some(evidence.source_path.clone()),
        source_id: evidence.source_id.clone(),
        markdown_path: evidence.markdown_path.clone(),
        image_path: evidence.image_path.clone(),
        provenance: Some(evidence.provenance.clone()),
    }
}

pub(super) fn build_relation_edges(
    document_node: &GraphNodeSummary,
    concept_accumulators: &[ConceptAccumulator],
    page_concepts: &[PageConceptSet],
    relation_candidates: &[RelationCandidateAccumulator],
    source_path: &str,
    source_id: Option<&str>,
) -> (
    Vec<RelationEdgeSummary>,
    BTreeMap<String, RelationEdgeDetail>,
    BTreeMap<String, usize>,
    BTreeMap<String, BTreeSet<String>>,
) {
    let mut edges = Vec::new();
    let mut edge_details_by_id = BTreeMap::new();
    let mut related_count_by_node_id = BTreeMap::<String, usize>::new();
    let mut connected_node_ids_by_node_id = BTreeMap::<String, BTreeSet<String>>::new();
    let concept_by_id = concept_accumulators
        .iter()
        .map(|concept| (concept.id.clone(), concept))
        .collect::<BTreeMap<_, _>>();

    for concept in concept_accumulators {
        let edge = RelationEdgeSummary {
            id: relation_edge_id(RelationKind::SourceDocument, &document_node.id, &concept.id),
            source_node_id: document_node.id.clone(),
            target_node_id: concept.id.clone(),
            kind: RelationKind::SourceDocument,
            label: "Compiled from source".into(),
            confidence: Some(0.94),
            evidence_count: concept.evidence.iter().take(2).count(),
        };
        let evidence = concept.evidence.iter().take(2).cloned().collect::<Vec<_>>();
        edge_details_by_id.insert(
            edge.id.clone(),
            RelationEdgeDetail {
                edge: edge.clone(),
                explanation: format!(
                    "HyprDuck linked the source document to {} because this concept was compiled from cited snippets in the import.",
                    concept.label
                ),
                evidence,
            },
        );
        note_relation(
            &mut related_count_by_node_id,
            &mut connected_node_ids_by_node_id,
            &edge.source_node_id,
            &edge.target_node_id,
        );
        edges.push(edge);
    }

    let mut concept_edge_accumulators = BTreeMap::<(String, String), EdgeAccumulator>::new();
    for candidate in relation_candidates {
        let (source_node_id, target_node_id) =
            if candidate.source_node_id <= candidate.target_node_id {
                (
                    candidate.source_node_id.clone(),
                    candidate.target_node_id.clone(),
                )
            } else {
                (
                    candidate.target_node_id.clone(),
                    candidate.source_node_id.clone(),
                )
            };
        let accumulator = concept_edge_accumulators
            .entry((source_node_id.clone(), target_node_id.clone()))
            .or_insert_with(|| EdgeAccumulator {
                source_node_id: source_node_id.clone(),
                target_node_id: target_node_id.clone(),
                relation_kind: candidate.relation_kind,
                label: markdown_relation_label(candidate.relation_kind),
                confidence: Some(candidate.confidence),
                evidence: Vec::new(),
                page_labels: BTreeSet::new(),
            });
        if accumulator.relation_kind == BrainRelationKind::RelatedTo
            && candidate.relation_kind != BrainRelationKind::RelatedTo
        {
            accumulator.relation_kind = candidate.relation_kind;
            accumulator.label = markdown_relation_label(candidate.relation_kind);
        }
        accumulator.confidence = match (accumulator.confidence, Some(candidate.confidence)) {
            (Some(left), Some(right)) => Some(left.max(right).min(0.94)),
            (Some(left), None) => Some(left),
            (None, Some(right)) => Some(right),
            (None, None) => None,
        };
        accumulator
            .page_labels
            .extend(candidate.page_labels.iter().cloned());
        accumulator
            .evidence
            .extend(candidate.evidence.iter().cloned());
    }
    for page in page_concepts {
        if page.concept_ids.len() < 2 {
            continue;
        }
        for left_index in 0..page.concept_ids.len() {
            for right_index in (left_index + 1)..page.concept_ids.len() {
                let left_id = &page.concept_ids[left_index];
                let right_id = &page.concept_ids[right_index];
                let (source_node_id, target_node_id) = if left_id <= right_id {
                    (left_id.clone(), right_id.clone())
                } else {
                    (right_id.clone(), left_id.clone())
                };
                let accumulator = concept_edge_accumulators
                    .entry((source_node_id.clone(), target_node_id.clone()))
                    .or_insert_with(|| EdgeAccumulator {
                        source_node_id: source_node_id.clone(),
                        target_node_id: target_node_id.clone(),
                        relation_kind: BrainRelationKind::RelatedTo,
                        label: "Related in source".into(),
                        confidence: None,
                        evidence: Vec::new(),
                        page_labels: BTreeSet::new(),
                    });
                accumulator.page_labels.insert(page.page_label.clone());
                accumulator.evidence.push(EvidenceRef {
                    id: format!(
                        "ev-edge-{}-{}-{}",
                        source_node_id,
                        target_node_id,
                        accumulator.evidence.len() + 1
                    ),
                    page_label: page.page_label.clone(),
                    page_index: Some(page.page_index),
                    snippet: page.snippet.clone(),
                    source_path: Some(source_path.to_string()),
                    source_id: source_id.map(ToString::to_string),
                    markdown_path: page.markdown_path.clone(),
                    image_path: page.image_path.clone(),
                    provenance: Some(format!(
                        "Relation evidence extracted because both concepts appeared in {}.",
                        page.page_label
                    )),
                });
            }
        }
    }

    let mut concept_edges = concept_edge_accumulators.into_values().collect::<Vec<_>>();
    concept_edges.sort_by(|left, right| {
        right
            .page_labels
            .len()
            .cmp(&left.page_labels.len())
            .then_with(|| left.source_node_id.cmp(&right.source_node_id))
            .then_with(|| left.target_node_id.cmp(&right.target_node_id))
    });

    for accumulator in concept_edges.into_iter().take(16) {
        let source_label = concept_by_id
            .get(&accumulator.source_node_id)
            .map(|concept| concept.label.clone())
            .unwrap_or_else(|| accumulator.source_node_id.clone());
        let target_label = concept_by_id
            .get(&accumulator.target_node_id)
            .map(|concept| concept.label.clone())
            .unwrap_or_else(|| accumulator.target_node_id.clone());
        let edge = RelationEdgeSummary {
            id: format!(
                "edge-{}-{}",
                accumulator.source_node_id, accumulator.target_node_id
            ),
            source_node_id: accumulator.source_node_id.clone(),
            target_node_id: accumulator.target_node_id.clone(),
            kind: RelationKind::RelatedTo,
            label: accumulator.label.clone(),
            confidence: accumulator.confidence.or_else(|| {
                Some((0.56 + (accumulator.page_labels.len().min(3) as f32 * 0.08)).min(0.84))
            }),
            evidence_count: accumulator.evidence.len(),
        };
        edge_details_by_id.insert(
            edge.id.clone(),
            RelationEdgeDetail {
                edge: edge.clone(),
                explanation: format!(
                    "HyprDuck linked {} and {} because they appeared together in {} page section(s).",
                    source_label,
                    target_label,
                    accumulator.page_labels.len()
                ),
                evidence: accumulator.evidence.clone(),
            },
        );
        note_relation(
            &mut related_count_by_node_id,
            &mut connected_node_ids_by_node_id,
            &edge.source_node_id,
            &edge.target_node_id,
        );
        edges.push(edge);
    }

    (
        edges,
        edge_details_by_id,
        related_count_by_node_id,
        connected_node_ids_by_node_id,
    )
}

pub(super) fn note_relation(
    related_count_by_node_id: &mut BTreeMap<String, usize>,
    connected_node_ids_by_node_id: &mut BTreeMap<String, BTreeSet<String>>,
    source_node_id: &str,
    target_node_id: &str,
) {
    *related_count_by_node_id
        .entry(source_node_id.to_string())
        .or_default() += 1;
    *related_count_by_node_id
        .entry(target_node_id.to_string())
        .or_default() += 1;
    connected_node_ids_by_node_id
        .entry(source_node_id.to_string())
        .or_default()
        .insert(target_node_id.to_string());
    connected_node_ids_by_node_id
        .entry(target_node_id.to_string())
        .or_default()
        .insert(source_node_id.to_string());
}

pub(super) fn concept_candidates(content: &str) -> Vec<String> {
    let mut labels: Vec<String> = Vec::new();
    for line in content.lines() {
        let cleaned = clean_candidate_line(line);
        if cleaned.is_empty() {
            continue;
        }
        if let Some(label) = derive_concept_label(&cleaned) {
            if !labels
                .iter()
                .any(|existing| normalize_key(existing) == normalize_key(&label))
            {
                labels.push(label);
            }
        }
        if labels.len() >= 3 {
            break;
        }
    }
    labels
}

pub(super) fn clean_candidate_line(value: &str) -> String {
    let trimmed = value.trim();
    if trimmed.is_empty()
        || trimmed.starts_with("![")
        || trimmed.starts_with("_AI analysis unavailable")
        || trimmed.starts_with("# ")
        || trimmed.starts_with("## Page ")
    {
        return String::new();
    }

    trimmed
        .trim_start_matches('#')
        .trim_start_matches('-')
        .trim_start_matches('*')
        .trim_start_matches(|c: char| c.is_ascii_digit() || c == '.' || c == ')')
        .trim()
        .replace('`', "")
        .replace('*', "")
}

pub(super) fn derive_concept_label(value: &str) -> Option<String> {
    let first_clause = value
        .split(|char| matches!(char, '.' | ':' | ';' | '(' | ')' | '[' | ']'))
        .next()
        .unwrap_or(value)
        .trim();
    let mut words = first_clause
        .split_whitespace()
        .map(|word| {
            word.trim_matches(|char: char| !char.is_alphanumeric() && char != '-' && char != '/')
        })
        .filter(|word| !word.is_empty())
        .collect::<Vec<_>>();

    while matches!(words.first(), Some(word) if is_leading_stopword(word)) {
        words.remove(0);
    }

    if words.len() < 2 {
        return None;
    }

    let label = words.into_iter().take(6).collect::<Vec<_>>().join(" ");
    if label.len() < 10 {
        return None;
    }

    Some(label)
}

pub(super) fn is_leading_stopword(value: &str) -> bool {
    matches!(
        value.to_ascii_lowercase().as_str(),
        "a" | "an"
            | "and"
            | "as"
            | "for"
            | "from"
            | "in"
            | "into"
            | "of"
            | "on"
            | "or"
            | "the"
            | "this"
            | "that"
            | "to"
            | "with"
    )
}

pub(super) fn fallback_concept_label(content: &str, page_label: &str) -> String {
    derive_concept_label(content).unwrap_or_else(|| format!("{page_label} summary"))
}

pub(super) fn normalize_key(value: &str) -> String {
    let mut normalized = String::new();
    let mut last_dash = false;
    for char in value.chars() {
        if char.is_ascii_alphanumeric() {
            normalized.push(char.to_ascii_lowercase());
            last_dash = false;
        } else if !last_dash {
            normalized.push('-');
            last_dash = true;
        }
    }
    normalized.trim_matches('-').to_string()
}

pub(super) fn extract_page_sections(markdown: &str) -> Vec<PageSection> {
    let normalized = markdown.replace("\r\n", "\n");
    let headers = regex_like_page_headers(&normalized);
    if headers.is_empty() {
        return vec![PageSection {
            page_index: 0,
            page_label: "Imported text".into(),
            content: normalized,
            markdown_path: None,
            image_path: None,
        }];
    }

    let mut sections = Vec::with_capacity(headers.len());
    for index in 0..headers.len() {
        let (page_label, _, content_start) = &headers[index];
        let next_start = headers
            .get(index + 1)
            .map(|(_, next_start, _)| *next_start)
            .unwrap_or(normalized.len());
        sections.push(PageSection {
            page_index: index,
            page_label: page_label.clone(),
            content: normalized[*content_start..next_start].trim().to_string(),
            markdown_path: None,
            image_path: None,
        });
    }
    sections
}

pub(super) fn attach_page_artifacts_to_sections(
    sections: &mut [PageSection],
    source_manifest: Option<&SourceArtifactManifest>,
) {
    let Some(manifest) = source_manifest else {
        return;
    };
    for section in sections {
        let artifact = manifest
            .pages
            .iter()
            .find(|page| page.label == section.page_label)
            .or_else(|| manifest.pages.get(section.page_index));
        if let Some(artifact) = artifact {
            section.page_index = artifact.index;
            section.markdown_path = artifact.markdown_path.clone();
            section.image_path = artifact.image_path.clone();
        }
    }
}

pub(super) fn regex_like_page_headers(markdown: &str) -> Vec<(String, usize, usize)> {
    let mut headers = Vec::new();
    let mut offset = 0usize;
    for line in markdown.lines() {
        let line_len = line.len();
        if let Some(page_label) = line
            .strip_prefix("## Page ")
            .map(|page| format!("Page {}", page.trim()))
        {
            headers.push((page_label, offset, offset + line_len + 1));
        }
        offset += line_len + 1;
    }
    headers
}

pub(super) fn infer_markdown_title(markdown_path: &str, markdown: &str) -> String {
    if let Some(heading) = markdown
        .lines()
        .find_map(|line| line.strip_prefix("# ").map(str::trim))
        .filter(|value| !value.is_empty())
    {
        return heading.to_string();
    }

    Path::new(markdown_path)
        .file_stem()
        .map(|value| value.to_string_lossy().to_string())
        .unwrap_or_else(|| "HyprDuck import".into())
}

pub(super) fn excerpt(value: &str, max_length: usize) -> String {
    let compact = value.split_whitespace().collect::<Vec<_>>().join(" ");
    if compact.is_empty() {
        return "No visible evidence snippet is available yet.".into();
    }
    let compact_chars = compact.chars().count();
    if compact_chars <= max_length {
        return compact;
    }
    let truncated = compact
        .chars()
        .take(max_length.saturating_sub(1))
        .collect::<String>();
    format!("{}…", truncated.trim_end())
}

pub(super) fn correction_actions_for_detail(
    _canonical_name: &str,
    aliases: &[String],
) -> Vec<CorrectionAction> {
    vec![
        CorrectionAction {
            kind: CorrectionKind::Merge,
            label: "Merge".into(),
            disabled_reason: None,
        },
        CorrectionAction {
            kind: CorrectionKind::KeepSeparate,
            label: "Keep Separate".into(),
            disabled_reason: if aliases.is_empty() {
                Some("No grouped aliases are available to split yet.".into())
            } else {
                None
            },
        },
        CorrectionAction {
            kind: CorrectionKind::Rename,
            label: "Rename".into(),
            disabled_reason: None,
        },
        CorrectionAction {
            kind: CorrectionKind::Split,
            label: "Split".into(),
            disabled_reason: None,
        },
    ]
}

#[derive(Debug, Clone)]
pub(super) struct StoredEdgeAccumulator {
    pub(super) kind: RelationKind,
    pub(super) source_node_id: String,
    pub(super) target_node_id: String,
    pub(super) label: String,
    pub(super) confidence: Option<f32>,
    pub(super) evidence: Vec<EvidenceRef>,
}

pub(super) fn apply_correction(
    project: &mut KnowledgeProject,
    request: &ApplyCorrectionRequest,
) -> Result<()> {
    match request.kind {
        CorrectionKind::Rename => apply_rename_correction(project, request)?,
        CorrectionKind::Merge => apply_merge_correction(project, request)?,
        CorrectionKind::KeepSeparate => apply_keep_separate_correction(project, request)?,
        CorrectionKind::Split => apply_split_correction(project, request)?,
    }
    refresh_project_after_correction(project);
    Ok(())
}

pub(super) fn answer_project(
    project: &KnowledgeProject,
    request: &AnswerProjectRequest,
) -> Result<AnswerResponse> {
    let question = request.question.trim();
    if question.is_empty() {
        return Ok(AnswerResponse {
            status: AnswerStatus::Blocked,
            text: None,
            explanation: "Ask a concrete question before HyprDuck tries to answer from the graph."
                .into(),
            citations: Vec::new(),
            related_node_ids: request
                .node_id
                .clone()
                .into_iter()
                .collect(),
            suggested_actions: vec![SuggestedAction {
                kind: SuggestedActionKind::AskDifferentQuestion,
                label: "Ask a concrete question".into(),
                description:
                    "Grounded answers work best when the question names a concept, action, or relationship."
                        .into(),
            }],
        });
    }

    let focal_node_id = select_focal_node_id(project, request, question)?;

    let detail = project
        .details_by_node_id
        .get(&focal_node_id)
        .ok_or_else(|| anyhow!("node detail {} was not found", focal_node_id))?;
    let base_answer = project
        .answer_by_node_id
        .get(&focal_node_id)
        .cloned()
        .unwrap_or_else(|| build_answer_for_detail(project, detail, Vec::new()));
    let citations = best_matching_evidence(question, detail);
    let status = if citations.is_empty() {
        if detail.evidence.is_empty() {
            AnswerStatus::Blocked
        } else {
            AnswerStatus::LowConfidence
        }
    } else {
        AnswerStatus::Grounded
    };
    let related_node_ids = if base_answer.related_node_ids.is_empty() {
        project
            .edges
            .iter()
            .filter_map(|edge| {
                if edge.source_node_id == focal_node_id {
                    Some(edge.target_node_id.clone())
                } else if edge.target_node_id == focal_node_id {
                    Some(edge.source_node_id.clone())
                } else {
                    None
                }
            })
            .collect()
    } else {
        base_answer.related_node_ids.clone()
    };

    Ok(AnswerResponse {
        status,
        text: Some(answer_text_for_question(
            project, detail, question, status, &citations,
        )),
        explanation: answer_explanation_for_question(detail, question, status, &citations),
        citations,
        related_node_ids,
        suggested_actions: answer_suggested_actions(status),
    })
}

pub(super) fn select_focal_node_id(
    project: &KnowledgeProject,
    request: &AnswerProjectRequest,
    question: &str,
) -> Result<String> {
    if let Some(node_id) = request.node_id.as_deref() {
        if project.details_by_node_id.contains_key(node_id) {
            return Ok(node_id.to_string());
        }
        bail!(
            "node {node_id} was not found in project {}",
            request.project_id
        );
    }

    if project.summary.project_id.starts_with("workspace:") {
        if let Some(node_id) = best_matching_detail_node_id(project, question) {
            return Ok(node_id);
        }
    }

    project
        .nodes
        .iter()
        .find(|node| is_source_like_node_kind(node.kind))
        .map(|node| node.id.clone())
        .ok_or_else(|| {
            anyhow!(
                "no answerable node was found in project {}",
                request.project_id
            )
        })
}

pub(super) fn best_matching_detail_node_id(
    project: &KnowledgeProject,
    question: &str,
) -> Option<String> {
    let terms = question_terms(question);
    if terms.is_empty() {
        return None;
    }

    project
        .details_by_node_id
        .values()
        .map(|detail| {
            let mut detail_terms = text_terms(&detail.canonical_name);
            detail_terms.extend(detail.aliases.iter().flat_map(|alias| text_terms(alias)));
            for evidence in &detail.evidence {
                detail_terms.extend(text_terms(&evidence.snippet));
            }
            let score = terms.intersection(&detail_terms).count();
            (score, detail.node.id.clone())
        })
        .filter(|(score, _)| *score > 0)
        .max_by(|left, right| left.0.cmp(&right.0).then_with(|| left.1.cmp(&right.1)))
        .map(|(_, node_id)| node_id)
}

pub(super) fn apply_rename_correction(
    project: &mut KnowledgeProject,
    request: &ApplyCorrectionRequest,
) -> Result<()> {
    let next_name = request
        .value
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| anyhow!("rename needs a non-empty canonical name"))?;
    let node_kind = project
        .nodes
        .iter()
        .find(|node| node.id == request.node_id)
        .map(|node| node.kind)
        .ok_or_else(|| anyhow!("node {} was not found", request.node_id))?;
    if node_kind != GraphNodeKind::Concept {
        bail!("only concept nodes can be renamed");
    }

    let previous_name = project
        .details_by_node_id
        .get(&request.node_id)
        .ok_or_else(|| anyhow!("node detail {} was not found", request.node_id))?
        .canonical_name
        .clone();
    if previous_name == next_name {
        return Ok(());
    }
    let previous_node_id = request.node_id.clone();
    let next_node_id = unique_renamed_node_id(project, &previous_node_id, next_name);

    let node = project
        .nodes
        .iter_mut()
        .find(|node| node.id == request.node_id)
        .ok_or_else(|| anyhow!("node {} was not found", request.node_id))?;
    let detail = project
        .details_by_node_id
        .get_mut(&request.node_id)
        .ok_or_else(|| anyhow!("node detail {} was not found", request.node_id))?;

    let mut aliases = detail.aliases.iter().cloned().collect::<BTreeSet<_>>();
    aliases.insert(previous_name.clone());
    aliases.remove(next_name);
    detail.aliases = aliases.into_iter().collect();
    detail.canonical_name = next_name.to_string();
    detail.description = format!(
        "Renamed from {} to {}. HyprDuck kept the previous canonical label as an alias so the evidence trail stays intact.",
        previous_name, next_name
    );
    node.id = next_node_id.clone();
    node.label = next_name.to_string();
    detail.node = node.clone();

    if next_node_id != previous_node_id {
        let detail = project
            .details_by_node_id
            .remove(&previous_node_id)
            .ok_or_else(|| anyhow!("node detail {previous_node_id} was not found"))?;
        project
            .details_by_node_id
            .insert(next_node_id.clone(), detail);
        if let Some(answer) = project.answer_by_node_id.remove(&previous_node_id) {
            project
                .answer_by_node_id
                .insert(next_node_id.clone(), answer);
        }
        rewrite_project_edges(project, Some((&previous_node_id, &next_node_id)));
    }

    Ok(())
}

pub(super) fn unique_renamed_node_id(
    project: &KnowledgeProject,
    current_node_id: &str,
    label: &str,
) -> String {
    let base = normalize_key(label);
    if base.is_empty() {
        return current_node_id.to_string();
    }
    let base_id = format!("concept-{base}");
    if base_id == current_node_id
        || !project
            .nodes
            .iter()
            .any(|node| node.id == base_id && node.id != current_node_id)
    {
        return base_id;
    }

    let mut suffix = 2usize;
    loop {
        let candidate = format!("concept-{base}-rename-{suffix}");
        if !project
            .nodes
            .iter()
            .any(|node| node.id == candidate && node.id != current_node_id)
        {
            return candidate;
        }
        suffix += 1;
    }
}

pub(super) fn apply_merge_correction(
    project: &mut KnowledgeProject,
    request: &ApplyCorrectionRequest,
) -> Result<()> {
    let target_node_id = request
        .target_node_id
        .as_deref()
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| anyhow!("merge needs a target concept"))?;
    if target_node_id == request.node_id {
        bail!("merge target must be different from the selected node");
    }

    let source_node = project
        .nodes
        .iter()
        .find(|node| node.id == request.node_id)
        .cloned()
        .ok_or_else(|| anyhow!("node {} was not found", request.node_id))?;
    let target_node = project
        .nodes
        .iter()
        .find(|node| node.id == target_node_id)
        .cloned()
        .ok_or_else(|| anyhow!("target node {} was not found", target_node_id))?;
    if source_node.kind != GraphNodeKind::Concept || target_node.kind != GraphNodeKind::Concept {
        bail!("merge only supports concept nodes");
    }

    let source_detail = project
        .details_by_node_id
        .get(&request.node_id)
        .cloned()
        .ok_or_else(|| anyhow!("node detail {} was not found", request.node_id))?;
    let target_name = project
        .details_by_node_id
        .get(target_node_id)
        .map(|detail| detail.canonical_name.clone())
        .ok_or_else(|| anyhow!("target node detail {} was not found", target_node_id))?;

    {
        let target_detail = project
            .details_by_node_id
            .get_mut(target_node_id)
            .ok_or_else(|| anyhow!("target node detail {} was not found", target_node_id))?;
        let mut aliases = target_detail
            .aliases
            .iter()
            .cloned()
            .collect::<BTreeSet<_>>();
        aliases.insert(source_detail.canonical_name.clone());
        aliases.extend(source_detail.aliases.iter().cloned());
        aliases.remove(&target_name);
        target_detail.aliases = aliases.into_iter().collect();
        target_detail.evidence = dedupe_evidence(
            target_detail
                .evidence
                .clone()
                .into_iter()
                .chain(source_detail.evidence.clone())
                .collect(),
        );
        target_detail.description = format!(
            "Merged {} into {}. HyprDuck kept all visible evidence on the surviving concept.",
            source_detail.canonical_name, target_name
        );
    }

    if let Some(node) = project
        .nodes
        .iter_mut()
        .find(|node| node.id == target_node_id)
    {
        node.evidence_count = project
            .details_by_node_id
            .get(target_node_id)
            .map(|detail| detail.evidence.len())
            .unwrap_or(node.evidence_count);
        node.confidence = Some(
            node.confidence
                .unwrap_or(0.72)
                .max(source_node.confidence.unwrap_or(0.72))
                .min(0.94),
        );
    }

    project.nodes.retain(|node| node.id != request.node_id);
    project.details_by_node_id.remove(&request.node_id);
    project.answer_by_node_id.remove(&request.node_id);
    rewrite_project_edges(project, Some((&request.node_id, target_node_id)));

    Ok(())
}

pub(super) fn apply_keep_separate_correction(
    project: &mut KnowledgeProject,
    request: &ApplyCorrectionRequest,
) -> Result<()> {
    let source_node = project
        .nodes
        .iter()
        .find(|node| node.id == request.node_id)
        .cloned()
        .ok_or_else(|| anyhow!("node {} was not found", request.node_id))?;
    if source_node.kind != GraphNodeKind::Concept {
        bail!("keep separate only supports concept nodes");
    }

    let source_detail = project
        .details_by_node_id
        .get(&request.node_id)
        .cloned()
        .ok_or_else(|| anyhow!("node detail {} was not found", request.node_id))?;
    if source_detail.aliases.is_empty() {
        bail!("keep separate needs at least one grouped alias");
    }

    {
        let detail = project
            .details_by_node_id
            .get_mut(&request.node_id)
            .ok_or_else(|| anyhow!("node detail {} was not found", request.node_id))?;
        detail.aliases.clear();
        detail.description = format!(
            "HyprDuck kept the previous aliases under {} as distinct concept nodes after a manual correction.",
            detail.canonical_name
        );
    }

    let split_evidence = source_detail.evidence.clone();
    for (index, alias) in source_detail.aliases.iter().enumerate() {
        let new_node_id = unique_manual_node_id(project, alias);
        let new_node = GraphNodeSummary {
            id: new_node_id.clone(),
            label: alias.clone(),
            kind: GraphNodeKind::Concept,
            confidence: Some(source_node.confidence.unwrap_or(0.68).min(0.82)),
            related_count: 0,
            evidence_count: split_evidence.len(),
            position: manual_split_position(&source_node.position, index),
        };
        project.nodes.push(new_node.clone());
        project.details_by_node_id.insert(
            new_node_id.clone(),
            GraphNodeDetail {
                node: new_node.clone(),
                canonical_name: alias.clone(),
                aliases: Vec::new(),
                description: format!(
                    "Created from a keep separate correction on {}. HyprDuck preserved the supporting evidence while treating this as its own concept.",
                    source_detail.canonical_name
                ),
                evidence: split_evidence.clone(),
                actions: Vec::new(),
                source: None,
            },
        );

        for source_node_id in source_like_node_ids_for_concept(project, &request.node_id) {
            let document_evidence = split_evidence.iter().take(2).cloned().collect::<Vec<_>>();
            let document_edge = RelationEdgeSummary {
                id: relation_edge_id(RelationKind::SourceDocument, &source_node_id, &new_node_id),
                source_node_id: source_node_id.clone(),
                target_node_id: new_node_id.clone(),
                kind: RelationKind::SourceDocument,
                label: "Compiled from source".into(),
                confidence: Some(0.76),
                evidence_count: document_evidence.len(),
            };
            project.edges.push(document_edge.clone());
            project.edge_details_by_id.insert(
                document_edge.id.clone(),
                RelationEdgeDetail {
                    edge: document_edge,
                    explanation: String::new(),
                    evidence: document_evidence,
                },
            );
        }

        let (source_node_id, target_node_id) = if request.node_id <= new_node_id {
            (request.node_id.clone(), new_node_id.clone())
        } else {
            (new_node_id.clone(), request.node_id.clone())
        };
        let relation_evidence = split_evidence.iter().take(2).cloned().collect::<Vec<_>>();
        let relation_edge = RelationEdgeSummary {
            id: relation_edge_id(RelationKind::RelatedTo, &source_node_id, &target_node_id),
            source_node_id,
            target_node_id,
            kind: RelationKind::RelatedTo,
            label: "Separated by correction".into(),
            confidence: Some(0.68),
            evidence_count: relation_evidence.len(),
        };
        project.edges.push(relation_edge.clone());
        project.edge_details_by_id.insert(
            relation_edge.id.clone(),
            RelationEdgeDetail {
                edge: relation_edge,
                explanation: String::new(),
                evidence: relation_evidence,
            },
        );
    }

    Ok(())
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct SplitReplacementMapping {
    #[serde(default)]
    pub(super) replacement_node_id: Option<String>,
    pub(super) replacement_label: String,
    #[serde(default)]
    pub(super) aliases: Vec<String>,
    pub(super) evidence_ids: Vec<String>,
    #[serde(default)]
    pub(super) edge_ids: Vec<String>,
}

pub(super) fn apply_split_correction(
    project: &mut KnowledgeProject,
    request: &ApplyCorrectionRequest,
) -> Result<()> {
    let source_node = project
        .nodes
        .iter()
        .find(|node| node.id == request.node_id)
        .cloned()
        .ok_or_else(|| anyhow!("node {} was not found", request.node_id))?;
    if source_node.kind != GraphNodeKind::Concept {
        bail!("split only supports concept nodes");
    }
    let source_detail = project
        .details_by_node_id
        .get(&request.node_id)
        .cloned()
        .ok_or_else(|| anyhow!("node detail {} was not found", request.node_id))?;
    let mappings = parse_split_replacement_mappings(request)?;
    let source_evidence_ids = source_detail
        .evidence
        .iter()
        .map(|evidence| evidence.id.clone())
        .collect::<BTreeSet<_>>();

    let mut replacement_ids = BTreeSet::new();
    let mut replacements = Vec::new();
    for (index, mapping) in mappings.iter().enumerate() {
        let label = mapping.replacement_label.trim();
        if label.is_empty() {
            bail!("split replacement label cannot be empty");
        }
        if mapping.evidence_ids.is_empty() {
            bail!("split replacement {label} needs evidenceIds");
        }
        let selected_evidence_ids = mapping
            .evidence_ids
            .iter()
            .map(|id| id.trim().to_string())
            .filter(|id| !id.is_empty())
            .collect::<BTreeSet<_>>();
        if selected_evidence_ids.is_empty() {
            bail!("split replacement {label} needs non-empty evidenceIds");
        }
        if !selected_evidence_ids
            .iter()
            .all(|id| source_evidence_ids.contains(id))
        {
            bail!("split replacement {label} references evidence outside the selected node");
        }
        let replacement_evidence = source_detail
            .evidence
            .iter()
            .filter(|evidence| selected_evidence_ids.contains(&evidence.id))
            .cloned()
            .collect::<Vec<_>>();
        let replacement_node_id = mapping
            .replacement_node_id
            .as_deref()
            .map(str::trim)
            .filter(|id| !id.is_empty())
            .map(ToOwned::to_owned)
            .unwrap_or_else(|| unique_manual_node_id(project, label));
        if replacement_node_id == request.node_id {
            bail!("split replacement {label} cannot reuse the original node id");
        }
        if !replacement_ids.insert(replacement_node_id.clone()) {
            bail!("split replacement node id {replacement_node_id} is duplicated");
        }
        if project
            .nodes
            .iter()
            .any(|node| node.id == replacement_node_id && node.id != request.node_id)
        {
            bail!("split replacement node id {replacement_node_id} already exists");
        }
        replacements.push((
            mapping,
            replacement_node_id,
            selected_evidence_ids,
            replacement_evidence,
            index,
        ));
    }

    let source_like_ids = source_like_node_ids_for_concept(project, &request.node_id);
    let existing_edges = project.edges.clone();
    let existing_edge_details = project.edge_details_by_id.clone();

    project.nodes.retain(|node| node.id != request.node_id);
    project.details_by_node_id.remove(&request.node_id);
    project.answer_by_node_id.remove(&request.node_id);
    project.edges.retain(|edge| {
        edge.source_node_id != request.node_id && edge.target_node_id != request.node_id
    });
    project.edge_details_by_id.retain(|_, detail| {
        detail.edge.source_node_id != request.node_id
            && detail.edge.target_node_id != request.node_id
    });

    for (mapping, replacement_node_id, selected_evidence_ids, replacement_evidence, index) in
        replacements
    {
        let label = mapping.replacement_label.trim().to_string();
        let aliases = mapping
            .aliases
            .iter()
            .map(|alias| alias.trim().to_string())
            .filter(|alias| !alias.is_empty() && alias != &label)
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect::<Vec<_>>();
        let new_node = GraphNodeSummary {
            id: replacement_node_id.clone(),
            label: label.clone(),
            kind: GraphNodeKind::Concept,
            confidence: Some(source_node.confidence.unwrap_or(0.7).min(0.88)),
            related_count: 0,
            evidence_count: replacement_evidence.len(),
            position: manual_split_position(&source_node.position, index),
        };
        project.nodes.push(new_node.clone());
        project.details_by_node_id.insert(
            replacement_node_id.clone(),
            GraphNodeDetail {
                node: new_node,
                canonical_name: label.clone(),
                aliases,
                description: format!(
                    "Created from an explicit split correction on {}. HyprDuck moved the mapped evidence, edges, claims, and wiki references onto this replacement concept.",
                    source_detail.canonical_name
                ),
                evidence: replacement_evidence.clone(),
                actions: Vec::new(),
                source: None,
            },
        );

        let mut copied_source_edges = BTreeSet::new();
        for edge in &existing_edges {
            if edge.source_node_id != request.node_id && edge.target_node_id != request.node_id {
                continue;
            }
            let edge_evidence = existing_edge_details
                .get(&edge.id)
                .map(|detail| detail.evidence.clone())
                .unwrap_or_default();
            if !split_mapping_matches_edge(mapping, &selected_evidence_ids, edge, &edge_evidence) {
                continue;
            }
            let mut next_edge = edge.clone();
            if next_edge.source_node_id == request.node_id {
                next_edge.source_node_id = replacement_node_id.clone();
            }
            if next_edge.target_node_id == request.node_id {
                next_edge.target_node_id = replacement_node_id.clone();
            }
            if next_edge.source_node_id == next_edge.target_node_id {
                continue;
            }
            if next_edge.kind == RelationKind::SourceDocument {
                for node_id in [&next_edge.source_node_id, &next_edge.target_node_id] {
                    if source_like_ids.contains(node_id) {
                        copied_source_edges.insert(node_id.clone());
                    }
                }
            }
            next_edge.id = relation_edge_id(
                next_edge.kind,
                &next_edge.source_node_id,
                &next_edge.target_node_id,
            );
            let next_evidence = if edge_evidence.is_empty() {
                replacement_evidence.iter().take(2).cloned().collect()
            } else {
                edge_evidence
            };
            next_edge.evidence_count = next_evidence.len();
            project.edges.push(next_edge.clone());
            project.edge_details_by_id.insert(
                next_edge.id.clone(),
                RelationEdgeDetail {
                    edge: next_edge,
                    explanation: String::new(),
                    evidence: next_evidence,
                },
            );
        }

        for source_node_id in source_like_ids
            .iter()
            .filter(|source_node_id| !copied_source_edges.contains(*source_node_id))
        {
            let document_evidence = replacement_evidence
                .iter()
                .take(2)
                .cloned()
                .collect::<Vec<_>>();
            let document_edge = RelationEdgeSummary {
                id: relation_edge_id(
                    RelationKind::SourceDocument,
                    source_node_id,
                    &replacement_node_id,
                ),
                source_node_id: source_node_id.clone(),
                target_node_id: replacement_node_id.clone(),
                kind: RelationKind::SourceDocument,
                label: "Compiled from source".into(),
                confidence: Some(0.76),
                evidence_count: document_evidence.len(),
            };
            project.edges.push(document_edge.clone());
            project.edge_details_by_id.insert(
                document_edge.id.clone(),
                RelationEdgeDetail {
                    edge: document_edge,
                    explanation: String::new(),
                    evidence: document_evidence,
                },
            );
        }
    }

    Ok(())
}

pub(super) fn parse_split_replacement_mappings(
    request: &ApplyCorrectionRequest,
) -> Result<Vec<SplitReplacementMapping>> {
    let value = request
        .value
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| anyhow!("split needs value JSON with replacement mappings"))?;
    let mappings: Vec<SplitReplacementMapping> =
        serde_json::from_str(value).context("split value must be a JSON array of mappings")?;
    if mappings.len() < 2 {
        bail!("split needs at least two replacement mappings");
    }
    Ok(mappings)
}

pub(super) fn split_mapping_matches_edge(
    mapping: &SplitReplacementMapping,
    selected_evidence_ids: &BTreeSet<String>,
    edge: &RelationEdgeSummary,
    edge_evidence: &[EvidenceRef],
) -> bool {
    if mapping.edge_ids.iter().any(|edge_id| edge_id == &edge.id) {
        return true;
    }
    edge_evidence
        .iter()
        .any(|evidence| selected_evidence_ids.contains(&evidence.id))
}

pub(super) fn refresh_project_after_correction(project: &mut KnowledgeProject) {
    rewrite_project_edges(project, None);

    let node_ids = project
        .nodes
        .iter()
        .map(|node| node.id.clone())
        .collect::<BTreeSet<_>>();
    project
        .details_by_node_id
        .retain(|node_id, _| node_ids.contains(node_id));
    project
        .answer_by_node_id
        .retain(|node_id, _| node_ids.contains(node_id));
    project.edges.retain(|edge| {
        node_ids.contains(&edge.source_node_id)
            && node_ids.contains(&edge.target_node_id)
            && edge.source_node_id != edge.target_node_id
    });
    project.edge_details_by_id.retain(|edge_id, detail| {
        node_ids.contains(&detail.edge.source_node_id)
            && node_ids.contains(&detail.edge.target_node_id)
            && detail.edge.source_node_id != detail.edge.target_node_id
            && edge_id == &detail.edge.id
    });

    let mut related_count_by_node_id = BTreeMap::<String, usize>::new();
    let mut connected_node_ids_by_node_id = BTreeMap::<String, BTreeSet<String>>::new();
    for edge in &project.edges {
        note_relation(
            &mut related_count_by_node_id,
            &mut connected_node_ids_by_node_id,
            &edge.source_node_id,
            &edge.target_node_id,
        );
    }

    for node in &mut project.nodes {
        if let Some(detail) = project.details_by_node_id.get_mut(&node.id) {
            if node.kind == GraphNodeKind::Concept {
                node.label = detail.canonical_name.clone();
                node.evidence_count = detail.evidence.len();
                detail.actions =
                    correction_actions_for_detail(&detail.canonical_name, &detail.aliases);
            } else {
                detail.actions = Vec::new();
            }
            node.related_count = related_count_by_node_id.get(&node.id).copied().unwrap_or(0);
            detail.node = node.clone();
        }
    }

    let label_by_node_id = project
        .nodes
        .iter()
        .map(|node| (node.id.clone(), node.label.clone()))
        .collect::<BTreeMap<_, _>>();
    for edge in &project.edges {
        if let Some(detail) = project.edge_details_by_id.get_mut(&edge.id) {
            detail.edge = edge.clone();
            detail.explanation = edge_explanation(edge, &label_by_node_id, &detail.evidence);
        }
    }

    for node in &project.nodes {
        if let Some(detail) = project.details_by_node_id.get(&node.id).cloned() {
            let related_node_ids = connected_node_ids_by_node_id
                .get(&node.id)
                .map(|related| related.iter().cloned().collect::<Vec<_>>())
                .unwrap_or_default();
            project.answer_by_node_id.insert(
                node.id.clone(),
                build_answer_for_detail(project, &detail, related_node_ids),
            );
        }
    }

    let concept_count = project
        .nodes
        .iter()
        .filter(|node| node.kind == GraphNodeKind::Concept)
        .count();
    let document_count = project
        .nodes
        .iter()
        .filter(|node| is_source_like_node_kind(node.kind))
        .count();
    let relationship_count = project.edges.len();
    let evidence_count = project
        .details_by_node_id
        .values()
        .map(|detail| detail.evidence.len())
        .sum::<usize>()
        + project
            .edge_details_by_id
            .values()
            .map(|detail| detail.evidence.len())
            .sum::<usize>();
    if let Some(document_title) = project
        .nodes
        .iter()
        .find(|node| is_source_like_node_kind(node.kind))
        .map(|node| node.label.clone())
    {
        project.summary.title = document_title;
    }
    project.summary.status = if concept_count > 0 {
        ProjectStatus::Ready
    } else {
        ProjectStatus::Degraded
    };
    project.summary.document_count = document_count;
    project.summary.node_count = project.nodes.len();
    project.summary.relationship_count = relationship_count;
    project.summary.evidence_count = evidence_count;
    project.summary.summary = format!(
        "Workspace contains {} concept nodes and {} explainable relationships. Manual corrections keep the graph grounded in visible evidence.",
        concept_count, relationship_count
    );
}

pub(super) fn rewrite_project_edges(
    project: &mut KnowledgeProject,
    redirect: Option<(&str, &str)>,
) {
    let mut previous_details = std::mem::take(&mut project.edge_details_by_id);
    let existing_edges = std::mem::take(&mut project.edges);
    let mut accumulators = BTreeMap::<String, StoredEdgeAccumulator>::new();
    let source_like_ids = source_like_node_ids(project);

    for edge in existing_edges {
        let mut source_node_id = edge.source_node_id.clone();
        let mut target_node_id = edge.target_node_id.clone();
        if let Some((from, to)) = redirect {
            if source_node_id == from {
                source_node_id = to.to_string();
            }
            if target_node_id == from {
                target_node_id = to.to_string();
            }
        }
        if source_node_id == target_node_id {
            continue;
        }
        if edge.kind == RelationKind::SourceDocument {
            if source_like_ids.contains(&target_node_id) {
                std::mem::swap(&mut source_node_id, &mut target_node_id);
            }
            if !source_like_ids.contains(&source_node_id) {
                continue;
            }
        } else if source_node_id > target_node_id {
            std::mem::swap(&mut source_node_id, &mut target_node_id);
        }

        let edge_id = relation_edge_id(edge.kind, &source_node_id, &target_node_id);
        let evidence = previous_details
            .remove(&edge.id)
            .map(|detail| detail.evidence)
            .unwrap_or_default();
        let accumulator =
            accumulators
                .entry(edge_id.clone())
                .or_insert_with(|| StoredEdgeAccumulator {
                    kind: edge.kind,
                    source_node_id: source_node_id.clone(),
                    target_node_id: target_node_id.clone(),
                    label: normalized_edge_label(edge.kind, &edge.label),
                    confidence: edge.confidence,
                    evidence: Vec::new(),
                });
        accumulator.label = preferred_edge_label(&accumulator.label, &edge.label, edge.kind);
        accumulator.confidence = match (accumulator.confidence, edge.confidence) {
            (Some(left), Some(right)) => Some(left.max(right)),
            (Some(left), None) => Some(left),
            (None, Some(right)) => Some(right),
            (None, None) => None,
        };
        accumulator.evidence = dedupe_evidence(
            accumulator
                .evidence
                .clone()
                .into_iter()
                .chain(evidence)
                .collect(),
        );
    }

    let mut edges = Vec::new();
    let mut edge_details_by_id = BTreeMap::new();
    for (edge_id, accumulator) in accumulators {
        let edge = RelationEdgeSummary {
            id: edge_id.clone(),
            source_node_id: accumulator.source_node_id,
            target_node_id: accumulator.target_node_id,
            kind: accumulator.kind,
            label: accumulator.label,
            confidence: accumulator.confidence,
            evidence_count: accumulator.evidence.len(),
        };
        edge_details_by_id.insert(
            edge_id,
            RelationEdgeDetail {
                edge: edge.clone(),
                explanation: String::new(),
                evidence: accumulator.evidence,
            },
        );
        edges.push(edge);
    }

    project.edges = edges;
    project.edge_details_by_id = edge_details_by_id;
}

pub(super) fn source_like_node_ids(project: &KnowledgeProject) -> BTreeSet<String> {
    project
        .nodes
        .iter()
        .filter(|node| is_source_like_node_kind(node.kind))
        .map(|node| node.id.clone())
        .collect()
}

pub(super) fn source_like_node_ids_for_concept(
    project: &KnowledgeProject,
    concept_node_id: &str,
) -> BTreeSet<String> {
    let linked_source_ids = project
        .edges
        .iter()
        .filter(|edge| edge.kind == RelationKind::SourceDocument)
        .filter_map(|edge| {
            if edge.target_node_id == concept_node_id {
                Some(edge.source_node_id.clone())
            } else if edge.source_node_id == concept_node_id {
                Some(edge.target_node_id.clone())
            } else {
                None
            }
        })
        .filter(|node_id| {
            project
                .nodes
                .iter()
                .any(|node| node.id == *node_id && is_source_like_node_kind(node.kind))
        })
        .collect::<BTreeSet<_>>();

    if linked_source_ids.is_empty() {
        source_like_node_ids(project)
    } else {
        linked_source_ids
    }
}

pub(super) fn build_answer_for_detail(
    project: &KnowledgeProject,
    detail: &GraphNodeDetail,
    related_node_ids: Vec<String>,
) -> AnswerResponse {
    match detail.node.kind {
        GraphNodeKind::Source | GraphNodeKind::Document => {
            let concept_count = project
                .nodes
                .iter()
                .filter(|node| node.kind == GraphNodeKind::Concept)
                .count();
            let concept_relationship_count = project
                .edges
                .iter()
                .filter(|edge| edge.kind == RelationKind::RelatedTo)
                .count();
            AnswerResponse {
                status: if concept_count > 0 {
                    AnswerStatus::Grounded
                } else {
                    AnswerStatus::LowConfidence
                },
                text: Some(format!(
                    "HyprDuck currently tracks {} concept nodes and {} explainable concept links in this workspace.",
                    concept_count, concept_relationship_count
                )),
                explanation:
                    "This document-level answer reflects the current corrected graph and stays grounded in visible evidence.".into(),
                citations: detail.evidence.iter().take(3).cloned().collect(),
                related_node_ids,
                suggested_actions: vec![
                    SuggestedAction {
                        kind: SuggestedActionKind::InspectEvidence,
                        label: "Inspect evidence".into(),
                        description:
                            "Review the cited snippets before trusting the workspace-wide answer."
                                .into(),
                    },
                    SuggestedAction {
                        kind: SuggestedActionKind::AskDifferentQuestion,
                        label: "Ask a narrower question".into(),
                        description:
                            "Grounded answers get stronger when you focus on one concept at a time."
                                .into(),
                    },
                ],
            }
        }
        GraphNodeKind::Concept | GraphNodeKind::Page => {
            let page_count = detail
                .evidence
                .iter()
                .map(|evidence| evidence.page_label.clone())
                .collect::<BTreeSet<_>>()
                .len();
            AnswerResponse {
                status: if detail.evidence.is_empty() {
                    AnswerStatus::LowConfidence
                } else {
                    AnswerStatus::Grounded
                },
                text: Some(format!(
                    "{} currently has {} visible evidence refs across {} page(s).",
                    detail.canonical_name,
                    detail.evidence.len(),
                    page_count
                )),
                explanation:
                    "This answer reflects the current corrected concept node and its visible evidence."
                        .into(),
                citations: detail.evidence.iter().take(3).cloned().collect(),
                related_node_ids,
                suggested_actions: vec![SuggestedAction {
                    kind: SuggestedActionKind::InspectEvidence,
                    label: "Inspect evidence".into(),
                    description:
                        "Use the cited snippets to verify the corrected concept before acting on it."
                            .into(),
                }],
            }
        }
    }
}

pub(super) fn best_matching_evidence(question: &str, detail: &GraphNodeDetail) -> Vec<EvidenceRef> {
    let question_terms = question_terms(question);
    if question_terms.is_empty() {
        return detail.evidence.iter().take(3).cloned().collect();
    }

    let mut scored = detail
        .evidence
        .iter()
        .map(|evidence| {
            let score = overlap_score(&question_terms, &evidence.snippet);
            (score, evidence.clone())
        })
        .collect::<Vec<_>>();
    scored.sort_by(|left, right| {
        right
            .0
            .cmp(&left.0)
            .then_with(|| left.1.page_label.cmp(&right.1.page_label))
    });

    let matched = scored
        .iter()
        .filter(|(score, _)| *score > 0)
        .map(|(_, evidence)| evidence.clone())
        .take(3)
        .collect::<Vec<_>>();
    if matched.is_empty() {
        detail.evidence.iter().take(2).cloned().collect()
    } else {
        matched
    }
}

pub(super) fn answer_text_for_question(
    project: &KnowledgeProject,
    detail: &GraphNodeDetail,
    question: &str,
    status: AnswerStatus,
    citations: &[EvidenceRef],
) -> String {
    let evidence_summary = citations
        .first()
        .map(|citation| citation.snippet.clone())
        .unwrap_or_else(|| "HyprDuck could not find a directly relevant snippet yet.".into());
    let page_count = detail
        .evidence
        .iter()
        .map(|evidence| evidence.page_label.clone())
        .collect::<BTreeSet<_>>()
        .len();

    match detail.node.kind {
        GraphNodeKind::Source | GraphNodeKind::Document => {
            let concept_count = project
                .nodes
                .iter()
                .filter(|node| node.kind == GraphNodeKind::Concept)
                .count();
            match status {
                AnswerStatus::Grounded => format!(
                    "For \"{}\", the strongest grounded reading is that this workspace currently contains {} concept nodes. Best visible support: {}",
                    question, concept_count, evidence_summary
                ),
                AnswerStatus::LowConfidence => format!(
                    "HyprDuck can partially answer \"{}\", but the graph only has weak snippet overlap. Closest visible support: {}",
                    question, evidence_summary
                ),
                AnswerStatus::Blocked | AnswerStatus::Stale => format!(
                    "HyprDuck cannot safely answer \"{}\" from the current workspace yet.",
                    question
                ),
            }
        }
        GraphNodeKind::Concept | GraphNodeKind::Page => match status {
            AnswerStatus::Grounded => format!(
                "For \"{}\", {} is supported by {} visible evidence refs across {} page(s). Best visible support: {}",
                question,
                detail.canonical_name,
                detail.evidence.len(),
                page_count,
                evidence_summary
            ),
            AnswerStatus::LowConfidence => format!(
                "HyprDuck found {} evidence refs for {}, but the question \"{}\" only weakly matches those snippets. Closest visible support: {}",
                detail.evidence.len(),
                detail.canonical_name,
                question,
                evidence_summary
            ),
            AnswerStatus::Blocked | AnswerStatus::Stale => format!(
                "HyprDuck cannot safely answer \"{}\" for {} yet.",
                question, detail.canonical_name
            ),
        },
    }
}

pub(super) fn answer_explanation_for_question(
    detail: &GraphNodeDetail,
    question: &str,
    status: AnswerStatus,
    citations: &[EvidenceRef],
) -> String {
    match status {
        AnswerStatus::Grounded => format!(
            "HyprDuck answered \"{}\" using {} visible citation(s) attached to {}.",
            question,
            citations.len(),
            detail.canonical_name
        ),
        AnswerStatus::LowConfidence => format!(
            "HyprDuck kept this answer cautious because the question \"{}\" only loosely overlaps with the visible evidence on {}.",
            question, detail.canonical_name
        ),
        AnswerStatus::Blocked => format!(
            "HyprDuck blocked this answer because it could not find enough grounded evidence for \"{}\".",
            question
        ),
        AnswerStatus::Stale => "HyprDuck is still reading from a stale workspace snapshot.".into(),
    }
}

pub(super) fn answer_suggested_actions(status: AnswerStatus) -> Vec<SuggestedAction> {
    match status {
        AnswerStatus::Grounded => vec![SuggestedAction {
            kind: SuggestedActionKind::InspectEvidence,
            label: "Inspect evidence".into(),
            description: "Review the cited snippets if you want to verify the grounded answer."
                .into(),
        }],
        AnswerStatus::LowConfidence => vec![
            SuggestedAction {
                kind: SuggestedActionKind::InspectEvidence,
                label: "Inspect evidence".into(),
                description:
                    "Check the cited snippets to see where the question stopped matching strongly."
                        .into(),
            },
            SuggestedAction {
                kind: SuggestedActionKind::AskDifferentQuestion,
                label: "Ask a narrower question".into(),
                description:
                    "Use a concept name, relationship, or page label to get a more grounded answer."
                        .into(),
            },
        ],
        AnswerStatus::Blocked | AnswerStatus::Stale => vec![SuggestedAction {
            kind: SuggestedActionKind::AskDifferentQuestion,
            label: "Ask a narrower question".into(),
            description:
                "HyprDuck needs a more concrete, evidence-seeking question before it can answer."
                    .into(),
        }],
    }
}

pub(super) fn question_terms(question: &str) -> BTreeSet<String> {
    text_terms(question)
}

pub(super) fn text_terms(value: &str) -> BTreeSet<String> {
    value
        .split(|char: char| !char.is_ascii_alphanumeric())
        .map(|term| term.trim().to_ascii_lowercase())
        .filter(|term| term.len() >= 3)
        .collect()
}

pub(super) fn overlap_score(question_terms: &BTreeSet<String>, haystack: &str) -> usize {
    let haystack_terms = haystack
        .split(|char: char| !char.is_ascii_alphanumeric())
        .map(|term| term.trim().to_ascii_lowercase())
        .filter(|term| term.len() >= 3)
        .collect::<BTreeSet<_>>();
    question_terms.intersection(&haystack_terms).count()
}

pub(super) fn edge_explanation(
    edge: &RelationEdgeSummary,
    label_by_node_id: &BTreeMap<String, String>,
    evidence: &[EvidenceRef],
) -> String {
    let source_label = label_by_node_id
        .get(&edge.source_node_id)
        .cloned()
        .unwrap_or_else(|| edge.source_node_id.clone());
    let target_label = label_by_node_id
        .get(&edge.target_node_id)
        .cloned()
        .unwrap_or_else(|| edge.target_node_id.clone());

    match edge.kind {
        RelationKind::SourceDocument => format!(
            "HyprDuck linked the source document to {} because this concept is grounded in cited snippets from the import.",
            target_label
        ),
        RelationKind::RelatedTo if edge.label == "Separated by correction" => format!(
            "HyprDuck keeps {} and {} separate because you explicitly split them during correction review.",
            source_label, target_label
        ),
        RelationKind::RelatedTo => format!(
            "HyprDuck linked {} and {} because they share {} visible evidence ref(s).",
            source_label,
            target_label,
            evidence.len()
        ),
    }
}

pub(super) fn relation_edge_id(
    kind: RelationKind,
    source_node_id: &str,
    target_node_id: &str,
) -> String {
    match kind {
        RelationKind::SourceDocument => format!("edge-{}-{}", source_node_id, target_node_id),
        RelationKind::RelatedTo => format!("edge-{}-{}", source_node_id, target_node_id),
    }
}

pub(super) fn normalized_edge_label(kind: RelationKind, label: &str) -> String {
    match kind {
        RelationKind::SourceDocument => "Compiled from source".into(),
        RelationKind::RelatedTo if label == "Separated by correction" => {
            "Separated by correction".into()
        }
        RelationKind::RelatedTo
            if matches!(
                label,
                "Supports" | "Contradicts" | "Supersedes" | "Same as" | "Depends on"
            ) =>
        {
            label.into()
        }
        RelationKind::RelatedTo => "Related in source".into(),
    }
}

pub(super) fn preferred_edge_label(current: &str, incoming: &str, kind: RelationKind) -> String {
    match kind {
        RelationKind::SourceDocument => "Compiled from source".into(),
        RelationKind::RelatedTo if current == "Separated by correction" => current.into(),
        RelationKind::RelatedTo if incoming == "Separated by correction" => incoming.into(),
        RelationKind::RelatedTo if current != "Related in source" => current.into(),
        RelationKind::RelatedTo
            if matches!(
                incoming,
                "Supports" | "Contradicts" | "Supersedes" | "Same as" | "Depends on"
            ) =>
        {
            incoming.into()
        }
        RelationKind::RelatedTo => "Related in source".into(),
    }
}

pub(super) fn dedupe_evidence(evidence: Vec<EvidenceRef>) -> Vec<EvidenceRef> {
    let mut seen = BTreeSet::new();
    let mut deduped = Vec::new();
    for item in evidence {
        let key = format!(
            "{}|{}|{}|{}",
            item.id,
            item.page_label,
            item.snippet,
            item.source_path.clone().unwrap_or_default()
        );
        if seen.insert(key) {
            deduped.push(item);
        }
    }
    deduped
}

pub(super) fn unique_manual_node_id(project: &KnowledgeProject, label: &str) -> String {
    let base = normalize_key(label);
    let base_id = format!("concept-{base}");
    if !project.nodes.iter().any(|node| node.id == base_id) {
        return base_id;
    }

    let mut suffix = 2usize;
    loop {
        let candidate = format!("concept-{base}-manual-{suffix}");
        if !project.nodes.iter().any(|node| node.id == candidate) {
            return candidate;
        }
        suffix += 1;
    }
}

pub(super) fn manual_split_position(base: &GraphNodePosition, index: usize) -> GraphNodePosition {
    let column = (index % 2) as f32;
    let row = (index / 2) as f32;
    GraphNodePosition {
        x: (base.x + 10.0 + column * 12.0).min(90.0),
        y: (base.y + row * 10.0).min(88.0),
    }
}

pub(super) fn layout_concept_positions(count: usize) -> Vec<GraphNodePosition> {
    let per_row = if count > 9 { 4 } else { 3 };
    let row_count = ((count as f32) / (per_row as f32)).ceil() as usize;
    let row_spacing = if row_count > 1 {
        48.0 / (row_count.saturating_sub(1) as f32)
    } else {
        0.0
    };
    let mut positions = Vec::with_capacity(count);

    for index in 0..count {
        let row = index / per_row;
        let col = index % per_row;
        let columns_in_row = if row == row_count.saturating_sub(1) {
            let remainder = count % per_row;
            if remainder == 0 {
                per_row
            } else {
                remainder
            }
        } else {
            per_row
        };
        let x = if columns_in_row == 1 {
            50.0
        } else {
            18.0 + (64.0 / (columns_in_row.saturating_sub(1) as f32)) * (col as f32)
        };
        let y = 40.0 + row_spacing * (row as f32);
        positions.push(GraphNodePosition { x, y });
    }

    positions
}
