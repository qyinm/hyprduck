#![allow(dead_code)]

use crate::*;

pub(crate) struct PageSection {
    pub(crate) page_index: usize,
    pub(crate) page_label: String,
    pub(crate) content: String,
    pub(crate) markdown_path: Option<String>,
    pub(crate) image_path: Option<String>,
}

#[derive(Debug, Clone)]
pub(crate) struct ConceptAccumulator {
    pub(crate) id: String,
    pub(crate) label: String,
    pub(crate) aliases: BTreeSet<String>,
    pub(crate) evidence: Vec<EvidenceRef>,
    pub(crate) page_labels: BTreeSet<String>,
}

#[derive(Debug, Clone)]
pub(crate) struct ExtractionEvidenceRef {
    pub(crate) id: String,
    pub(crate) page_index: usize,
    pub(crate) page_label: String,
    pub(crate) snippet: String,
    pub(crate) source_path: String,
    pub(crate) source_id: Option<String>,
    pub(crate) markdown_path: Option<String>,
    pub(crate) image_path: Option<String>,
    pub(crate) provenance: String,
}

#[derive(Debug, Clone)]
pub(crate) struct ExtractedConcept {
    pub(crate) id: String,
    pub(crate) label: String,
    pub(crate) aliases: BTreeSet<String>,
    pub(crate) evidence_ids: Vec<String>,
    pub(crate) page_labels: BTreeSet<String>,
}

#[derive(Debug, Clone)]
pub(crate) struct ExtractedClaim {
    pub(crate) id: String,
    pub(crate) text: String,
    pub(crate) subject_concept_id: String,
    pub(crate) evidence_id: String,
}

