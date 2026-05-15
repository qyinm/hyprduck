use crate::*;

pub(crate) fn read_materialized_brain_snapshot(
    root: &Path,
    workspace_id: &str,
) -> Result<BrainRepoSnapshot> {
    let manifest_path = root.join("brain-manifest.json");
    if !manifest_path.exists() {
        return Ok(empty_replayed_brain_snapshot(workspace_id));
    }
    let mut snapshot: BrainRepoSnapshot = read_json_artifact(&manifest_path)?;
    if snapshot.workspace_id != workspace_id {
        bail!(
            "brain manifest workspace_id {} does not match requested workspace {}",
            snapshot.workspace_id,
            workspace_id
        );
    }
    if root.join("graph/nodes.json").exists() {
        snapshot.nodes = read_json_artifact(&root.join("graph/nodes.json"))?;
    }
    if root.join("graph/edges.json").exists() {
        snapshot.relations = read_json_artifact(&root.join("graph/edges.json"))?;
    }
    if root.join("graph/evidence.json").exists() {
        snapshot.evidence = read_json_artifact(&root.join("graph/evidence.json"))?;
    }
    if root.join("graph/entities.json").exists() {
        snapshot.entities = read_json_artifact(&root.join("graph/entities.json"))?;
    }
    if root.join("graph/claims.json").exists() {
        snapshot.claims = read_json_artifact(&root.join("graph/claims.json"))?;
    }
    if let Ok(extractions) = read_structured_extraction_artifacts(root, &snapshot.sources) {
        if !extractions.is_empty() {
            snapshot.extractions = extractions;
        }
    }
    snapshot.memories = read_memory_records(root)?;
    snapshot.events = read_brain_events_jsonl(&root.join("events/brain_events.jsonl"))?;
    Ok(snapshot)
}

pub(crate) fn empty_replayed_brain_snapshot(workspace_id: &str) -> BrainRepoSnapshot {
    BrainRepoSnapshot {
        workspace_id: workspace_id.to_string(),
        generated_at: 0,
        sources: Vec::new(),
        nodes: Vec::new(),
        relations: Vec::new(),
        evidence: Vec::new(),
        memories: Vec::new(),
        wiki_pages: Vec::new(),
        entities: Vec::new(),
        claims: Vec::new(),
        extractions: Vec::new(),
        events: Vec::new(),
    }
}
