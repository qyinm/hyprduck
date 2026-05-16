use super::*;

pub(crate) fn source_like_node_ids_for_concept(
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

pub(crate) fn build_answer_for_detail(
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

pub(crate) fn best_matching_evidence(question: &str, detail: &GraphNodeDetail) -> Vec<EvidenceRef> {
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

pub(crate) fn answer_text_for_question(
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

pub(crate) fn answer_explanation_for_question(
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

pub(crate) fn answer_suggested_actions(status: AnswerStatus) -> Vec<SuggestedAction> {
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

pub(crate) fn question_terms(question: &str) -> BTreeSet<String> {
    text_terms(question)
}

pub(crate) fn text_terms(value: &str) -> BTreeSet<String> {
    value
        .split(|char: char| !char.is_ascii_alphanumeric())
        .map(|term| term.trim().to_ascii_lowercase())
        .filter(|term| term.len() >= 3)
        .collect()
}

pub(crate) fn overlap_score(question_terms: &BTreeSet<String>, haystack: &str) -> usize {
    let haystack_terms = haystack
        .split(|char: char| !char.is_ascii_alphanumeric())
        .map(|term| term.trim().to_ascii_lowercase())
        .filter(|term| term.len() >= 3)
        .collect::<BTreeSet<_>>();
    question_terms.intersection(&haystack_terms).count()
}

pub(crate) fn edge_explanation(
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

pub(crate) fn relation_edge_id(
    kind: RelationKind,
    source_node_id: &str,
    target_node_id: &str,
) -> String {
    match kind {
        RelationKind::SourceDocument => format!("edge-{}-{}", source_node_id, target_node_id),
        RelationKind::RelatedTo => format!("edge-{}-{}", source_node_id, target_node_id),
    }
}

pub(crate) fn normalized_edge_label(kind: RelationKind, label: &str) -> String {
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

pub(crate) fn preferred_edge_label(current: &str, incoming: &str, kind: RelationKind) -> String {
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

pub(crate) fn dedupe_evidence(evidence: Vec<EvidenceRef>) -> Vec<EvidenceRef> {
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

pub(crate) fn unique_manual_node_id(project: &KnowledgeProject, label: &str) -> String {
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

pub(crate) fn manual_split_position(base: &GraphNodePosition, index: usize) -> GraphNodePosition {
    let column = (index % 2) as f32;
    let row = (index / 2) as f32;
    GraphNodePosition {
        x: (base.x + 10.0 + column * 12.0).min(90.0),
        y: (base.y + row * 10.0).min(88.0),
    }
}

pub(crate) fn layout_concept_positions(count: usize) -> Vec<GraphNodePosition> {
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
