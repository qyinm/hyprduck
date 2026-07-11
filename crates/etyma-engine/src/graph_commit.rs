use std::path::Path;

use anyhow::Result;
use etyma_engine_types::BrainRepoSnapshot;

use crate::{
    brain_repo::{
        project_graph_wiki_read_model, publish_graph_wiki_read_model_marker,
        write_brain_events_jsonl,
    },
    KnowledgeGraphPersistReport, KnowledgeStore,
};

pub(crate) fn commit_graph_materialization(
    root: &Path,
    store: &KnowledgeStore,
    snapshot: &BrainRepoSnapshot,
) -> Result<KnowledgeGraphPersistReport> {
    let report = store.persist_graph_snapshot(snapshot)?;
    project_graph_wiki_read_model(root, snapshot)?;
    write_brain_events_jsonl(&root.join("events/brain_events.jsonl"), &snapshot.events)?;
    publish_graph_wiki_read_model_marker(root, snapshot)?;
    Ok(report)
}
