use serde::{Deserialize, Serialize};

use etyma_knowledge::{
    AnswerResponse, CorrectionKind, KnowledgeProject,
};

use crate::{SourceSummary, WorkspaceId};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct LoadProjectRequest {
    #[serde(default)]
    pub project_id: Option<String>,
    #[serde(default)]
    pub workspace_id: Option<WorkspaceId>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LoadProjectResponseData {
    #[serde(default)]
    pub project: Option<KnowledgeProject>,
    #[serde(default)]
    pub workspace_id: Option<WorkspaceId>,
    #[serde(default)]
    pub sources: Vec<SourceSummary>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApplyCorrectionRequest {
    pub project_id: String,
    pub node_id: String,
    pub kind: CorrectionKind,
    #[serde(default)]
    pub target_node_id: Option<String>,
    #[serde(default)]
    pub value: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ApplyCorrectionResponseData {
    pub project: KnowledgeProject,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AnswerProjectRequest {
    pub project_id: String,
    #[serde(default)]
    pub node_id: Option<String>,
    pub question: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AnswerProjectResponseData {
    pub answer: AnswerResponse,
}
