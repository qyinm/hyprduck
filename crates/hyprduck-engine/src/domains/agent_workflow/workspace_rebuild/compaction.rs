//! Source graph compaction helpers.

use std::collections::{BTreeMap, BTreeSet};

use anyhow::{bail, Result};

use super::super::validation::validate_provider_source_local_graph_snapshot;
use super::{
    SourceGraphCompactionReport, SOURCE_GRAPH_HARD_MAX_CONCEPTS, SOURCE_GRAPH_HARD_MAX_RELATIONS,
    SOURCE_GRAPH_MAX_CLAIMS, SOURCE_GRAPH_MAX_EVIDENCE_PER_NODE,
    SOURCE_GRAPH_MAX_EVIDENCE_PER_RELATION, SOURCE_GRAPH_TARGET_CONCEPTS,
};
use crate::{
    empty_replayed_brain_snapshot, sanitize_name, unix_timestamp_seconds, BrainNodeKind,
    BrainNodeRecord, BrainRelationKind, BrainRelationRecord, BrainRepoSnapshot, BrainScope,
    ClaimRecord, EvidenceRef,
};

#[cfg(test)]
#[derive(Debug, Clone)]
pub(super) struct SourceGraphCompactionResult {
    pub(super) raw_snapshot: BrainRepoSnapshot,
    pub(super) canonical_snapshot: BrainRepoSnapshot,
    pub(super) report: SourceGraphCompactionReport,
}

#[derive(Debug, Clone)]
struct CanonicalNodeAccumulator {
    canonical_id: String,
    label: String,
    kind: BrainNodeKind,
    aliases: BTreeSet<String>,
    normalized_aliases: BTreeSet<String>,
    evidence_ids: BTreeSet<String>,
    raw_node_ids: BTreeSet<String>,
    confidence: Option<f32>,
    score: i32,
}

#[derive(Debug, Clone)]
struct CanonicalRelationAccumulator {
    relation_id: String,
    kind: BrainRelationKind,
    source_node_id: String,
    target_node_id: String,
    label: String,
    evidence_ids: BTreeSet<String>,
    confidence: Option<f32>,
    score: i32,
}

