use serde::{Deserialize, Serialize};

use crate::BrainReadScope;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WriteProposeRequest {
    pub scope: BrainReadScope,
    pub content_type: String,
    pub title: String,
    pub body: String,
    pub evidence_refs: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WriteProposeResponseData {
    pub proposal_id: String,
    pub status: String,
    pub created_at: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WriteCommitRequest {
    pub scope: BrainReadScope,
    pub proposal_id: String,
    #[serde(default)]
    pub user_approved: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WriteCommitResponseData {
    pub event_id: String,
    pub memory_id: String,
    pub stored_at: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WriteCommitAllRequest {
    pub scope: BrainReadScope,
    pub proposal_ids: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WriteCommitAllResponseData {
    pub results: Vec<WriteCommitResultItem>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WriteCommitResultItem {
    pub proposal_id: String,
    pub status: String,
    #[serde(default)]
    pub event_id: Option<String>,
    #[serde(default)]
    pub memory_id: Option<String>,
    #[serde(default)]
    pub error_category: Option<String>,
    #[serde(default)]
    pub error: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WriteListRequest {
    pub scope: BrainReadScope,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WriteListResponseData {
    pub proposals: Vec<WriteProposalSummary>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WriteProposalSummary {
    pub proposal_id: String,
    pub content_type: String,
    pub title: String,
    pub body: String,
    pub evidence_refs: Vec<String>,
    pub created_at: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WriteRejectRequest {
    pub scope: BrainReadScope,
    pub proposal_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WriteRejectResponseData {
    pub proposal_id: String,
    pub status: String,
}
