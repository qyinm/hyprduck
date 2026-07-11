use super::*;

pub(crate) fn ensure_materialized_brain_repo_dirs(root: &Path) -> Result<()> {
    let wiki_root = root.join("wiki");
    for dir in [
        root.join("graph"),
        root.join("artifacts"),
        root.join("events"),
        root.join("memory"),
        root.join("state"),
        wiki_root.join("sources"),
        wiki_root.join("entities"),
        wiki_root.join("topics"),
        wiki_root.join("claims"),
        wiki_root.join("questions"),
    ] {
        fs::create_dir_all(&dir).with_context(|| format!("failed creating {}", dir.display()))?;
    }

    fs::write(root.join("memory/.gitkeep"), "").context("failed writing memory placeholder")?;
    Ok(())
}

pub(crate) fn publish_latest_readable_graph_snapshot_marker(
    root: &Path,
    snapshot: &BrainRepoSnapshot,
) -> Result<Option<LatestReadableGraphSnapshotMarker>> {
    validate_latest_readable_materialized_files(root, snapshot)?;
    let Some(event) = latest_graph_materialized_event(&snapshot.events, &snapshot.workspace_id)
    else {
        return Ok(None);
    };
    let materialized_at = event
        .causality
        .materialized_version
        .unwrap_or(event.created_at);
    let snapshot_id = event
        .causality
        .snapshot_id
        .clone()
        .unwrap_or_else(|| format!("snapshot-{}-{materialized_at}", snapshot.workspace_id));
    let marker = LatestReadableGraphSnapshotMarker {
        schema_version: BRAIN_EVENT_SCHEMA_VERSION,
        workspace_id: snapshot.workspace_id.clone(),
        snapshot_id,
        event_id: event.event_id.clone(),
        source_ingest_id: graph_snapshot_source_ingest_id(event),
        artifact_role: MATERIALIZED_ARTIFACT_ROLE_MIGRATION_INPUT.into(),
        canonical_state_store: CANONICAL_STATE_STORE_SQLITE_GRAPHQLITE.into(),
        materialized_at,
        published_at: unix_timestamp_seconds(),
        source_markdown_refs: event.source_markdown_refs.clone(),
        materialized_files: latest_readable_materialized_file_refs(snapshot),
    };
    write_json_pretty(&root.join(LATEST_READABLE_SNAPSHOT_PATH), &marker)?;
    Ok(Some(marker))
}

pub(crate) fn read_latest_readable_graph_snapshot_marker(
    root: &Path,
) -> Result<Option<LatestReadableGraphSnapshotMarker>> {
    let repo = BrainArtifactRepository::new(root.to_path_buf());
    let path = root.join(LATEST_READABLE_SNAPSHOT_PATH);
    if !path.exists() {
        return Ok(None);
    }
    repo.read_json_artifact(LATEST_READABLE_SNAPSHOT_PATH)
        .map(Some)
}

pub(crate) fn validate_latest_readable_materialized_files(
    root: &Path,
    snapshot: &BrainRepoSnapshot,
) -> Result<()> {
    let repo = BrainArtifactRepository::new(root.to_path_buf());
    let manifest: BrainRepoSnapshot = repo.read_json_artifact("brain-manifest.json")?;
    if manifest.workspace_id != snapshot.workspace_id {
        bail!(
            "materialized brain manifest workspace_id {} does not match {}",
            manifest.workspace_id,
            snapshot.workspace_id
        );
    }
    let nodes: Vec<BrainNodeRecord> = repo.read_json_artifact("graph/nodes.json")?;
    let edges: Vec<BrainRelationRecord> = repo.read_json_artifact("graph/edges.json")?;
    let claims: Vec<ClaimRecord> = repo.read_json_artifact("graph/claims.json")?;
    let memories: Vec<MemoryRecord> = repo.read_optional_json_artifact("memory/records.json")?;
    let events = repo.read_brain_events()?;
    if nodes != snapshot.nodes {
        bail!("materialized graph/nodes.json does not match the completed snapshot");
    }
    if edges != snapshot.relations {
        bail!("materialized graph/edges.json does not match the completed snapshot");
    }
    if claims != snapshot.claims {
        bail!("materialized graph/claims.json does not match the completed snapshot");
    }
    if memories != snapshot.memories {
        bail!("materialized memory/records.json does not match the completed snapshot");
    }
    if events != snapshot.events {
        bail!("materialized events/brain_events.jsonl does not match the completed snapshot");
    }
    for page in &snapshot.wiki_pages {
        let page_body = repo
            .read_text_artifact(&page.path)
            .with_context(|| format!("failed reading materialized wiki page {}", page.path))?;
        let expected = materialized_wiki_page_body(page, snapshot);
        if page_body != expected {
            bail!(
                "materialized wiki page {} does not match the completed snapshot",
                page.path
            );
        }
    }
    Ok(())
}

