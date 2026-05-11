use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProjectStatus {
    Preview,
    Ready,
    Degraded,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GraphNodeKind {
    Source,
    Document,
    Page,
    Concept,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AnswerStatus {
    Grounded,
    LowConfidence,
    Blocked,
    Stale,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GraphNodePosition {
    pub x: f32,
    pub y: f32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectOverview {
    pub project_id: String,
    pub title: String,
    pub status: ProjectStatus,
    #[serde(default)]
    pub stale: bool,
    pub summary: String,
    pub document_count: usize,
    pub node_count: usize,
    pub relationship_count: usize,
    pub evidence_count: usize,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GraphNodeSummary {
    pub id: String,
    pub label: String,
    pub kind: GraphNodeKind,
    #[serde(default)]
    pub confidence: Option<f32>,
    pub related_count: usize,
    pub evidence_count: usize,
    pub position: GraphNodePosition,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EvidenceRef {
    pub id: String,
    pub page_label: String,
    #[serde(default)]
    pub page_index: Option<usize>,
    pub snippet: String,
    #[serde(default)]
    pub source_path: Option<String>,
    #[serde(default)]
    pub source_id: Option<String>,
    #[serde(default)]
    pub markdown_path: Option<String>,
    #[serde(default)]
    pub image_path: Option<String>,
    #[serde(default)]
    pub provenance: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SourceBacking {
    pub workspace_id: String,
    pub source_id: String,
    pub original_path: String,
    pub source_path: String,
    pub markdown_path: String,
    pub format: String,
    pub status: String,
    pub page_count: usize,
    pub success_count: usize,
    pub failed_count: usize,
    pub updated_at: u64,
    #[serde(default)]
    pub manifest_path: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CorrectionKind {
    Merge,
    KeepSeparate,
    Rename,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CorrectionAction {
    pub kind: CorrectionKind,
    pub label: String,
    #[serde(default)]
    pub disabled_reason: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GraphNodeDetail {
    pub node: GraphNodeSummary,
    pub canonical_name: String,
    #[serde(default)]
    pub aliases: Vec<String>,
    pub description: String,
    #[serde(default)]
    pub evidence: Vec<EvidenceRef>,
    #[serde(default)]
    pub actions: Vec<CorrectionAction>,
    #[serde(default)]
    pub source: Option<SourceBacking>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RelationKind {
    SourceDocument,
    RelatedTo,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RelationEdgeSummary {
    pub id: String,
    pub source_node_id: String,
    pub target_node_id: String,
    pub kind: RelationKind,
    pub label: String,
    #[serde(default)]
    pub confidence: Option<f32>,
    pub evidence_count: usize,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RelationEdgeDetail {
    pub edge: RelationEdgeSummary,
    pub explanation: String,
    #[serde(default)]
    pub evidence: Vec<EvidenceRef>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SuggestedActionKind {
    InspectEvidence,
    ApplyCorrection,
    ReimportProject,
    AskDifferentQuestion,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SuggestedAction {
    pub kind: SuggestedActionKind,
    pub label: String,
    pub description: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AnswerResponse {
    pub status: AnswerStatus,
    #[serde(default)]
    pub text: Option<String>,
    pub explanation: String,
    #[serde(default)]
    pub citations: Vec<EvidenceRef>,
    #[serde(default)]
    pub related_node_ids: Vec<String>,
    #[serde(default)]
    pub suggested_actions: Vec<SuggestedAction>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct KnowledgeProject {
    pub summary: ProjectOverview,
    pub nodes: Vec<GraphNodeSummary>,
    #[serde(default)]
    pub edges: Vec<RelationEdgeSummary>,
    #[serde(default)]
    pub details_by_node_id: BTreeMap<String, GraphNodeDetail>,
    #[serde(default)]
    pub edge_details_by_id: BTreeMap<String, RelationEdgeDetail>,
    #[serde(default)]
    pub answer_by_node_id: BTreeMap<String, AnswerResponse>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn answer_response_serializes_expected_status_shape() {
        let response = AnswerResponse {
            status: AnswerStatus::LowConfidence,
            text: Some("Need more evidence before answering confidently.".into()),
            explanation: "Preview answer is grounded by visible snippets only.".into(),
            citations: vec![EvidenceRef {
                id: "ev-1".into(),
                page_label: "Page 1".into(),
                page_index: Some(0),
                snippet: "Visible evidence snippet".into(),
                source_path: Some("sample.pdf".into()),
                source_id: Some("source-1".into()),
                markdown_path: Some("pages/page_1.md".into()),
                image_path: Some("images/page_1.png".into()),
                provenance: Some("Test evidence".into()),
            }],
            related_node_ids: vec!["page-1".into()],
            suggested_actions: vec![SuggestedAction {
                kind: SuggestedActionKind::InspectEvidence,
                label: "Inspect evidence".into(),
                description: "Review the cited snippets before trusting the draft answer.".into(),
            }],
        };

        let encoded = serde_json::to_string(&response).expect("serialize answer response");
        assert!(encoded.contains("\"status\":\"low_confidence\""));
        assert!(encoded.contains("\"relatedNodeIds\":[\"page-1\"]"));
    }

    #[test]
    fn relation_edge_uses_camel_case_wire_shape() {
        let edge = RelationEdgeSummary {
            id: "edge-a-b".into(),
            source_node_id: "concept-a".into(),
            target_node_id: "concept-b".into(),
            kind: RelationKind::RelatedTo,
            label: "Related in source".into(),
            confidence: Some(0.74),
            evidence_count: 2,
        };

        let encoded = serde_json::to_string(&edge).expect("serialize relation edge");
        assert!(encoded.contains("\"sourceNodeId\":\"concept-a\""));
        assert!(encoded.contains("\"targetNodeId\":\"concept-b\""));
        assert!(encoded.contains("\"kind\":\"related_to\""));
    }
}
