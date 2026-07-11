use super::*;

pub(crate) fn extract_markdown_relationship_evidence(
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

#[derive(Debug, Clone)]
pub(crate) struct RelationshipMention {
    pub(crate) label: String,
    pub(crate) position: usize,
    pub(crate) matched_node_id: Option<String>,
    pub(crate) resolved_node_id: String,
    pub(crate) endpoint_resolution: String,
}

pub(crate) fn relationship_mentions_in_line(
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

pub(crate) fn infer_markdown_relation_kind(line: &str) -> Option<BrainRelationKind> {
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

pub(crate) fn markdown_relation_label(kind: BrainRelationKind) -> String {
    match kind {
        BrainRelationKind::Supports => "Supports".into(),
        BrainRelationKind::Contradicts => "Contradicts".into(),
        BrainRelationKind::Supersedes => "Supersedes".into(),
        BrainRelationKind::SameAs => "Same as".into(),
        BrainRelationKind::DependsOn => "Depends on".into(),
        _ => "Related in source".into(),
    }
}

pub(crate) fn line_has_explicit_link_signal(line: &str) -> bool {
    line.contains("[[") || line.contains("](") || line.contains("->") || line.contains("<->")
}

pub(crate) fn relationship_reason(line: &str, relation_kind: Option<BrainRelationKind>) -> String {
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

pub(crate) type RelationEdgeBuildResult = (
    Vec<RelationEdgeSummary>,
    BTreeMap<String, RelationEdgeDetail>,
    BTreeMap<String, usize>,
    BTreeMap<String, BTreeSet<String>>,
);

pub(crate) fn build_relation_edges(
    document_node: &GraphNodeSummary,
    concept_accumulators: &[ConceptAccumulator],
    page_concepts: &[PageConceptSet],
    relation_candidates: &[RelationCandidateAccumulator],
    source_path: &str,
    source_id: Option<&str>,
) -> RelationEdgeBuildResult {
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
                    "Etyma linked the source document to {} because this concept was compiled from cited snippets in the import.",
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
                    "Etyma linked {} and {} because they appeared together in {} page section(s).",
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

pub(crate) fn note_relation(
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