#[derive(Debug, Clone)]
pub(crate) struct ExtractedRelation {
    pub(crate) source_concept_id: String,
    pub(crate) target_concept_id: String,
    pub(crate) relation_kind: BrainRelationKind,
    pub(crate) confidence: f32,
    pub(crate) evidence_ids: Vec<String>,
    pub(crate) page_labels: BTreeSet<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct MarkdownNodeCandidate {
    pub(crate) candidate_id: String,
    pub(crate) label: String,
    pub(crate) kind: BrainNodeKind,
    pub(crate) source_path: String,
    pub(crate) line_start: usize,
    pub(crate) evidence_snippet: String,
    pub(crate) confidence: f32,
    pub(crate) reason: String,
    #[serde(default)]
    pub(crate) matched_node_id: Option<String>,
    #[serde(default)]
    pub(crate) matched_node_label: Option<String>,
    #[serde(default)]
    pub(crate) match_score: Option<f32>,
    #[serde(default)]
    pub(crate) match_reason: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct MarkdownRelationshipEvidence {
    pub(crate) candidate_id: String,
    pub(crate) evidence_id: String,
    pub(crate) source_path: String,
    #[serde(default)]
    pub(crate) source_id: Option<String>,
    #[serde(default)]
    pub(crate) source_refs: Vec<String>,
    pub(crate) line_start: usize,
    pub(crate) snippet: String,
    pub(crate) source_label: String,
    pub(crate) target_label: String,
    pub(crate) relation_kind: BrainRelationKind,
    pub(crate) relation_label: String,
    pub(crate) confidence: f32,
    pub(crate) reason: String,
    #[serde(default)]
    pub(crate) matched_source_node_id: Option<String>,
    #[serde(default)]
    pub(crate) matched_target_node_id: Option<String>,
    #[serde(default)]
    pub(crate) resolved_source_node_id: Option<String>,
    #[serde(default)]
    pub(crate) resolved_target_node_id: Option<String>,
    #[serde(default)]
    pub(crate) endpoint_resolution: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct MarkdownClaimCandidate {
    pub(crate) candidate_id: String,
    pub(crate) evidence_id: String,
    pub(crate) statement: String,
    #[serde(default)]
    pub(crate) classification: MarkdownClaimClassification,
    #[serde(default)]
    pub(crate) durable: bool,
    #[serde(default)]
    pub(crate) memory_candidate: bool,
    pub(crate) source_path: String,
    #[serde(default)]
    pub(crate) source_id: Option<String>,
    #[serde(default)]
    pub(crate) source_refs: Vec<String>,
    pub(crate) line_start: usize,
    pub(crate) line_end: usize,
    pub(crate) char_start: usize,
    pub(crate) char_end: usize,
    pub(crate) evidence_span: MarkdownEvidenceSpan,
    pub(crate) evidence_snippet: String,
    #[serde(default)]
    pub(crate) subject_labels: Vec<String>,
    #[serde(default)]
    pub(crate) subject_refs: Vec<String>,
    pub(crate) confidence: f32,
    pub(crate) reason: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct MarkdownSignalArtifact {
    pub(crate) source_path: String,
    #[serde(default)]
    pub(crate) source_id: Option<String>,
    #[serde(default)]
    pub(crate) source_refs: Vec<String>,
    #[serde(default)]
    pub(crate) title: Option<String>,
    #[serde(default)]
    pub(crate) headings: Vec<MarkdownHeadingSignal>,
    #[serde(default)]
    pub(crate) links: Vec<MarkdownLinkSignal>,
    #[serde(default)]
    pub(crate) entities: Vec<MarkdownEntitySignal>,
    #[serde(default)]
    pub(crate) keywords: Vec<MarkdownKeywordSignal>,
    #[serde(default)]
    pub(crate) related_pages: Vec<MarkdownRelatedPageSignal>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct MarkdownHeadingSignal {
    pub(crate) text: String,
    pub(crate) level: usize,
    pub(crate) line_start: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct MarkdownLinkSignal {
    pub(crate) label: String,
    pub(crate) target: String,
    pub(crate) kind: String,
    pub(crate) line_start: usize,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct MarkdownEntitySignal {
    pub(crate) label: String,
    pub(crate) line_start: usize,
    pub(crate) confidence: f32,
    pub(crate) reason: String,
    #[serde(default)]
    pub(crate) matched_node_id: Option<String>,
    #[serde(default)]
    pub(crate) matched_node_label: Option<String>,
    #[serde(default)]
    pub(crate) match_score: Option<f32>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct MarkdownKeywordSignal {
    pub(crate) term: String,
    pub(crate) count: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct MarkdownRelatedPageSignal {
    pub(crate) page_id: String,
    pub(crate) path: String,
    pub(crate) title: String,
    pub(crate) score: usize,
    pub(crate) matched_terms: Vec<String>,
    pub(crate) reason: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct MarkdownEvidenceSpan {
    pub(crate) source_path: String,
    #[serde(default)]
    pub(crate) source_id: Option<String>,
    pub(crate) line_start: usize,
    pub(crate) line_end: usize,
    pub(crate) char_start: usize,
    pub(crate) char_end: usize,
    pub(crate) snippet: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum MarkdownClaimClassification {
    DurableFact,
    Decision,
}

impl Default for MarkdownClaimClassification {
    fn default() -> Self {
        Self::DurableFact
    }
}

#[derive(Debug, Clone)]
pub(crate) struct ExtractionArtifact {
    pub(crate) concepts: Vec<ExtractedConcept>,
    pub(crate) claims: Vec<ExtractedClaim>,
    pub(crate) relations: Vec<ExtractedRelation>,
    pub(crate) evidence_refs: BTreeMap<String, ExtractionEvidenceRef>,
}

#[derive(Debug, Clone)]
pub(crate) struct PageConceptSet {
    pub(crate) page_index: usize,
    pub(crate) page_label: String,
    pub(crate) concept_ids: Vec<String>,
    pub(crate) snippet: String,
    pub(crate) markdown_path: Option<String>,
    pub(crate) image_path: Option<String>,
}

#[derive(Debug, Clone)]
pub(crate) struct CollectedConcepts {
    pub(crate) concepts: Vec<ConceptAccumulator>,
    pub(crate) page_concepts: Vec<PageConceptSet>,
    pub(crate) relation_candidates: Vec<RelationCandidateAccumulator>,
}

#[derive(Debug, Clone)]
pub(crate) struct EdgeAccumulator {
    pub(crate) source_node_id: String,
    pub(crate) target_node_id: String,
    pub(crate) relation_kind: BrainRelationKind,
    pub(crate) label: String,
    pub(crate) confidence: Option<f32>,
    pub(crate) evidence: Vec<EvidenceRef>,
    pub(crate) page_labels: BTreeSet<String>,
}

#[derive(Debug, Clone)]
pub(crate) struct RelationCandidateAccumulator {
    pub(crate) source_node_id: String,
    pub(crate) target_node_id: String,
    pub(crate) relation_kind: BrainRelationKind,
    pub(crate) confidence: f32,
    pub(crate) evidence: Vec<EvidenceRef>,
    pub(crate) page_labels: BTreeSet<String>,
}

#[derive(Debug, Clone)]
pub(crate) struct StoredSourceRow {
    pub(crate) summary: SourceSummary,
    pub(crate) project_id: String,
    pub(crate) manifest_path: String,
}

#[derive(Debug, Clone)]
pub(crate) struct WorkspaceConceptAccumulator {
    pub(crate) node_id: String,
    pub(crate) canonical_name: String,
    pub(crate) aliases: BTreeSet<String>,
    pub(crate) evidence: Vec<EvidenceRef>,
    pub(crate) confidence: Option<f32>,
}

mod aggregate;
mod answer;
mod cleanup;
mod compiler;
mod corrections;
mod markdown_extract;
mod materialize;
mod origin;
mod replay;

pub(crate) use aggregate::*;
pub(crate) use answer::*;
pub(crate) use compiler::*;
pub(crate) use corrections::*;
pub(crate) use markdown_extract::*;
pub(crate) use materialize::*;
pub(crate) use replay::*;