pub(crate) fn latest_readable_materialized_file_refs(snapshot: &BrainRepoSnapshot) -> Vec<String> {
    let mut files = vec![
        "brain-manifest.json".to_string(),
        "graph/nodes.json".to_string(),
        "graph/edges.json".to_string(),
        "graph/claims.json".to_string(),
        "memory/records.json".to_string(),
        "events/brain_events.jsonl".to_string(),
        MATERIALIZED_RECORD_ORIGINS_PATH.to_string(),
    ];
    files.extend(snapshot.wiki_pages.iter().map(|page| page.path.clone()));
    files.sort();
    files.dedup();
    files
}

pub(crate) fn write_structured_extraction_artifacts(
    root: &Path,
    extractions: &[StructuredExtractionArtifact],
) -> Result<()> {
    for extraction in extractions {
        write_json_pretty(
            &root
                .join("artifacts")
                .join(sanitize_name(&extraction.source_id))
                .join("extraction.json"),
            extraction,
        )?;
    }
    Ok(())
}

pub(crate) fn persist_materialized_graph_and_wiki_state(
    root: &Path,
    snapshot: &BrainRepoSnapshot,
) -> Result<()> {
    remove_stale_materialized_wiki_files(root, snapshot)?;
    write_json_pretty(&root.join("brain-manifest.json"), snapshot)?;
    write_json_pretty(&root.join("graph/nodes.json"), &snapshot.nodes)?;
    write_json_pretty(&root.join("graph/edges.json"), &snapshot.relations)?;
    write_json_pretty(&root.join("graph/evidence.json"), &snapshot.evidence)?;
    write_json_pretty(&root.join("graph/entities.json"), &snapshot.entities)?;
    write_json_pretty(&root.join("graph/claims.json"), &snapshot.claims)?;

    for page in &snapshot.wiki_pages {
        let path = root.join(&page.path);
        write_file_atomic(
            &path,
            materialized_wiki_page_body(page, snapshot).as_bytes(),
        )?;
    }
    Ok(())
}

fn remove_stale_materialized_wiki_files(root: &Path, snapshot: &BrainRepoSnapshot) -> Result<()> {
    let next_wiki_paths = snapshot
        .wiki_pages
        .iter()
        .map(|page| page.path.as_str())
        .collect::<BTreeSet<_>>();
    for relative_path in existing_wiki_markdown_files(root)? {
        if next_wiki_paths.contains(relative_path.as_str()) {
            continue;
        }
        let path = root.join(&relative_path);
        if path.exists() {
            fs::remove_file(&path)
                .with_context(|| format!("failed removing stale wiki page {}", path.display()))?;
        }
    }
    Ok(())
}

fn existing_wiki_markdown_files(root: &Path) -> Result<Vec<String>> {
    let wiki_root = root.join("wiki");
    if !wiki_root.exists() {
        return Ok(Vec::new());
    }
    let mut stack = vec![wiki_root];
    let mut files = Vec::new();
    while let Some(dir) = stack.pop() {
        for entry in
            fs::read_dir(&dir).with_context(|| format!("failed reading {}", dir.display()))?
        {
            let entry = entry.with_context(|| format!("failed reading {}", dir.display()))?;
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
                continue;
            }
            let Ok(relative_path) = path.strip_prefix(root) else {
                continue;
            };
            let relative_path = relative_path.to_string_lossy().replace('\\', "/");
            if is_wiki_markdown_ref(&relative_path) {
                files.push(relative_path);
            }
        }
    }
    Ok(files)
}

pub(crate) fn read_structured_extraction_artifacts(
    root: &Path,
    sources: &[SourceRecord],
) -> Result<Vec<StructuredExtractionArtifact>> {
    let mut artifacts = Vec::new();
    for source in sources {
        let path = root
            .join("artifacts")
            .join(sanitize_name(&source.source_id))
            .join("extraction.json");
        if path.exists() {
            artifacts.push(
                read_json_artifact(&path)
                    .with_context(|| format!("failed reading {}", path.display()))?,
            );
        }
    }
    Ok(artifacts)
}

pub(crate) fn read_markdown_claim_candidates_for_row(
    row: &StoredSourceRow,
) -> Vec<MarkdownClaimCandidate> {
    Path::new(&row.manifest_path)
        .parent()
        .map(|artifact_root| artifact_root.join("claim-candidates.json"))
        .filter(|path| path.exists())
        .and_then(|path| read_json_artifact::<Vec<MarkdownClaimCandidate>>(&path).ok())
        .unwrap_or_default()
}

