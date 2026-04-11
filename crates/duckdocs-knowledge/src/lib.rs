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
pub struct GraphNodeSummary {
    pub id: String,
    pub label: String,
    pub kind: GraphNodeKind,
    #[serde(default)]
    pub confidence: Option<f32>,
    pub related_count: usize,
    pub evidence_count: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EvidenceRef {
    pub id: String,
    pub page_label: String,
    pub snippet: String,
    #[serde(default)]
    pub source_path: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CorrectionKind {
    Merge,
    KeepSeparate,
    Rename,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CorrectionAction {
    pub kind: CorrectionKind,
    pub label: String,
    #[serde(default)]
    pub disabled_reason: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
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
pub struct SuggestedAction {
    pub kind: SuggestedActionKind,
    pub label: String,
    pub description: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
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
                snippet: "Visible evidence snippet".into(),
                source_path: Some("sample.pdf".into()),
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
        assert!(encoded.contains("\"related_node_ids\":[\"page-1\"]"));
    }
}
