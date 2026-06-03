use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

use crate::{BrainNodeKind, BrainRelationKind, ClaimRecord};

pub(super) const PROVIDER_GRAPH_PROMPT_VERSION: u32 = 2;
pub(super) const PROVIDER_SOURCE_GRAPH_SCHEMA_VERSION: u32 = 1;
pub(super) const PROVIDER_WORKSPACE_LINKING_SCHEMA_VERSION: u32 = 1;
pub(super) const SOURCE_GRAPH_CHUNK_BATCH_MAX_CHARS: usize = 80_000;
pub(super) const SOURCE_GRAPH_CHUNK_BATCH_MAX_CHUNKS: usize = 40;
pub(super) const SOURCE_GRAPH_AUTO_BATCH_LIMIT: usize = 8;
pub(super) const SOURCE_GRAPH_CHUNK_PARALLELISM: usize = 4;
pub(super) const SOURCE_GRAPH_TARGET_CONCEPTS: usize = 18;
pub(super) const SOURCE_GRAPH_HARD_MAX_CONCEPTS: usize = 32;
pub(super) const SOURCE_GRAPH_HARD_MAX_RELATIONS: usize = 48;
pub(super) const SOURCE_GRAPH_MAX_CLAIMS: usize = 12;
pub(super) const SOURCE_GRAPH_MAX_EVIDENCE_PER_NODE: usize = 8;
pub(super) const SOURCE_GRAPH_MAX_EVIDENCE_PER_RELATION: usize = 6;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct GraphCandidateBatch {
    pub(crate) run_id: String,
    pub(crate) chunk_run_id: String,
    pub(crate) source_id: String,
    #[serde(default)]
    pub(crate) chunk_ids: Vec<String>,
    #[serde(default)]
    pub(crate) nodes: Vec<GraphCandidateNode>,
    #[serde(default)]
    pub(crate) relations: Vec<GraphCandidateRelation>,
    #[serde(default)]
    pub(crate) claims: Vec<ClaimRecord>,
    #[serde(default)]
    pub(crate) raw_response_ref: Option<String>,
    pub(crate) created_at: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct GraphCandidateNode {
    pub(crate) raw_node_id: String,
    pub(crate) label: String,
    pub(crate) kind: BrainNodeKind,
    #[serde(default)]
    pub(crate) aliases: Vec<String>,
    #[serde(default)]
    pub(crate) evidence_ids: Vec<String>,
    #[serde(default)]
    pub(crate) page_refs: Vec<String>,
    #[serde(default)]
    pub(crate) confidence: Option<f32>,
    #[serde(default)]
    pub(crate) reason: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct GraphCandidateRelation {
    pub(crate) raw_relation_id: String,
    pub(crate) source_raw_node_id: String,
    pub(crate) target_raw_node_id: String,
    pub(crate) kind: BrainRelationKind,
    #[serde(default)]
    pub(crate) label: String,
    #[serde(default)]
    pub(crate) evidence_ids: Vec<String>,
    #[serde(default)]
    pub(crate) confidence: Option<f32>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SourceGraphCompactionReport {
    pub(crate) raw_node_count: usize,
    pub(crate) raw_relation_count: usize,
    pub(crate) deduped_node_count: usize,
    pub(crate) deduped_relation_count: usize,
    pub(crate) materialized_node_count: usize,
    pub(crate) materialized_relation_count: usize,
    pub(crate) dropped_node_count: usize,
    pub(crate) dropped_relation_count: usize,
    #[serde(default)]
    pub(crate) drop_reasons: BTreeMap<String, usize>,
    #[serde(default)]
    pub(crate) candidate_to_canonical_map: BTreeMap<String, Option<String>>,
}
