use super::*;

pub(crate) fn correction_actions_for_detail(
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
        delete_correction_action(),
    ]
}

pub(crate) fn source_node_actions() -> Vec<CorrectionAction> {
    vec![delete_correction_action()]
}

pub(crate) fn delete_correction_action() -> CorrectionAction {
    CorrectionAction {
        kind: CorrectionKind::Delete,
        label: "Delete".into(),
        disabled_reason: None,
    }
}

#[derive(Debug, Clone)]
pub(crate) struct StoredEdgeAccumulator {
    pub(crate) kind: RelationKind,
    pub(crate) source_node_id: String,
    pub(crate) target_node_id: String,
    pub(crate) label: String,
    pub(crate) confidence: Option<f32>,
    pub(crate) evidence: Vec<EvidenceRef>,
}

pub(crate) fn apply_correction(
    project: &mut KnowledgeProject,
    request: &ApplyCorrectionRequest,
) -> Result<()> {
    match request.kind {
        CorrectionKind::Rename => apply_rename_correction(project, request)?,
        CorrectionKind::Merge => apply_merge_correction(project, request)?,
        CorrectionKind::KeepSeparate => apply_keep_separate_correction(project, request)?,
        CorrectionKind::Split => apply_split_correction(project, request)?,
        CorrectionKind::Delete => apply_delete_correction(project, request)?,
    }
    refresh_project_after_correction(project);
    Ok(())
}

