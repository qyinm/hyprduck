use std::path::Path;

use anyhow::Result;
use hyprduck_engine_types::BrainRepoSnapshot;

use crate::{
    brain_repo::write_brain_events_jsonl,
    domains::knowledge_store::{KnowledgeGraphPersistReport, KnowledgeStore},
    knowledge::{
        ensure_materialized_brain_repo_dirs, persist_materialized_graph_and_wiki_state,
        publish_latest_readable_graph_snapshot_marker,
    },
};

pub(crate) fn commit_graph_materialization(
    root: &Path,
    store: &KnowledgeStore,
    snapshot: &BrainRepoSnapshot,
) -> Result<KnowledgeGraphPersistReport> {
    let report = store.persist_graph_snapshot(snapshot)?;
    ensure_materialized_brain_repo_dirs(root)?;
    persist_materialized_graph_and_wiki_state(root, snapshot)?;
    write_brain_events_jsonl(&root.join("events/brain_events.jsonl"), &snapshot.events)?;
    publish_latest_readable_graph_snapshot_marker(root, snapshot)?;
    Ok(report)
}
