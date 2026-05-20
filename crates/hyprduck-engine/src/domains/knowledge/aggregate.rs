use super::*;

pub(crate) const SOURCE_UI_VISIBLE_CONCEPT_LIMIT: usize = 16;
pub(crate) const SOURCE_UI_VISIBLE_RELATION_LIMIT: usize = 24;
pub(crate) const WORKSPACE_UI_VISIBLE_CONCEPT_LIMIT: usize = 60;
pub(crate) const WORKSPACE_UI_VISIBLE_RELATION_LIMIT: usize = 90;

pub(crate) fn aggregate_workspace_project(
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
                actions: source_node_actions(),
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
            accumulator.evidence.extend(evidence);
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
                aliases: aliases.clone(),
                description: format!(
                    "Workspace concept compiled from {} evidence refs across {} source(s).",
                    accumulator.evidence.len(),
                    source_ids.len()
                ),
                evidence: accumulator.evidence.clone(),
                actions: correction_actions_for_detail(&accumulator.canonical_name, &aliases),
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

pub(crate) fn merge_workspace_concept_groups(
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
            merged_accumulator.aliases.extend(stale_accumulator.aliases);
            merged_accumulator
                .evidence
                .extend(stale_accumulator.evidence);
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

pub(crate) fn remap_workspace_edge_accumulators(
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
        existing.evidence.extend(accumulator.evidence);
    }
    for accumulator in remapped.values_mut() {
        accumulator.evidence = dedupe_evidence(std::mem::take(&mut accumulator.evidence));
    }
    remapped
}

pub(crate) fn remap_workspace_concept_node_id(
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

pub(crate) fn finalize_workspace_project(
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
            hidden_concept_count: 0,
            hidden_relation_count: 0,
        },
        nodes,
        edges,
        details_by_node_id,
        edge_details_by_id,
        answer_by_node_id,
    }
}

pub(crate) fn workspace_project_id(workspace_id: &str) -> String {
    format!("workspace:{workspace_id}")
}

pub(crate) fn workspace_id_from_project_id(project_id: &str) -> Option<&str> {
    project_id
        .strip_prefix("workspace:")
        .filter(|workspace_id| !workspace_id.trim().is_empty())
}

