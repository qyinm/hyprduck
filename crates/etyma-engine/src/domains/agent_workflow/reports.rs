use crate::*;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ProviderGraphMaterializationInputFingerprint {
    pub(crate) workspace_id: String,
    pub(crate) source_id: String,
    pub(crate) manifest_updated_at: u64,
    pub(crate) markdown_hash: String,
    pub(crate) provider: String,
    pub(crate) model: String,
    pub(crate) source_graph_schema_version: u32,
    pub(crate) workspace_linking_schema_version: u32,
    pub(crate) prompt_version: u32,
    #[serde(default)]
    pub(crate) baseline_snapshot_id: Option<String>,
    #[serde(default)]
    pub(crate) baseline_event_id: Option<String>,
    #[serde(default)]
    pub(crate) baseline_materialized_at: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ProviderGraphMaterializationReport {
    pub(crate) status: String,
    pub(crate) provider: String,
    pub(crate) model: String,
    pub(crate) source_id: String,
    #[serde(default)]
    pub(crate) input_fingerprint: Option<ProviderGraphMaterializationInputFingerprint>,
    #[serde(default)]
    pub(crate) source_graph_node_count: usize,
    #[serde(default)]
    pub(crate) source_graph_relation_count: usize,
    #[serde(default)]
    pub(crate) workspace_link_count: usize,
    #[serde(default)]
    pub(crate) materialized_node_count: usize,
    #[serde(default)]
    pub(crate) materialized_relation_count: usize,
    #[serde(default)]
    pub(crate) materialized_claim_count: usize,
    #[serde(default)]
    pub(crate) materialized_memory_count: usize,
    #[serde(default)]
    pub(crate) skipped_reason: Option<String>,
    #[serde(default)]
    pub(crate) error_message: Option<String>,
    #[serde(default)]
    pub(crate) provider_run_ids: Vec<String>,
    #[serde(default)]
    pub(crate) source_graph_run_id: Option<String>,
    #[serde(default)]
    pub(crate) workspace_linking_run_id: Option<String>,
    #[serde(default)]
    pub(crate) source_graph_materialized: bool,
    #[serde(default)]
    pub(crate) workspace_linking_materialized: bool,
    #[serde(default)]
    pub(crate) retryable: bool,
    #[serde(default)]
    pub(crate) stage: String,
    #[serde(default)]
    pub(crate) progress: f32,
    #[serde(default)]
    pub(crate) failed_reason: Option<String>,
    #[serde(default)]
    pub(crate) chunk_total: usize,
    #[serde(default)]
    pub(crate) chunk_succeeded: usize,
    #[serde(default)]
    pub(crate) chunk_failed: usize,
    #[serde(default)]
    pub(crate) chunk_discovered: usize,
    #[serde(default)]
    pub(crate) chunk_processed: usize,
    #[serde(default)]
    pub(crate) chunk_skipped: usize,
    #[serde(default)]
    pub(crate) stage_runs: Vec<ProviderGraphStageRunReport>,
    #[serde(default)]
    pub(crate) raw_source_graph_node_count: usize,
    #[serde(default)]
    pub(crate) raw_source_graph_relation_count: usize,
    #[serde(default)]
    pub(crate) canonical_source_graph_node_count: usize,
    #[serde(default)]
    pub(crate) canonical_source_graph_relation_count: usize,
    #[serde(default)]
    pub(crate) pruned_source_graph_node_count: usize,
    #[serde(default)]
    pub(crate) pruned_source_graph_relation_count: usize,
    #[serde(default)]
    pub(crate) compaction_status: Option<String>,
    #[serde(default)]
    pub(crate) compaction_report_path: Option<String>,
    pub(crate) updated_at: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ProviderGraphStageRunReport {
    pub(crate) stage: String,
    pub(crate) run_id: String,
    #[serde(default)]
    pub(crate) chunk_ids: Vec<String>,
    pub(crate) status: String,
    #[serde(default)]
    pub(crate) retryable: bool,
    #[serde(default)]
    pub(crate) node_count: usize,
    #[serde(default)]
    pub(crate) relation_count: usize,
    #[serde(default)]
    pub(crate) error_message: Option<String>,
}
