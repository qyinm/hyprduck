use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use etyma_knowledge::{BrainNodeKind, BrainRelationKind, BrainScope};

use crate::{BrainReadScope, SourceId};

pub const GRAPH_PATCH_SCHEMA_VERSION: &str = "etyma.graph_patch.v1";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GraphPatch {
    pub schema_version: String,
    #[serde(default)]
    pub source_ids: Vec<SourceId>,
    #[serde(default)]
    pub evidence_refs: Vec<String>,
    #[serde(default)]
    pub nodes: Vec<GraphPatchNode>,
    #[serde(default)]
    pub relations: Vec<GraphPatchRelation>,
    #[serde(default)]
    pub claims: Vec<GraphPatchClaim>,
    #[serde(default)]
    pub wiki_pages: Vec<GraphPatchWikiPage>,
    #[serde(default)]
    pub agent_metadata: BTreeMap<String, Value>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GraphPatchNode {
    pub node_id: String,
    pub kind: BrainNodeKind,
    pub label: String,
    #[serde(default)]
    pub scope: Option<BrainScope>,
    #[serde(default)]
    pub aliases: Vec<String>,
    #[serde(default)]
    pub source_ids: Vec<SourceId>,
    #[serde(default)]
    pub evidence_ids: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GraphPatchRelation {
    pub relation_id: String,
    pub kind: BrainRelationKind,
    pub source_node_id: String,
    pub target_node_id: String,
    #[serde(default)]
    pub label: String,
    #[serde(default)]
    pub evidence_ids: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GraphPatchClaim {
    pub claim_id: String,
    pub statement: String,
    #[serde(default)]
    pub topic_refs: Vec<String>,
    #[serde(default)]
    pub source_refs: Vec<SourceId>,
    #[serde(default)]
    pub evidence_refs: Vec<String>,
    #[serde(default = "default_graph_patch_claim_status")]
    pub status: String,
}

fn default_graph_patch_claim_status() -> String {
    "agent_generated".into()
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GraphPatchWikiPage {
    pub page_id: String,
    pub path: String,
    pub title: String,
    pub body: String,
    #[serde(default)]
    pub node_refs: Vec<String>,
    #[serde(default)]
    pub source_refs: Vec<SourceId>,
    #[serde(default)]
    pub evidence_refs: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApplyGraphPatchRequest {
    pub scope: BrainReadScope,
    pub graph_patch: GraphPatch,
    #[serde(default)]
    pub agent_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApplyGraphPatchResponseData {
    pub event_id: String,
    pub status: String,
    pub graph_ready: bool,
    pub graph_status: String,
    pub applied_at: u64,
    pub source_ids: Vec<SourceId>,
    pub evidence_refs: Vec<String>,
    pub changed_node_ids: Vec<String>,
    pub changed_relation_ids: Vec<String>,
    pub changed_claim_ids: Vec<String>,
    pub changed_wiki_page_ids: Vec<String>,
    #[serde(default)]
    pub warnings: Vec<String>,
}