pub(crate) fn matching_source_concept_node_ids(
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

pub(crate) fn source_ui_graph_projection(project: KnowledgeProject) -> KnowledgeProject {
    ui_graph_projection(
        project,
        SOURCE_UI_VISIBLE_CONCEPT_LIMIT,
        SOURCE_UI_VISIBLE_RELATION_LIMIT,
    )
}

pub(crate) fn workspace_ui_graph_projection(project: KnowledgeProject) -> KnowledgeProject {
    ui_graph_projection(
        project,
        WORKSPACE_UI_VISIBLE_CONCEPT_LIMIT,
        WORKSPACE_UI_VISIBLE_RELATION_LIMIT,
    )
}

pub(crate) fn ui_graph_projection(
    mut project: KnowledgeProject,
    visible_concept_limit: usize,
    visible_relation_limit: usize,
) -> KnowledgeProject {
    let original_concept_count = project
        .nodes
        .iter()
        .filter(|node| node.kind == GraphNodeKind::Concept)
        .count();
    let original_relation_count = project.edges.len();
    let related_count_by_node_id =
        project
            .edges
            .iter()
            .fold(BTreeMap::<String, usize>::new(), |mut counts, edge| {
                *counts.entry(edge.source_node_id.clone()).or_default() += 1;
                *counts.entry(edge.target_node_id.clone()).or_default() += 1;
                counts
            });

    let mut ranked_concepts = project
        .nodes
        .iter()
        .filter(|node| node.kind == GraphNodeKind::Concept)
        .map(|node| {
            (
                ui_node_salience_score(node, &related_count_by_node_id),
                node.evidence_count,
                node.label.clone(),
                node.id.clone(),
            )
        })
        .collect::<Vec<_>>();
    ranked_concepts.sort_by(|left, right| {
        right
            .0
            .cmp(&left.0)
            .then(right.1.cmp(&left.1))
            .then(left.2.cmp(&right.2))
            .then(left.3.cmp(&right.3))
    });
    let visible_concept_ids = ranked_concepts
        .into_iter()
        .take(visible_concept_limit)
        .map(|(_, _, _, node_id)| node_id)
        .collect::<BTreeSet<_>>();
    let visible_node_ids = project
        .nodes
        .iter()
        .filter(|node| {
            node.kind != GraphNodeKind::Concept || visible_concept_ids.contains(&node.id)
        })
        .map(|node| node.id.clone())
        .collect::<BTreeSet<_>>();

    project
        .nodes
        .retain(|node| visible_node_ids.contains(&node.id));
    project
        .details_by_node_id
        .retain(|node_id, _| visible_node_ids.contains(node_id));
    project
        .answer_by_node_id
        .retain(|node_id, _| visible_node_ids.contains(node_id));

    let mut visible_edges = project
        .edges
        .into_iter()
        .filter(|edge| {
            visible_node_ids.contains(&edge.source_node_id)
                && visible_node_ids.contains(&edge.target_node_id)
        })
        .collect::<Vec<_>>();
    visible_edges.sort_by(|left, right| {
        ui_relation_salience_score(right)
            .cmp(&ui_relation_salience_score(left))
            .then(right.evidence_count.cmp(&left.evidence_count))
            .then(left.label.cmp(&right.label))
            .then(left.id.cmp(&right.id))
    });
    visible_edges.truncate(visible_relation_limit);
    let visible_edge_ids = visible_edges
        .iter()
        .map(|edge| edge.id.clone())
        .collect::<BTreeSet<_>>();
    project.edges = visible_edges;
    project
        .edge_details_by_id
        .retain(|edge_id, _| visible_edge_ids.contains(edge_id));

    refresh_project_projection_links(&mut project);
    let visible_concept_count = project
        .nodes
        .iter()
        .filter(|node| node.kind == GraphNodeKind::Concept)
        .count();
    let hidden_concept_count = original_concept_count.saturating_sub(visible_concept_count);
    let hidden_relation_count = original_relation_count.saturating_sub(project.edges.len());
    project.summary.node_count = project.nodes.len();
    project.summary.relationship_count = project.edges.len();
    project.summary.hidden_concept_count = hidden_concept_count;
    project.summary.hidden_relation_count = hidden_relation_count;
    if hidden_concept_count > 0 || hidden_relation_count > 0 {
        project.summary.summary = format!(
            "{} Default projection shows {} visible concept nodes and {} visible relationships; {} concept nodes and {} relationships are hidden.",
            project.summary.summary,
            visible_concept_count,
            project.edges.len(),
            hidden_concept_count,
            hidden_relation_count
        );
    }
    project
}

fn ui_node_salience_score(
    node: &GraphNodeSummary,
    related_count_by_node_id: &BTreeMap<String, usize>,
) -> i32 {
    let confidence_score = node
        .confidence
        .map(|confidence| (confidence.clamp(0.0, 1.0) * 100.0).round() as i32)
        .unwrap_or(0);
    let label_penalty = if node.label.chars().count() > 80 {
        50
    } else {
        0
    };
    (node.evidence_count as i32 * 100)
        + (related_count_by_node_id.get(&node.id).copied().unwrap_or(0) as i32 * 12)
        + confidence_score
        - label_penalty
}

fn ui_relation_salience_score(edge: &RelationEdgeSummary) -> i32 {
    let kind_score = match edge.kind {
        RelationKind::SourceDocument => 20,
        RelationKind::RelatedTo => 10,
    };
    let confidence_score = edge
        .confidence
        .map(|confidence| (confidence.clamp(0.0, 1.0) * 100.0).round() as i32)
        .unwrap_or(0);
    (edge.evidence_count as i32 * 100) + kind_score + confidence_score
}

fn refresh_project_projection_links(project: &mut KnowledgeProject) {
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
        node.related_count = related_count_by_node_id.get(&node.id).copied().unwrap_or(0);
        if let Some(detail) = project.details_by_node_id.get_mut(&node.id) {
            detail.node = node.clone();
        }
        if let Some(answer) = project.answer_by_node_id.get_mut(&node.id) {
            answer.related_node_ids = connected_node_ids_by_node_id
                .get(&node.id)
                .map(|related| related.iter().cloned().collect())
                .unwrap_or_default();
        }
    }

    let edge_by_id = project
        .edges
        .iter()
        .map(|edge| (edge.id.clone(), edge.clone()))
        .collect::<BTreeMap<_, _>>();
    let label_by_node_id = project
        .nodes
        .iter()
        .map(|node| (node.id.clone(), node.label.clone()))
        .collect::<BTreeMap<_, _>>();
    for (edge_id, detail) in &mut project.edge_details_by_id {
        if let Some(edge) = edge_by_id.get(edge_id) {
            detail.edge = edge.clone();
            detail.explanation = edge_explanation(edge, &label_by_node_id, &detail.evidence);
        }
    }
}

pub(crate) fn source_label_from_summary(summary: &SourceSummary) -> String {
    Path::new(&summary.original_path)
        .file_name()
        .or_else(|| Path::new(&summary.source_path).file_name())
        .map(|value| value.to_string_lossy().into_owned())
        .unwrap_or_else(|| summary.source_id.clone())
}

pub(crate) fn source_backing_from_summary(
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