pub(super) fn compact_source_graph_snapshot(
    workspace_id: &str,
    baseline: &BrainRepoSnapshot,
    source_id: &str,
    raw: &BrainRepoSnapshot,
    stripped_source_of_relation_count: usize,
) -> Result<(BrainRepoSnapshot, SourceGraphCompactionReport)> {
    let source_node_id = format!("source:{source_id}");
    let generated_at = unix_timestamp_seconds();
    let mut drop_reasons = BTreeMap::<String, usize>::new();
    if stripped_source_of_relation_count > 0 {
        drop_reasons.insert(
            "provider_source_of".into(),
            stripped_source_of_relation_count,
        );
    }
    let mut candidate_to_canonical_map = BTreeMap::<String, Option<String>>::new();
    let mut accumulators = BTreeMap::<String, CanonicalNodeAccumulator>::new();
    let evidence_by_id = raw
        .evidence
        .iter()
        .map(|evidence| (evidence.id.as_str(), evidence))
        .collect::<BTreeMap<_, _>>();
    let raw_relation_degree_by_node_id =
        raw.relations
            .iter()
            .fold(BTreeMap::<String, usize>::new(), |mut degree, relation| {
                *degree.entry(relation.source_node_id.clone()).or_default() += 1;
                *degree.entry(relation.target_node_id.clone()).or_default() += 1;
                degree
            });

    for node in raw
        .nodes
        .iter()
        .filter(|node| node.node_id != source_node_id)
    {
        if node.evidence_ids.is_empty() {
            increment_drop_reason(&mut drop_reasons, "missing_evidence");
            candidate_to_canonical_map.insert(node.node_id.clone(), None);
            continue;
        }
        let Some(key) = canonical_concept_keys(node).into_iter().next() else {
            increment_drop_reason(&mut drop_reasons, "empty_label");
            candidate_to_canonical_map.insert(node.node_id.clone(), None);
            continue;
        };
        let canonical_id = format!("concept:{source_id}:{}", sanitize_name(&key));
        let entry = accumulators
            .entry(key.clone())
            .or_insert_with(|| CanonicalNodeAccumulator {
                canonical_id: canonical_id.clone(),
                label: best_readable_label(&node.label, &node.aliases),
                kind: if matches!(node.kind, BrainNodeKind::Concept | BrainNodeKind::Topic) {
                    node.kind
                } else {
                    BrainNodeKind::Concept
                },
                aliases: BTreeSet::new(),
                normalized_aliases: BTreeSet::new(),
                evidence_ids: BTreeSet::new(),
                raw_node_ids: BTreeSet::new(),
                confidence: None,
                score: 0,
            });
        entry.raw_node_ids.insert(node.node_id.clone());
        entry.aliases.insert(node.label.clone());
        entry.aliases.extend(node.aliases.iter().cloned());
        let normalized_label = normalize_concept_label(&node.label);
        if !normalized_label.is_empty() {
            entry.normalized_aliases.insert(normalized_label);
        }
        entry.normalized_aliases.extend(
            node.aliases
                .iter()
                .map(|alias| normalize_concept_label(alias))
                .filter(|alias| !alias.is_empty()),
        );
        entry.evidence_ids.extend(node.evidence_ids.iter().cloned());
        entry.confidence = max_confidence(entry.confidence, node.confidence);
        entry.score += node_salience_score(
            node,
            &key,
            &evidence_by_id,
            raw_relation_degree_by_node_id
                .get(&node.node_id)
                .copied()
                .unwrap_or(0),
        );
        if better_label(&node.label, &entry.label) {
            entry.label = node.label.clone();
        }
    }

    let raw_non_source_node_count = raw
        .nodes
        .iter()
        .filter(|node| node.node_id != source_node_id)
        .count();
    let mut ranked_nodes =
        merge_overlapping_node_accumulators(accumulators.into_values(), source_id);
    let deduped_node_count = ranked_nodes.len();
    ranked_nodes.sort_by(|left, right| {
        right
            .score
            .cmp(&left.score)
            .then(right.evidence_ids.len().cmp(&left.evidence_ids.len()))
            .then(left.label.cmp(&right.label))
            .then(left.canonical_id.cmp(&right.canonical_id))
    });
    let capped_nodes = ranked_nodes
        .into_iter()
        .enumerate()
        .filter_map(|(index, node)| {
            if index < SOURCE_GRAPH_TARGET_CONCEPTS {
                for raw_id in &node.raw_node_ids {
                    candidate_to_canonical_map
                        .insert(raw_id.clone(), Some(node.canonical_id.clone()));
                }
                Some(node)
            } else {
                increment_drop_reason(&mut drop_reasons, "capped_out");
                for raw_id in &node.raw_node_ids {
                    candidate_to_canonical_map.insert(raw_id.clone(), None);
                }
                None
            }
        })
        .collect::<Vec<_>>();

    let kept_node_ids = capped_nodes
        .iter()
        .map(|node| node.canonical_id.clone())
        .collect::<BTreeSet<_>>();
    let canonical_node_score_by_id = capped_nodes
        .iter()
        .map(|node| (node.canonical_id.clone(), node.score))
        .collect::<BTreeMap<_, _>>();
    let raw_to_canonical = candidate_to_canonical_map
        .iter()
        .filter_map(|(raw_id, canonical_id)| {
            canonical_id
                .as_ref()
                .filter(|candidate| kept_node_ids.contains(*candidate))
                .map(|canonical_id| (raw_id.clone(), canonical_id.clone()))
        })
        .collect::<BTreeMap<_, _>>();

    let mut canonical = empty_replayed_brain_snapshot(workspace_id);
    canonical.generated_at = generated_at;
    canonical.sources = baseline
        .sources
        .iter()
        .filter(|source| source.source_id == source_id)
        .cloned()
        .collect();
    canonical.evidence = raw.evidence.clone();
    if let Some(source_node) = raw.nodes.iter().find(|node| node.node_id == source_node_id) {
        let mut source_node = source_node.clone();
        source_node.evidence_ids = canonical
            .evidence
            .iter()
            .map(|evidence| evidence.id.clone())
            .collect();
        canonical.nodes.push(source_node);
    }

    for node in &capped_nodes {
        let evidence_ids = node
            .evidence_ids
            .iter()
            .take(SOURCE_GRAPH_MAX_EVIDENCE_PER_NODE)
            .cloned()
            .collect::<Vec<_>>();
        let aliases = node
            .aliases
            .iter()
            .filter(|alias| alias.trim() != node.label)
            .take(8)
            .cloned()
            .collect::<Vec<_>>();
        canonical.nodes.push(BrainNodeRecord {
            node_id: node.canonical_id.clone(),
            kind: node.kind,
            label: node.label.clone(),
            scope: BrainScope::Project,
            aliases,
            evidence_ids,
            source_ids: vec![source_id.to_string()],
            confidence: node.confidence,
            updated_at: generated_at,
        });
    }

    let relation_semantic_budget =
        SOURCE_GRAPH_HARD_MAX_RELATIONS.saturating_sub(capped_nodes.len());
    let mut relation_accumulators = BTreeMap::<String, CanonicalRelationAccumulator>::new();
    let mut raw_relation_count = 0usize;
    for relation in &raw.relations {
        raw_relation_count += 1;
        if relation.evidence_ids.is_empty() {
            increment_drop_reason(&mut drop_reasons, "relation_missing_evidence");
            continue;
        }
        let Some(source_node_id) = raw_to_canonical.get(&relation.source_node_id).cloned() else {
            increment_drop_reason(&mut drop_reasons, "relation_endpoint_pruned");
            continue;
        };
        let Some(target_node_id) = raw_to_canonical.get(&relation.target_node_id).cloned() else {
            increment_drop_reason(&mut drop_reasons, "relation_endpoint_pruned");
            continue;
        };
        if source_node_id == target_node_id {
            increment_drop_reason(&mut drop_reasons, "relation_self_loop");
            continue;
        }
        let (left, right) = relation_key_endpoints(relation.kind, &source_node_id, &target_node_id);
        let key = format!("{:?}|{}|{}", relation.kind, left, right);
        let relation_id = format!(
            "rel-{}-{}-{}",
            relation_kind_slug(relation.kind),
            sanitize_name(&left),
            sanitize_name(&right)
        );
        let entry =
            relation_accumulators
                .entry(key)
                .or_insert_with(|| CanonicalRelationAccumulator {
                    relation_id,
                    kind: relation.kind,
                    source_node_id: left.clone(),
                    target_node_id: right.clone(),
                    label: relation_label(relation),
                    evidence_ids: BTreeSet::new(),
                    confidence: None,
                    score: 0,
                });
        entry
            .evidence_ids
            .extend(relation.evidence_ids.iter().cloned());
        entry.confidence = max_confidence(entry.confidence, relation.confidence);
        let endpoint_score = canonical_node_score_by_id
            .get(&source_node_id)
            .copied()
            .unwrap_or_default()
            + canonical_node_score_by_id
                .get(&target_node_id)
                .copied()
                .unwrap_or_default();
        entry.score += relation_salience_score(relation) + endpoint_score / 10;
        if better_label(&relation.label, &entry.label) {
            entry.label = relation.label.clone();
        }
    }

    let deduped_relation_count = relation_accumulators.len();
    let mut ranked_relations = relation_accumulators.into_values().collect::<Vec<_>>();
    ranked_relations.sort_by(|left, right| {
        right
            .score
            .cmp(&left.score)
            .then(right.evidence_ids.len().cmp(&left.evidence_ids.len()))
            .then(left.relation_id.cmp(&right.relation_id))
    });
    for relation in ranked_relations.into_iter().take(relation_semantic_budget) {
        canonical.relations.push(BrainRelationRecord {
            relation_id: relation.relation_id,
            kind: relation.kind,
            source_node_id: relation.source_node_id,
            target_node_id: relation.target_node_id,
            label: relation.label,
            evidence_ids: relation
                .evidence_ids
                .into_iter()
                .take(SOURCE_GRAPH_MAX_EVIDENCE_PER_RELATION)
                .collect(),
            confidence: relation.confidence,
            updated_at: generated_at,
        });
    }
    let retained_semantic_relation_count = canonical.relations.len();
    if deduped_relation_count > retained_semantic_relation_count {
        for _ in retained_semantic_relation_count..deduped_relation_count {
            increment_drop_reason(&mut drop_reasons, "relation_capped_out");
        }
    }

    for node in canonical
        .nodes
        .iter()
        .filter(|node| node.node_id != source_node_id)
    {
        canonical.relations.push(BrainRelationRecord {
            relation_id: format!(
                "rel-source_of-{}-{}",
                sanitize_name(source_id),
                sanitize_name(&node.node_id)
            ),
            kind: BrainRelationKind::SourceOf,
            source_node_id: source_node_id.clone(),
            target_node_id: node.node_id.clone(),
            label: "source_of".into(),
            evidence_ids: node
                .evidence_ids
                .iter()
                .take(SOURCE_GRAPH_MAX_EVIDENCE_PER_RELATION)
                .cloned()
                .collect(),
            confidence: Some(1.0),
            updated_at: generated_at,
        });
    }

    canonical.claims = compact_source_claims(raw, source_id, &raw_to_canonical);
    validate_provider_source_local_graph_snapshot(&canonical, source_id)?;
    if canonical.nodes.len().saturating_sub(1) > SOURCE_GRAPH_HARD_MAX_CONCEPTS {
        bail!("canonical source graph exceeds concept cap");
    }
    if canonical.relations.len() > SOURCE_GRAPH_HARD_MAX_RELATIONS {
        bail!("canonical source graph exceeds relation cap");
    }

    let dropped_node_count = raw_non_source_node_count.saturating_sub(capped_nodes.len());
    let dropped_relation_count =
        raw_relation_count.saturating_sub(retained_semantic_relation_count);
    let report = SourceGraphCompactionReport {
        raw_node_count: raw.nodes.len(),
        raw_relation_count,
        deduped_node_count,
        deduped_relation_count,
        materialized_node_count: canonical.nodes.len(),
        materialized_relation_count: canonical.relations.len(),
        dropped_node_count,
        dropped_relation_count,
        drop_reasons,
        candidate_to_canonical_map,
    };
    Ok((canonical, report))
}

