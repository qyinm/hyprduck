use super::cleanup::*;
use super::origin::*;
use super::replay::*;
use super::*;

pub(crate) fn write_materialized_brain_repo(
    root: &Path,
    snapshot: &BrainRepoSnapshot,
) -> Result<()> {
    let writer = BrainWorkspaceWriter::open(root.to_path_buf())?;
    let root = writer.root();
    ensure_materialized_brain_repo_dirs(root)?;
    let effective_state = compute_effective_brain_state(root, snapshot)?;
    persist_effective_brain_state(root, &effective_state)?;

    Ok(())
}

pub(crate) fn compute_effective_brain_snapshot(
    root: &Path,
    snapshot: &BrainRepoSnapshot,
) -> Result<BrainRepoSnapshot> {
    Ok(compute_effective_brain_state(root, snapshot)?.snapshot)
}

struct EffectiveBrainState {
    snapshot: BrainRepoSnapshot,
    origins: MaterializedRecordOrigins,
}

fn compute_effective_brain_state(
    root: &Path,
    snapshot: &BrainRepoSnapshot,
) -> Result<EffectiveBrainState> {
    let mut effective_snapshot = snapshot.clone();
    let disk_origins = read_materialized_record_origins(root)?;
    let events_path = root.join("events/brain_events.jsonl");
    let existing_events = if events_path.exists() {
        read_brain_events_jsonl(&events_path)
            .with_context(|| format!("failed reading existing events {}", events_path.display()))?
    } else {
        Vec::new()
    };
    effective_snapshot.events =
        merge_preserved_brain_events(snapshot.events.clone(), &existing_events);
    let existing_memories = read_memory_records(root)?;
    let mut bootstrap_snapshot = effective_snapshot.clone();
    bootstrap_snapshot.memories =
        merge_materialized_memory_records(snapshot.memories.clone(), existing_memories.clone());
    let previous_origins =
        merge_bootstrapped_materialized_record_origins(disk_origins, &bootstrap_snapshot, snapshot);
    let protected_current_records =
        ProtectedMaterializedRecordKeys::from_snapshot(snapshot, &previous_origins);
    let existing_memories = discard_stale_workspace_linking_memory_collisions(
        existing_memories,
        snapshot,
        &previous_origins,
    );
    effective_snapshot.memories =
        merge_materialized_memory_records(snapshot.memories.clone(), existing_memories);
    let origins = replay_preserved_materialized_graph_events(
        &mut effective_snapshot,
        &previous_origins,
        &protected_current_records,
    )?;
    apply_accepted_proposals_to_snapshot(root, &mut effective_snapshot)?;
    refresh_materialized_wiki_pages(&mut effective_snapshot);
    refresh_current_materialized_events(&mut effective_snapshot)?;
    Ok(EffectiveBrainState {
        snapshot: effective_snapshot,
        origins,
    })
}

pub(crate) fn persist_effective_brain_snapshot(
    root: &Path,
    effective_snapshot: &BrainRepoSnapshot,
) -> Result<()> {
    let effective_state = compute_effective_brain_state(root, effective_snapshot)?;
    persist_effective_brain_state(root, &effective_state)
}

fn persist_effective_brain_state(root: &Path, effective_state: &EffectiveBrainState) -> Result<()> {
    let effective_snapshot = &effective_state.snapshot;
    persist_materialized_graph_and_wiki_state(root, effective_snapshot)?;
    write_json_pretty(
        &root.join("memory/records.json"),
        &effective_snapshot.memories,
    )?;
    write_structured_extraction_artifacts(root, &effective_snapshot.extractions)?;
    write_brain_events_jsonl(
        &root.join("events/brain_events.jsonl"),
        &effective_snapshot.events,
    )?;
    write_materialized_record_origins(root, &effective_state.origins)?;
    publish_latest_readable_graph_snapshot_marker(root, effective_snapshot)?;

    Ok(())
}