pub(crate) fn answer_project(
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
    if project.details_by_node_id.is_empty() {
        return Ok(AnswerResponse {
            status: AnswerStatus::Blocked,
            text: None,
            explanation: "No graph nodes remain in this workspace. Import a source or add knowledge before asking from the graph."
                .into(),
            citations: Vec::new(),
            related_node_ids: Vec::new(),
            suggested_actions: vec![SuggestedAction {
                kind: SuggestedActionKind::AskDifferentQuestion,
                label: "Add graph context".into(),
                description:
                    "Grounded answers need at least one source-backed graph node or accepted graph update."
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

pub(crate) fn select_focal_node_id(
    project: &KnowledgeProject,
    request: &AnswerProjectRequest,
    question: &str,
) -> Result<String> {
    if let Some(node_id) = request.node_id.as_deref() {
        if project.details_by_node_id.contains_key(node_id) {
            return Ok(node_id.to_string());
        }
        if project.summary.project_id.starts_with("workspace:") {
            if let Some(node_id) = best_matching_detail_node_id(project, question) {
                return Ok(node_id);
            }
            if let Some(node_id) = project
                .nodes
                .iter()
                .find(|node| is_source_like_node_kind(node.kind))
                .map(|node| node.id.clone())
            {
                return Ok(node_id);
            }
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

pub(crate) fn best_matching_detail_node_id(
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

pub(crate) fn apply_rename_correction(
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

pub(crate) fn unique_renamed_node_id(
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

pub(crate) fn apply_merge_correction(
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

pub(crate) fn apply_keep_separate_correction(
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
pub(crate) struct SplitReplacementMapping {
    #[serde(default)]
    pub(crate) replacement_node_id: Option<String>,
    pub(crate) replacement_label: String,
    #[serde(default)]
    pub(crate) aliases: Vec<String>,
    pub(crate) evidence_ids: Vec<String>,
    #[serde(default)]
    pub(crate) edge_ids: Vec<String>,
}

pub(crate) fn apply_split_correction(
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

pub(crate) fn apply_delete_correction(
    project: &mut KnowledgeProject,
    request: &ApplyCorrectionRequest,
) -> Result<()> {
    let selected_node = project
        .nodes
        .iter()
        .find(|node| node.id == request.node_id)
        .cloned()
        .ok_or_else(|| anyhow!("node {} was not found", request.node_id))?;

    let mut node_ids_to_remove = BTreeSet::from([request.node_id.clone()]);
    if is_source_like_node_kind(selected_node.kind) {
        let deleted_source_ids = source_ids_for_deleted_source_node(project, &request.node_id);
        let linked_derived_node_ids = project
            .edges
            .iter()
            .filter(|edge| edge.kind == RelationKind::SourceDocument)
            .filter_map(|edge| {
                if edge.source_node_id == request.node_id {
                    Some(edge.target_node_id.clone())
                } else if edge.target_node_id == request.node_id {
                    Some(edge.source_node_id.clone())
                } else {
                    None
                }
            })
            .collect::<BTreeSet<_>>();

        for detail in project.details_by_node_id.values() {
            if is_source_like_node_kind(detail.node.kind) {
                continue;
            }
            let evidence_source_ids = detail
                .evidence
                .iter()
                .filter_map(|evidence| evidence.source_id.clone())
                .collect::<BTreeSet<_>>();
            let only_deleted_source_evidence = !evidence_source_ids.is_empty()
                && !deleted_source_ids.is_empty()
                && evidence_source_ids.is_subset(&deleted_source_ids);
            if linked_derived_node_ids.contains(&detail.node.id) || only_deleted_source_evidence {
                node_ids_to_remove.insert(detail.node.id.clone());
            }
        }
    }

    remove_project_nodes(project, &node_ids_to_remove);
    Ok(())
}

pub(crate) fn source_ids_for_deleted_source_node(
    project: &KnowledgeProject,
    node_id: &str,
) -> BTreeSet<String> {
    let mut source_ids = BTreeSet::new();
    if let Some(source_id) = node_id.strip_prefix("source:") {
        source_ids.insert(source_id.to_string());
    }
    if let Some(detail) = project.details_by_node_id.get(node_id) {
        if let Some(source) = &detail.source {
            source_ids.insert(source.source_id.clone());
        }
        for evidence in &detail.evidence {
            if let Some(source_id) = evidence.source_id.as_deref() {
                source_ids.insert(source_id.to_string());
            }
        }
    }
    source_ids
}

pub(crate) fn apply_workspace_delete_corrections_to_aggregate(
    project: &mut KnowledgeProject,
    corrections: &[WorkspaceCorrection],
) {
    for correction in corrections {
        if correction.kind != CorrectionKind::Delete {
            continue;
        }
        remove_project_nodes(
            project,
            &workspace_delete_node_ids_for_correction(correction),
        );
    }
}

pub(crate) fn workspace_delete_node_ids_for_correction(
    correction: &WorkspaceCorrection,
) -> BTreeSet<String> {
    let mut node_ids = BTreeSet::from([correction.aggregate_node_id.clone()]);
    if let Some(source_id) = correction.aggregate_node_id.strip_prefix("source:") {
        node_ids.insert(source_node_id(source_id));
    }
    for source_node_ref in &correction.source_node_ids {
        if let Some((_, node_id)) = source_node_ref.split_once(':') {
            if !node_id.is_empty() {
                node_ids.insert(node_id.to_string());
            }
        }
    }
    node_ids
}

pub(crate) fn remove_project_nodes(project: &mut KnowledgeProject, node_ids: &BTreeSet<String>) {
    project.nodes.retain(|node| !node_ids.contains(&node.id));
    project
        .details_by_node_id
        .retain(|node_id, _| !node_ids.contains(node_id));
    project
        .answer_by_node_id
        .retain(|node_id, _| !node_ids.contains(node_id));
    project.edges.retain(|edge| {
        !node_ids.contains(&edge.source_node_id) && !node_ids.contains(&edge.target_node_id)
    });
    project.edge_details_by_id.retain(|_, detail| {
        !node_ids.contains(&detail.edge.source_node_id)
            && !node_ids.contains(&detail.edge.target_node_id)
    });
}

pub(crate) fn parse_split_replacement_mappings(
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

pub(crate) fn split_mapping_matches_edge(
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

pub(crate) fn refresh_project_after_correction(project: &mut KnowledgeProject) {
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
            } else if is_source_like_node_kind(node.kind) {
                detail.actions = source_node_actions();
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
    project.summary.hidden_concept_count = 0;
    project.summary.hidden_relation_count = 0;
    project.summary.summary = format!(
        "Workspace contains {} concept nodes and {} explainable relationships. Manual corrections keep the graph grounded in visible evidence.",
        concept_count, relationship_count
    );
}

pub(crate) fn rewrite_project_edges(
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

pub(crate) fn source_like_node_ids(project: &KnowledgeProject) -> BTreeSet<String> {
    project
        .nodes
        .iter()
        .filter(|node| is_source_like_node_kind(node.kind))
        .map(|node| node.id.clone())
        .collect()
}