fn compact_source_claims(
    raw: &BrainRepoSnapshot,
    source_id: &str,
    raw_to_canonical: &BTreeMap<String, String>,
) -> Vec<ClaimRecord> {
    raw.claims
        .iter()
        .filter_map(|claim| {
            let topic_refs = claim
                .topic_refs
                .iter()
                .filter_map(|topic_ref| raw_to_canonical.get(topic_ref).cloned())
                .collect::<BTreeSet<_>>()
                .into_iter()
                .collect::<Vec<_>>();
            if topic_refs.is_empty() || claim.evidence_refs.is_empty() {
                return None;
            }
            let mut claim = claim.clone();
            claim.topic_refs = topic_refs;
            claim.source_refs = vec![source_id.to_string()];
            Some(claim)
        })
        .take(SOURCE_GRAPH_MAX_CLAIMS)
        .collect()
}

fn increment_drop_reason(drop_reasons: &mut BTreeMap<String, usize>, reason: &str) {
    *drop_reasons.entry(reason.to_string()).or_insert(0) += 1;
}

pub(super) fn strip_source_of_relations(snapshot: &mut BrainRepoSnapshot) -> usize {
    let before = snapshot.relations.len();
    snapshot
        .relations
        .retain(|relation| relation.kind != BrainRelationKind::SourceOf);
    before.saturating_sub(snapshot.relations.len())
}

