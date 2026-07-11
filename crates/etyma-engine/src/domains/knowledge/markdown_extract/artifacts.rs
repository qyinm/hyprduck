use super::*;

pub(crate) fn page_section_for_candidate<'a>(
    sections: &'a [PageSection],
    candidate: &MarkdownNodeCandidate,
) -> Option<&'a PageSection> {
    sections.iter().find(|section| {
        section.content.contains(&candidate.evidence_snippet)
            || section.content.contains(&candidate.label)
    })
}

pub(crate) fn page_section_for_line(
    sections: &[PageSection],
    _line_start: usize,
) -> Option<&PageSection> {
    sections.first()
}

pub(crate) fn build_extraction_artifact(
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

pub(crate) fn collected_concepts_from_artifact(artifact: &ExtractionArtifact) -> CollectedConcepts {
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

pub(crate) fn evidence_ref_from_extraction(evidence: &ExtractionEvidenceRef) -> EvidenceRef {
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
