use crate::*;

pub(crate) fn read_materialized_brain_snapshot(
    root: &Path,
    workspace_id: &str,
) -> Result<BrainRepoSnapshot> {
    let repo = BrainArtifactRepository::new(root.to_path_buf());
    if !repo.brain_manifest_path().exists() {
        return Ok(empty_replayed_brain_snapshot(workspace_id));
    }
    let mut snapshot: BrainRepoSnapshot = repo.read_brain_manifest()?;
    if snapshot.workspace_id != workspace_id {
        bail!(
            "brain manifest workspace_id {} does not match requested workspace {}",
            snapshot.workspace_id,
            workspace_id
        );
    }
    if root.join("graph/nodes.json").exists() {
        snapshot.nodes = repo.read_json_artifact("graph/nodes.json")?;
    }
    if root.join("graph/edges.json").exists() {
        snapshot.relations = repo.read_json_artifact("graph/edges.json")?;
    }
    if root.join("graph/evidence.json").exists() {
        snapshot.evidence = repo.read_json_artifact("graph/evidence.json")?;
    }
    if root.join("graph/entities.json").exists() {
        snapshot.entities = repo.read_json_artifact("graph/entities.json")?;
    }
    if root.join("graph/claims.json").exists() {
        snapshot.claims = repo.read_json_artifact("graph/claims.json")?;
    }
    let mut extractions = Vec::new();
    for source in &snapshot.sources {
        let path = format!(
            "artifacts/{}/extraction.json",
            sanitize_name(&source.source_id)
        );
        if root.join(&path).exists() {
            extractions.push(repo.read_json_artifact(&path)?);
        }
    }
    if !extractions.is_empty() {
        snapshot.extractions = extractions;
    }
    snapshot.memories = repo.read_optional_json_artifact("memory/records.json")?;
    snapshot.events = repo.read_brain_events()?;
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