fn canonical_concept_keys(node: &BrainNodeRecord) -> Vec<String> {
    std::iter::once(node.label.as_str())
        .chain(node.aliases.iter().map(String::as_str))
        .filter_map(|candidate| {
            let normalized = normalize_concept_label(candidate);
            (!normalized.is_empty()).then_some(normalized)
        })
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

fn merge_overlapping_node_accumulators(
    nodes: impl IntoIterator<Item = CanonicalNodeAccumulator>,
    source_id: &str,
) -> Vec<CanonicalNodeAccumulator> {
    let mut nodes = nodes.into_iter().collect::<Vec<_>>();
    let mut changed = true;
    while changed {
        changed = false;
        'outer: for left_index in 0..nodes.len() {
            for right_index in (left_index + 1)..nodes.len() {
                if node_accumulators_overlap(&nodes[left_index], &nodes[right_index]) {
                    let right = nodes.remove(right_index);
                    merge_node_accumulator(&mut nodes[left_index], right, source_id);
                    changed = true;
                    break 'outer;
                }
            }
        }
    }
    nodes
}

fn node_accumulators_overlap(
    left: &CanonicalNodeAccumulator,
    right: &CanonicalNodeAccumulator,
) -> bool {
    left.normalized_aliases
        .iter()
        .any(|alias| right.normalized_aliases.contains(alias))
        || left.evidence_ids.iter().any(|evidence_id| {
            !is_chunk_level_evidence_ref(evidence_id) && right.evidence_ids.contains(evidence_id)
        })
}

