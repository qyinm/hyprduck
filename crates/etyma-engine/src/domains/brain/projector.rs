use std::path::Path;

use anyhow::Result;
use etyma_engine_types::BrainRepoSnapshot;

use crate::knowledge::{
    ensure_materialized_brain_repo_dirs, persist_materialized_graph_and_wiki_state,
    publish_latest_readable_graph_snapshot_marker,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct GraphWikiProjectionReport {
    pub(crate) node_count: usize,
    pub(crate) relation_count: usize,
    pub(crate) wiki_page_count: usize,
    pub(crate) evidence_count: usize,
}

impl GraphWikiProjectionReport {
    fn from_snapshot(snapshot: &BrainRepoSnapshot) -> Self {
        Self {
            node_count: snapshot.nodes.len(),
            relation_count: snapshot.relations.len(),
            wiki_page_count: snapshot.wiki_pages.len(),
            evidence_count: snapshot.evidence.len(),
        }
    }
}

pub(crate) fn project_graph_wiki_read_model(
    root: &Path,
    snapshot: &BrainRepoSnapshot,
) -> Result<GraphWikiProjectionReport> {
    ensure_materialized_brain_repo_dirs(root)?;
    persist_materialized_graph_and_wiki_state(root, snapshot)?;
    Ok(GraphWikiProjectionReport::from_snapshot(snapshot))
}

pub(crate) fn publish_graph_wiki_read_model_marker(
    root: &Path,
    snapshot: &BrainRepoSnapshot,
) -> Result<()> {
    publish_latest_readable_graph_snapshot_marker(root, snapshot).map(|_| ())
}
