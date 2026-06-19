use std::collections::{BTreeMap, BTreeSet};

use crate::search_context::search_terms;
use crate::*;

const DIRECT_LINK_MIN_CONFIDENCE: f32 = 0.75;
const INFERRED_LINK_MIN_CONFIDENCE: f32 = 0.85;
const MAX_WORKSPACE_LINKING_CANDIDATES: usize = 32;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum WorkspaceLinkStrength {
    Direct,
    Inferred,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct WorkspaceLinkCandidate {
    pub(crate) candidate_id: String,
    pub(crate) source_node_id: String,
    pub(crate) target_node_id: String,
    pub(crate) strength: WorkspaceLinkStrength,
    pub(crate) signal: String,
    pub(crate) shared_terms: Vec<String>,
}

pub(crate) fn workspace_linking_candidates(
    snapshot: &BrainRepoSnapshot,
    imported_source_id: &str,
) -> Vec<WorkspaceLinkCandidate> {
    let evidence_by_id = snapshot
        .evidence
        .iter()
        .map(|evidence| (evidence.id.as_str(), evidence))
        .collect::<BTreeMap<_, _>>();
    let imported_nodes = snapshot
        .nodes
        .iter()
        .filter(|node| workspace_linking_node_is_source_only(node, imported_source_id, true))
        .collect::<Vec<_>>();
    let other_nodes = snapshot
        .nodes
        .iter()
        .filter(|node| workspace_linking_node_is_source_only(node, imported_source_id, false))
        .collect::<Vec<_>>();

    let mut candidates = Vec::new();
    for imported in imported_nodes {
        for other in &other_nodes {
            if let Some(candidate) =
                build_workspace_link_candidate(imported, other, imported_source_id, &evidence_by_id)
            {
                candidates.push(candidate);
            }
        }
    }
    candidates.sort_by(|left, right| {
        workspace_link_candidate_score(right)
            .cmp(&workspace_link_candidate_score(left))
            .then(left.candidate_id.cmp(&right.candidate_id))
    });
    candidates.truncate(MAX_WORKSPACE_LINKING_CANDIDATES);
    candidates
}

pub(crate) fn workspace_linking_candidate_summary(
    snapshot: &BrainRepoSnapshot,
    imported_source_id: &str,
) -> String {
    let candidates = workspace_linking_candidates(snapshot, imported_source_id);
    if candidates.is_empty() {
        return "(none)".into();
    }
    candidates
        .iter()
        .map(|candidate| {
            format!(
                "- candidateId: {}; sourceNodeId: {}; targetNodeId: {}; strength: {}; signal: {}; sharedTerms: {}",
                candidate.candidate_id,
                candidate.source_node_id,
                candidate.target_node_id,
                match candidate.strength {
                    WorkspaceLinkStrength::Direct => "direct",
                    WorkspaceLinkStrength::Inferred => "inferred",
                },
                candidate.signal,
                if candidate.shared_terms.is_empty() {
                    "none".into()
                } else {
                    candidate.shared_terms.join(", ")
                }
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

pub(crate) fn workspace_linking_relation_is_verified(
    relation: &BrainRelationRecord,
    snapshot: &BrainRepoSnapshot,
    imported_source_id: &str,
) -> bool {
    let Some(candidate) = workspace_linking_candidates(snapshot, imported_source_id)
        .into_iter()
        .find(|candidate| workspace_link_candidate_matches_relation(&candidate, relation))
    else {
        return false;
    };
    if relation_has_blocked_generic_label(relation, &candidate) {
        return false;
    }
    let confidence = relation.confidence.unwrap_or_default();
    match candidate.strength {
        WorkspaceLinkStrength::Direct => confidence >= DIRECT_LINK_MIN_CONFIDENCE,
        WorkspaceLinkStrength::Inferred => confidence >= INFERRED_LINK_MIN_CONFIDENCE,
    }
}

pub(crate) fn unverified_workspace_linking_relation_ids(
    snapshot: &BrainRepoSnapshot,
    imported_source_id: &str,
) -> Vec<String> {
    snapshot
        .relations
        .iter()
        .filter(|relation| {
            relation.valid_to.is_none()
                && relation.kind != BrainRelationKind::SourceOf
                && relation_touches_imported_source(relation, snapshot, imported_source_id)
                && !workspace_linking_relation_is_verified(relation, snapshot, imported_source_id)
        })
        .map(|relation| relation.relation_id.clone())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

fn build_workspace_link_candidate(
    imported: &BrainNodeRecord,
    other: &BrainNodeRecord,
    imported_source_id: &str,
    evidence_by_id: &BTreeMap<&str, &EvidenceRef>,
) -> Option<WorkspaceLinkCandidate> {
    let imported_names = node_names(imported);
    let other_names = node_names(other);
    let imported_name_set = imported_names
        .iter()
        .map(|name| normalize_link_phrase(name))
        .filter(|name| !name.is_empty())
        .collect::<BTreeSet<_>>();
    let other_name_set = other_names
        .iter()
        .map(|name| normalize_link_phrase(name))
        .filter(|name| !name.is_empty())
        .collect::<BTreeSet<_>>();
    let exact_name = imported_name_set
        .intersection(&other_name_set)
        .next()
        .cloned();

    let imported_text = node_support_text(imported, evidence_by_id);
    let other_text = node_support_text(other, evidence_by_id);
    let imported_label_in_other_text = imported_name_set
        .iter()
        .any(|name| phrase_occurs_in_text(name, &other_text));
    let other_label_in_imported_text = other_name_set
        .iter()
        .any(|name| phrase_occurs_in_text(name, &imported_text));
    let imported_terms = significant_terms(&imported_text);
    let other_terms = significant_terms(&other_text);
    let shared_terms = imported_terms
        .intersection(&other_terms)
        .take(8)
        .cloned()
        .collect::<Vec<_>>();

    let (strength, signal) = if let Some(name) = exact_name {
        (
            WorkspaceLinkStrength::Direct,
            format!("shared label `{name}`"),
        )
    } else if imported_label_in_other_text || other_label_in_imported_text {
        (
            WorkspaceLinkStrength::Direct,
            "endpoint label appears in opposite source evidence".into(),
        )
    } else if shared_terms.len() >= 2 {
        (
            WorkspaceLinkStrength::Inferred,
            format!("shared technical terms: {}", shared_terms.join(", ")),
        )
    } else {
        return None;
    };

    Some(WorkspaceLinkCandidate {
        candidate_id: format!(
            "candidate-{}-{}-{}",
            imported_source_id,
            sanitize_link_id(&imported.node_id),
            sanitize_link_id(&other.node_id)
        ),
        source_node_id: imported.node_id.clone(),
        target_node_id: other.node_id.clone(),
        strength,
        signal,
        shared_terms,
    })
}

fn workspace_link_candidate_score(candidate: &WorkspaceLinkCandidate) -> usize {
    (match candidate.strength {
        WorkspaceLinkStrength::Direct => 10_000,
        WorkspaceLinkStrength::Inferred => 1_000,
    }) + candidate.shared_terms.len() * 100
        + candidate.signal.len().min(99)
}

fn workspace_link_candidate_matches_relation(
    candidate: &WorkspaceLinkCandidate,
    relation: &BrainRelationRecord,
) -> bool {
    if relation.source_node_id == candidate.source_node_id
        && relation.target_node_id == candidate.target_node_id
    {
        return true;
    }
    matches!(
        relation.kind,
        BrainRelationKind::RelatedTo | BrainRelationKind::SameAs
    ) && relation.source_node_id == candidate.target_node_id
        && relation.target_node_id == candidate.source_node_id
}

fn relation_has_blocked_generic_label(
    relation: &BrainRelationRecord,
    candidate: &WorkspaceLinkCandidate,
) -> bool {
    let label = normalize_link_phrase(&relation.label);
    let allowed_direct_same_as = matches!(candidate.strength, WorkspaceLinkStrength::Direct)
        && matches!(relation.kind, BrainRelationKind::SameAs)
        && matches!(label.as_str(), "same as" | "same_as" | "equivalent to");
    let generic = matches!(
        label.as_str(),
        "" | "related to"
            | "related_to"
            | "uses indirectly"
            | "use indirectly"
            | "contrasts with"
            | "contrast with"
            | "connects"
            | "connects to"
            | "connected to"
    );
    generic && !allowed_direct_same_as
}

fn relation_touches_imported_source(
    relation: &BrainRelationRecord,
    snapshot: &BrainRepoSnapshot,
    imported_source_id: &str,
) -> bool {
    let node_sources = snapshot
        .nodes
        .iter()
        .map(|node| (node.node_id.as_str(), node.source_ids.as_slice()))
        .collect::<BTreeMap<_, _>>();
    [&relation.source_node_id, &relation.target_node_id]
        .iter()
        .filter_map(|node_id| node_sources.get(node_id.as_str()))
        .any(|source_ids| {
            source_ids
                .iter()
                .any(|source_id| source_id == imported_source_id)
        })
}

fn workspace_linking_node_is_source_only(
    node: &BrainNodeRecord,
    imported_source_id: &str,
    imported: bool,
) -> bool {
    if node.kind == BrainNodeKind::Source || node.valid_to.is_some() {
        return false;
    }
    let has_imported = node
        .source_ids
        .iter()
        .any(|source_id| source_id == imported_source_id);
    let has_other = node
        .source_ids
        .iter()
        .any(|source_id| source_id != imported_source_id);
    if imported {
        has_imported && !has_other
    } else {
        has_other && !has_imported
    }
}

fn node_names(node: &BrainNodeRecord) -> Vec<String> {
    std::iter::once(node.label.clone())
        .chain(node.aliases.iter().cloned())
        .collect()
}

fn node_support_text(
    node: &BrainNodeRecord,
    evidence_by_id: &BTreeMap<&str, &EvidenceRef>,
) -> String {
    let mut parts = node_names(node);
    parts.extend(
        node.evidence_ids
            .iter()
            .filter_map(|evidence_id| evidence_by_id.get(evidence_id.as_str()))
            .map(|evidence| evidence.snippet.clone()),
    );
    parts.join("\n")
}

fn significant_terms(text: &str) -> BTreeSet<String> {
    search_terms(text)
        .into_iter()
        .filter(|term| term.len() >= 3 && !is_generic_link_term(term))
        .collect()
}

fn is_generic_link_term(term: &str) -> bool {
    matches!(
        term,
        "and"
            | "are"
            | "for"
            | "from"
            | "has"
            | "have"
            | "into"
            | "that"
            | "the"
            | "this"
            | "with"
            | "document"
            | "documents"
            | "parser"
            | "parsing"
            | "method"
            | "methods"
            | "operation"
            | "operations"
            | "system"
            | "systems"
            | "result"
            | "results"
            | "source"
            | "sources"
            | "page"
            | "pages"
            | "text"
            | "score"
            | "scores"
            | "table"
            | "tables"
    )
}

fn phrase_occurs_in_text(phrase: &str, text: &str) -> bool {
    if phrase.len() < 4 {
        return false;
    }
    normalize_link_phrase(text).contains(phrase)
}

fn normalize_link_phrase(value: &str) -> String {
    value
        .to_ascii_lowercase()
        .split(|char: char| !(char.is_ascii_alphanumeric() || char == '_'))
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
}

fn sanitize_link_id(value: &str) -> String {
    let mut output = value
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() {
                ch.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect::<String>();
    while output.contains("--") {
        output = output.replace("--", "-");
    }
    output.trim_matches('-').chars().take(80).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn candidate_builder_rejects_docmate_hash_table_false_positive() {
        let snapshot = BrainRepoSnapshot {
            workspace_id: "default".into(),
            generated_at: 1,
            sources: vec![],
            evidence: vec![
                EvidenceRef {
                    id: "ev-paper".into(),
                    page_label: "Paper".into(),
                    page_index: Some(0),
                    snippet: "DocMate analyzes PDF parsing quality with MinerU and rasterized fallback for hidden text layer corruption.".into(),
                    source_path: None,
                    source_id: Some("source-paper".into()),
                    markdown_path: None,
                    image_path: None,
                    provenance: None,
                },
                EvidenceRef {
                    id: "ev-hash".into(),
                    page_label: "Hashing".into(),
                    page_index: Some(0),
                    snippet: "Hash tables store identifiers using hash functions, buckets, slots, collisions, and overflow handling.".into(),
                    source_path: None,
                    source_id: Some("source-hash".into()),
                    markdown_path: None,
                    image_path: None,
                    provenance: None,
                },
            ],
            nodes: vec![
                BrainNodeRecord {
                    node_id: "concept:source-paper:docmate".into(),
                    kind: BrainNodeKind::Concept,
                    label: "DocMate".into(),
                    scope: BrainScope::Project,
                    aliases: vec!["PDF parsing".into()],
                    evidence_ids: vec!["ev-paper".into()],
                    source_ids: vec!["source-paper".into()],
                    confidence: Some(0.9),
                    updated_at: 1,
                    valid_from: 0,
                    valid_to: None,
                    superseded_by: None,
                },
                BrainNodeRecord {
                    node_id: "concept:source-hash:hash-table".into(),
                    kind: BrainNodeKind::Concept,
                    label: "Hash table".into(),
                    scope: BrainScope::Project,
                    aliases: vec!["Hashing".into()],
                    evidence_ids: vec!["ev-hash".into()],
                    source_ids: vec!["source-hash".into()],
                    confidence: Some(0.9),
                    updated_at: 1,
                    valid_from: 0,
                    valid_to: None,
                    superseded_by: None,
                },
            ],
            relations: vec![],
            memories: vec![],
            wiki_pages: vec![],
            entities: vec![],
            claims: vec![],
            extractions: vec![],
            events: vec![],
        };

        assert!(workspace_linking_candidates(&snapshot, "source-paper").is_empty());
    }
}