fn merge_node_accumulator(
    left: &mut CanonicalNodeAccumulator,
    right: CanonicalNodeAccumulator,
    source_id: &str,
) {
    left.aliases.extend(right.aliases);
    left.normalized_aliases.extend(right.normalized_aliases);
    left.evidence_ids.extend(right.evidence_ids);
    left.raw_node_ids.extend(right.raw_node_ids);
    left.confidence = max_confidence(left.confidence, right.confidence);
    left.score += right.score;
    if better_label(&right.label, &left.label) {
        left.label = right.label;
    }
    let canonical_key = left
        .normalized_aliases
        .iter()
        .cloned()
        .next()
        .unwrap_or_else(|| normalize_concept_label(&left.label));
    left.canonical_id = format!("concept:{source_id}:{}", sanitize_name(&canonical_key));
}

fn is_chunk_level_evidence_ref(evidence_id: &str) -> bool {
    evidence_id.starts_with("retrieved:")
}

pub(super) fn normalize_concept_label(value: &str) -> String {
    let mut normalized = value.trim().to_lowercase();
    for prefix in ["concept:", "topic:", "node:", "concept-", "topic-", "node-"] {
        normalized = normalized
            .strip_prefix(prefix)
            .unwrap_or(&normalized)
            .to_string();
    }
    let mut output = String::new();
    let mut previous_space = false;
    for ch in normalized.chars() {
        if ch.is_ascii_alphanumeric() {
            output.push(ch);
            previous_space = false;
        } else if !previous_space {
            output.push(' ');
            previous_space = true;
        }
    }
    let mut parts = output
        .split_whitespace()
        .map(simple_singular)
        .filter(|part| !part.is_empty() && part != "s")
        .collect::<Vec<_>>();
    if parts.first().is_some_and(|part| part == "the") {
        parts.remove(0);
    }
    parts.join(" ")
}

fn simple_singular(value: &str) -> String {
    if value.len() > 4 && value.ends_with("ies") {
        format!("{}y", &value[..value.len() - 3])
    } else if value.len() > 3 && value.ends_with('s') && !value.ends_with("ss") {
        value[..value.len() - 1].to_string()
    } else {
        value.to_string()
    }
}

fn best_readable_label(label: &str, aliases: &[String]) -> String {
    std::iter::once(label)
        .chain(aliases.iter().map(String::as_str))
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .min_by(|left, right| {
            left.chars()
                .count()
                .cmp(&right.chars().count())
                .then(left.cmp(right))
        })
        .unwrap_or(label.trim())
        .to_string()
}

fn better_label(candidate: &str, current: &str) -> bool {
    let candidate = candidate.trim();
    if candidate.is_empty() {
        return false;
    }
    current.trim().is_empty() || candidate.chars().count() < current.chars().count()
}

fn max_confidence(left: Option<f32>, right: Option<f32>) -> Option<f32> {
    match (left, right) {
        (Some(left), Some(right)) => Some(left.max(right)),
        (Some(value), None) | (None, Some(value)) => Some(value),
        (None, None) => None,
    }
}

fn node_salience_score(
    node: &BrainNodeRecord,
    normalized_key: &str,
    evidence_by_id: &BTreeMap<&str, &EvidenceRef>,
    relation_degree: usize,
) -> i32 {
    let evidence_score = node.evidence_ids.len() as i32 * 5;
    let mut page_refs = BTreeSet::new();
    let mut chunk_refs = BTreeSet::new();
    let mut heading_or_title_refs = 0usize;
    for evidence_id in &node.evidence_ids {
        if is_chunk_level_evidence_ref(evidence_id) {
            chunk_refs.insert(evidence_id.as_str());
        }
        if let Some(evidence) = evidence_by_id.get(evidence_id.as_str()) {
            let page_ref = evidence
                .page_index
                .map(|page_index| format!("index:{page_index}"))
                .unwrap_or_else(|| evidence.page_label.clone());
            page_refs.insert(page_ref);
            if evidence.page_label.trim().chars().any(char::is_alphabetic)
                && !evidence.page_label.starts_with("Lines ")
            {
                heading_or_title_refs += 1;
            }
        }
    }
    let page_score = page_refs.len() as i32 * 3;
    let chunk_score = chunk_refs.len() as i32 * 2;
    let heading_score = heading_or_title_refs.min(3) as i32;
    let relation_score = relation_degree.min(8) as i32 * 2;
    let alias_score = node.aliases.len() as i32;
    let confidence_score = node.confidence.unwrap_or_default().mul_add(4.0, 0.0) as i32;
    let low_information_penalty =
        if normalized_key.split_whitespace().count() <= 1 && node.evidence_ids.len() <= 1 {
            2
        } else {
            0
        };
    let long_label_penalty = (normalized_key.split_whitespace().count() as i32 - 6).max(0);
    evidence_score
        + page_score
        + chunk_score
        + heading_score
        + relation_score
        + alias_score
        + confidence_score
        - low_information_penalty
        - long_label_penalty
}

fn relation_salience_score(relation: &BrainRelationRecord) -> i32 {
    let kind_score = match relation.kind {
        BrainRelationKind::RelatedTo => 1,
        BrainRelationKind::SourceOf => 0,
        _ => 4,
    };
    relation.evidence_ids.len() as i32 * 4
        + kind_score
        + relation.confidence.unwrap_or_default().mul_add(4.0, 0.0) as i32
}

fn relation_key_endpoints(
    kind: BrainRelationKind,
    source_node_id: &str,
    target_node_id: &str,
) -> (String, String) {
    if matches!(
        kind,
        BrainRelationKind::RelatedTo | BrainRelationKind::SameAs
    ) && source_node_id > target_node_id
    {
        (target_node_id.to_string(), source_node_id.to_string())
    } else {
        (source_node_id.to_string(), target_node_id.to_string())
    }
}

fn relation_kind_slug(kind: BrainRelationKind) -> &'static str {
    match kind {
        BrainRelationKind::Mentions => "mentions",
        BrainRelationKind::Supports => "supports",
        BrainRelationKind::Contradicts => "contradicts",
        BrainRelationKind::Supersedes => "supersedes",
        BrainRelationKind::SameAs => "same_as",
        BrainRelationKind::WorksAt => "works_at",
        BrainRelationKind::Founded => "founded",
        BrainRelationKind::InvestedIn => "invested_in",
        BrainRelationKind::Advises => "advises",
        BrainRelationKind::Attended => "attended",
        BrainRelationKind::Owns => "owns",
        BrainRelationKind::ResponsibleFor => "responsible_for",
        BrainRelationKind::Decided => "decided",
        BrainRelationKind::Blocks => "blocks",
        BrainRelationKind::DependsOn => "depends_on",
        BrainRelationKind::SourceOf => "source_of",
        BrainRelationKind::DerivedFrom => "derived_from",
        BrainRelationKind::Cites => "cites",
        BrainRelationKind::LinksTo => "links_to",
        BrainRelationKind::RelatedTo => "related_to",
    }
}

fn relation_label(relation: &BrainRelationRecord) -> String {
    if relation.label.trim().is_empty() {
        relation_kind_slug(relation.kind).to_string()
    } else {
        relation.label.clone()
    }
}
