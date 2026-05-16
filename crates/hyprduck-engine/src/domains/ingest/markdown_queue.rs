use crate::*;

fn markdown_source_queued_event(
    workspace_id: &str,
    queued: &MarkdownIngestQueueRecord,
) -> Result<BrainEvent> {
    Ok(BrainEvent {
        event_id: format!("evt-{}", Uuid::now_v7()),
        schema_version: BRAIN_EVENT_SCHEMA_VERSION,
        workspace_id: workspace_id.to_string(),
        scope: BrainScope::Project,
        event_type: BrainEventKind::SourceIngestQueued,
        operation_type: Some("source_ingest_queued".into()),
        actor: BrainActor {
            actor_type: BrainActorType::Agent,
            actor_id: "hyprduck-agent-ingest".into(),
        },
        source_refs: vec![queued.queue_id.clone()],
        source_markdown_refs: vec![queued.relative_path.clone()],
        node_refs: Vec::new(),
        relation_refs: Vec::new(),
        claim_refs: Vec::new(),
        memory_refs: Vec::new(),
        target_node_ids: Vec::new(),
        target_edge_ids: Vec::new(),
        target_claim_ids: Vec::new(),
        target_memory_ids: Vec::new(),
        evidence_refs: Vec::new(),
        payload_json: serde_json::to_string(queued)
            .context("failed to encode source ingest queued event payload")?,
        causality: BrainEventCausality {
            caused_by_source_ids: vec![queued.queue_id.clone()],
            materialized_version: Some(queued.enqueued_at),
            ..Default::default()
        },
        confidence: None,
        policy_result: "auto_enqueued".into(),
        created_at: queued.enqueued_at,
    })
}

fn markdown_source_compiled_event(
    record: &MarkdownIngestQueueRecord,
    manifest: Option<&SourceArtifactManifest>,
) -> Result<BrainEvent> {
    let source_refs = manifest
        .map(|manifest| vec![manifest.source_id.clone()])
        .unwrap_or_else(|| vec![record.queue_id.clone()]);
    Ok(BrainEvent {
        event_id: format!("evt-{}", Uuid::now_v7()),
        schema_version: BRAIN_EVENT_SCHEMA_VERSION,
        workspace_id: record.workspace_id.clone(),
        scope: BrainScope::Project,
        event_type: BrainEventKind::SourceCompiled,
        operation_type: Some("source_compiled".into()),
        actor: BrainActor {
            actor_type: BrainActorType::Agent,
            actor_id: "hyprduck-agent-ingest".into(),
        },
        source_refs,
        source_markdown_refs: vec![record.relative_path.clone()],
        node_refs: Vec::new(),
        relation_refs: Vec::new(),
        claim_refs: Vec::new(),
        memory_refs: Vec::new(),
        target_node_ids: Vec::new(),
        target_edge_ids: Vec::new(),
        target_claim_ids: Vec::new(),
        target_memory_ids: Vec::new(),
        evidence_refs: Vec::new(),
        payload_json: serde_json::to_string(record)
            .context("failed to encode source compiled event payload")?,
        causality: BrainEventCausality {
            caused_by_source_ids: manifest
                .map(|manifest| vec![manifest.source_id.clone()])
                .unwrap_or_else(|| vec![record.queue_id.clone()]),
            materialized_version: record.completed_at,
            ..Default::default()
        },
        confidence: None,
        policy_result: if record.status == "ingested" {
            "auto_compiled".into()
        } else {
            "failed".into()
        },
        created_at: record.completed_at.unwrap_or_else(unix_timestamp_seconds),
    })
}

fn markdown_ingest_idempotent_noop_event(
    record: &MarkdownIngestQueueRecord,
    manifest: &SourceArtifactManifest,
    snapshot: &BrainRepoSnapshot,
) -> Result<BrainEvent> {
    let snapshot_id = format!(
        "snapshot-{}-{}",
        snapshot.workspace_id, snapshot.generated_at
    );
    let evidence_refs = snapshot
        .evidence
        .iter()
        .filter(|evidence| evidence.source_id.as_deref() == Some(manifest.source_id.as_str()))
        .map(|evidence| evidence.id.clone())
        .collect::<BTreeSet<_>>();
    Ok(BrainEvent {
        event_id: format!("evt-{}", Uuid::now_v7()),
        schema_version: BRAIN_EVENT_SCHEMA_VERSION,
        workspace_id: record.workspace_id.clone(),
        scope: BrainScope::Project,
        event_type: BrainEventKind::GraphMaterialized,
        operation_type: Some("graph_materialize_noop".into()),
        actor: BrainActor {
            actor_type: BrainActorType::Agent,
            actor_id: "hyprduck-agent-ingest".into(),
        },
        source_refs: vec![manifest.source_id.clone()],
        source_markdown_refs: vec![record.relative_path.clone(), manifest.markdown_path.clone()],
        node_refs: snapshot
            .nodes
            .iter()
            .filter(|node| node.source_ids.contains(&manifest.source_id))
            .map(|node| node.node_id.clone())
            .collect(),
        relation_refs: Vec::new(),
        claim_refs: snapshot
            .claims
            .iter()
            .filter(|claim| claim.source_refs.contains(&manifest.source_id))
            .map(|claim| claim.claim_id.clone())
            .collect(),
        memory_refs: snapshot
            .memories
            .iter()
            .filter(|memory| memory.source_refs.contains(&manifest.source_id))
            .map(|memory| memory.memory_id.clone())
            .collect(),
        target_node_ids: Vec::new(),
        target_edge_ids: Vec::new(),
        target_claim_ids: Vec::new(),
        target_memory_ids: Vec::new(),
        evidence_refs: evidence_refs.into_iter().collect(),
        payload_json: serde_json::to_string(&json!({
            "mutationType": "noop",
            "result": "idempotent",
            "reason": "markdown_source_already_materialized",
            "sourceId": manifest.source_id,
            "sourcePath": record.relative_path,
            "contentHash": record.content_hash,
            "snapshotId": snapshot_id,
            "diff": {
                "changedFiles": [],
                "nodeChanges": [],
                "edgeChanges": [],
                "claimChanges": [],
                "memoryChanges": [],
                "wikiChanges": []
            },
            "rollbackHint": "No materialized graph/wiki files changed; replay remains anchored by events/brain_events.jsonl."
        }))
        .context("failed to encode idempotent markdown ingest event payload")?,
        causality: BrainEventCausality {
            caused_by_source_ids: vec![manifest.source_id.clone()],
            snapshot_id: Some(snapshot_id),
            materialized_version: Some(snapshot.generated_at),
            ..Default::default()
        },
        confidence: None,
        policy_result: "idempotent_noop".into(),
        created_at: record.completed_at.unwrap_or_else(unix_timestamp_seconds),
    })
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct MarkdownIngestPaths {
    workspace_root: PathBuf,
    source_dir: PathBuf,
    wiki_dir: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct NewMarkdownSource {
    source_path: PathBuf,
    pub(crate) relative_path: PathBuf,
    size_bytes: u64,
    modified_at: Option<u64>,
    content_hash: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct MarkdownSourceStateFile {
    generated_at: u64,
    #[serde(default)]
    sources: Vec<MarkdownSourceStateRecord>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct MarkdownSourceStateRecord {
    source_path: String,
    relative_path: String,
    normalized_path: String,
    size_bytes: u64,
    #[serde(default)]
    modified_at: Option<u64>,
    content_hash: String,
    first_seen_at: u64,
    last_seen_at: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct MarkdownSourceScan {
    pub(crate) new_sources: Vec<NewMarkdownSource>,
    pub(crate) current_state: MarkdownSourceStateFile,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct MarkdownIngestQueueFile {
    generated_at: u64,
    #[serde(default)]
    records: Vec<MarkdownIngestQueueRecord>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct MarkdownIngestQueueRecord {
    queue_id: String,
    workspace_id: String,
    source_path: String,
    pub(crate) relative_path: String,
    normalized_path: String,
    size_bytes: u64,
    #[serde(default)]
    modified_at: Option<u64>,
    content_hash: String,
    status: String,
    #[serde(default = "default_markdown_trigger_status")]
    trigger_status: String,
    #[serde(default)]
    trigger_error_message: Option<String>,
    discovered_at: u64,
    enqueued_at: u64,
    #[serde(default)]
    started_at: Option<u64>,
    #[serde(default)]
    completed_at: Option<u64>,
    #[serde(default)]
    error_message: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct MarkdownEnqueueResult {
    pub(crate) enqueued: Vec<MarkdownIngestQueueRecord>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct MarkdownIngestWorkerResult {
    pub(crate) started: bool,
    pub(crate) processed: usize,
    pub(crate) failed: usize,
    pub(crate) processed_sources: Vec<String>,
    pub(crate) failed_sources: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct CompletedMarkdownSourceMetadata {
    workspace_id: String,
    queue_id: String,
    source_id: String,
    source_path: String,
    relative_path: String,
    normalized_path: String,
    markdown_path: String,
    manifest_path: String,
    artifact_root: String,
    size_bytes: u64,
    modified_at: Option<u64>,
    content_hash: String,
    discovered_at: u64,
    enqueued_at: u64,
    started_at: Option<u64>,
    completed_at: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct CompletedMarkdownIngest {
    record: MarkdownIngestQueueRecord,
    manifest: SourceArtifactManifest,
    source_metadata: CompletedMarkdownSourceMetadata,
}

impl CompletedMarkdownIngest {
    fn new(record: &MarkdownIngestQueueRecord, manifest: &SourceArtifactManifest) -> Self {
        Self {
            record: record.clone(),
            manifest: manifest.clone(),
            source_metadata: CompletedMarkdownSourceMetadata {
                workspace_id: record.workspace_id.clone(),
                queue_id: record.queue_id.clone(),
                source_id: manifest.source_id.clone(),
                source_path: record.source_path.clone(),
                relative_path: record.relative_path.clone(),
                normalized_path: record.normalized_path.clone(),
                markdown_path: manifest.markdown_path.clone(),
                manifest_path: manifest.manifest_path.clone(),
                artifact_root: manifest.artifact_root.clone(),
                size_bytes: record.size_bytes,
                modified_at: record.modified_at,
                content_hash: record.content_hash.clone(),
                discovered_at: record.discovered_at,
                enqueued_at: record.enqueued_at,
                started_at: record.started_at,
                completed_at: record.completed_at,
            },
        }
    }
}
fn default_markdown_trigger_status() -> String {
    "accepted".into()
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct MarkdownSourceFileState {
    source_path: PathBuf,
    relative_path: PathBuf,
    normalized_absolute_path: String,
    normalized_relative_path: String,
    size_bytes: u64,
    modified_at: Option<u64>,
    content_hash: String,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
struct BrainWorkspaceConfig {
    #[serde(default, alias = "markdown_sources_dir")]
    markdown_sources_dir: Option<String>,
    #[serde(default, alias = "wiki_dir")]
    wiki_dir: Option<String>,
}

pub(crate) fn resolve_markdown_ingest_paths(scope: &BrainReadScope) -> Result<MarkdownIngestPaths> {
    let workspace_root = resolve_brain_workspace_root(scope)?;
    let config = read_brain_workspace_config(&workspace_root)?;
    let configured_source_dir = config.markdown_sources_dir.as_deref();
    let source_dir =
        resolve_workspace_config_path(&workspace_root, configured_source_dir, "sources")?;
    let wiki_dir =
        resolve_workspace_config_path(&workspace_root, config.wiki_dir.as_deref(), "wiki")?;

    if configured_source_dir.is_some() && !source_dir.is_dir() {
        bail!(
            "configured markdown source directory {} does not exist or is not a directory",
            source_dir.display()
        );
    }
    if configured_source_dir.is_none() {
        fs::create_dir_all(&source_dir).with_context(|| {
            format!(
                "failed creating markdown source directory {}",
                source_dir.display()
            )
        })?;
    }
    fs::create_dir_all(&wiki_dir)
        .with_context(|| format!("failed creating wiki directory {}", wiki_dir.display()))?;

    Ok(MarkdownIngestPaths {
        workspace_root,
        source_dir,
        wiki_dir,
    })
}

pub(crate) fn scan_new_markdown_sources(
    paths: &MarkdownIngestPaths,
    snapshot: &BrainRepoSnapshot,
    source_state: &MarkdownSourceStateFile,
    ingest_queue: &MarkdownIngestQueueFile,
) -> Result<MarkdownSourceScan> {
    let mut known_paths = BTreeSet::new();
    for source in &snapshot.sources {
        for raw_path in [
            source.original_path.as_str(),
            source.source_path.as_str(),
            source.markdown_path.as_str(),
        ] {
            if !raw_path.trim().is_empty() {
                known_paths.insert(normalize_ingest_path_for_compare(raw_path));
            }
        }
    }
    let previous_state_by_path = source_state_by_path(source_state);
    for record in previous_state_by_path.values() {
        known_paths.insert(record.normalized_path.clone());
        known_paths.insert(normalize_ingest_path_for_compare(&record.relative_path));
        known_paths.insert(normalize_ingest_path_for_compare(&record.source_path));
    }
    for record in &ingest_queue.records {
        known_paths.insert(record.normalized_path.clone());
        known_paths.insert(normalize_ingest_path_for_compare(&record.relative_path));
        known_paths.insert(normalize_ingest_path_for_compare(&record.source_path));
    }
    for event in &snapshot.events {
        if event.event_type != BrainEventKind::SourceIngestQueued {
            continue;
        }
        if let Ok(record) = serde_json::from_str::<MarkdownIngestQueueRecord>(&event.payload_json) {
            known_paths.insert(record.normalized_path);
            known_paths.insert(normalize_ingest_path_for_compare(&record.relative_path));
            known_paths.insert(normalize_ingest_path_for_compare(&record.source_path));
        }
    }

    let mut file_states = Vec::new();
    collect_markdown_source_file_states(&paths.source_dir, &paths.source_dir, &mut file_states)?;
    file_states.sort_by(|left, right| left.relative_path.cmp(&right.relative_path));

    let mut new_sources = file_states
        .iter()
        .filter(|file| {
            !known_paths.contains(&file.normalized_absolute_path)
                && !known_paths.contains(&file.normalized_relative_path)
        })
        .map(|file| NewMarkdownSource {
            source_path: file.source_path.clone(),
            relative_path: file.relative_path.clone(),
            size_bytes: file.size_bytes,
            modified_at: file.modified_at,
            content_hash: file.content_hash.clone(),
        })
        .collect::<Vec<_>>();
    new_sources.sort_by(|left, right| left.relative_path.cmp(&right.relative_path));

    let generated_at = unix_timestamp_seconds();
    let current_state = MarkdownSourceStateFile {
        generated_at,
        sources: file_states
            .into_iter()
            .map(|file| {
                let previous = previous_state_by_path
                    .get(&file.normalized_relative_path)
                    .or_else(|| previous_state_by_path.get(&file.normalized_absolute_path));
                MarkdownSourceStateRecord {
                    source_path: file.source_path.display().to_string(),
                    relative_path: file.relative_path.display().to_string(),
                    normalized_path: file.normalized_relative_path,
                    size_bytes: file.size_bytes,
                    modified_at: file.modified_at,
                    content_hash: file.content_hash,
                    first_seen_at: previous
                        .map(|record| record.first_seen_at)
                        .unwrap_or(generated_at),
                    last_seen_at: generated_at,
                }
            })
            .collect(),
    };

    Ok(MarkdownSourceScan {
        new_sources,
        current_state,
    })
}

pub(crate) fn enqueue_markdown_sources(
    writer: &BrainWorkspaceWriter,
    paths: &MarkdownIngestPaths,
    ingest_queue: &MarkdownIngestQueueFile,
    scan: &MarkdownSourceScan,
) -> Result<MarkdownEnqueueResult> {
    if scan.new_sources.is_empty() {
        write_markdown_ingest_queue(paths, ingest_queue)?;
        return Ok(MarkdownEnqueueResult::default());
    }

    let mut records = ingest_queue.records.clone();
    let mut queued_keys = records
        .iter()
        .map(markdown_queue_dedupe_key)
        .collect::<BTreeSet<_>>();
    let mut enqueued = Vec::new();
    let enqueued_at = unix_timestamp_seconds();

    for source in &scan.new_sources {
        let normalized_path =
            normalize_ingest_path_for_compare(&source.relative_path.display().to_string());
        let key = format!("{}:{}", normalized_path, source.content_hash);
        if queued_keys.contains(&key) {
            continue;
        }
        let record = MarkdownIngestQueueRecord {
            queue_id: format!("markdown-source-{}", sanitize_name(&key)),
            workspace_id: paths
                .workspace_root
                .file_name()
                .map(|name| name.to_string_lossy().to_string())
                .unwrap_or_else(|| DEFAULT_WORKSPACE_ID.into()),
            source_path: source.source_path.display().to_string(),
            relative_path: source.relative_path.display().to_string(),
            normalized_path,
            size_bytes: source.size_bytes,
            modified_at: source.modified_at,
            content_hash: source.content_hash.clone(),
            status: "queued".into(),
            trigger_status: "accepted".into(),
            trigger_error_message: None,
            discovered_at: enqueued_at,
            enqueued_at,
            started_at: None,
            completed_at: None,
            error_message: None,
        };
        writer.append_event(&markdown_source_queued_event(
            &record.workspace_id,
            &record,
        )?)?;
        queued_keys.insert(markdown_queue_dedupe_key(&record));
        records.push(record.clone());
        enqueued.push(record);
    }

    records.sort_by(|left, right| {
        left.relative_path
            .cmp(&right.relative_path)
            .then_with(|| left.queue_id.cmp(&right.queue_id))
    });
    write_markdown_ingest_queue(
        paths,
        &MarkdownIngestQueueFile {
            generated_at: enqueued_at,
            records,
        },
    )?;

    Ok(MarkdownEnqueueResult { enqueued })
}

pub(crate) fn run_markdown_ingest_worker(
    paths: &MarkdownIngestPaths,
    ingest_queue: &MarkdownIngestQueueFile,
    store: &KnowledgeProjectStore,
) -> Result<MarkdownIngestWorkerResult> {
    run_markdown_ingest_worker_with_post_ingest_hook(paths, ingest_queue, store, &mut |_| Ok(()))
}

fn run_markdown_ingest_worker_with_post_ingest_hook(
    paths: &MarkdownIngestPaths,
    ingest_queue: &MarkdownIngestQueueFile,
    store: &KnowledgeProjectStore,
    post_ingest_hook: &mut dyn FnMut(&CompletedMarkdownIngest) -> Result<()>,
) -> Result<MarkdownIngestWorkerResult> {
    let queued_indexes = ingest_queue
        .records
        .iter()
        .enumerate()
        .filter_map(|(index, record)| (record.status == "queued").then_some(index))
        .collect::<Vec<_>>();
    if queued_indexes.is_empty() {
        return Ok(MarkdownIngestWorkerResult::default());
    }

    let mut queue = ingest_queue.clone();
    let mut result = MarkdownIngestWorkerResult {
        started: true,
        ..MarkdownIngestWorkerResult::default()
    };

    for index in queued_indexes {
        let started_at = unix_timestamp_seconds();
        queue.records[index].status = "ingesting".into();
        queue.records[index].started_at = Some(started_at);
        queue.records[index].error_message = None;
        write_markdown_ingest_queue(paths, &queue)?;

        let compile_result = compile_queued_markdown_source(paths, &queue.records[index], store);
        let completed_at = unix_timestamp_seconds();
        queue.records[index].completed_at = Some(completed_at);
        match compile_result {
            Ok(manifest) => {
                queue.records[index].status = "ingested".into();
                queue.records[index].error_message = None;
                result.processed += 1;
                result
                    .processed_sources
                    .push(queue.records[index].relative_path.clone());
                let completed_ingest =
                    CompletedMarkdownIngest::new(&queue.records[index], &manifest);
                post_ingest_hook(&completed_ingest).with_context(|| {
                    format!(
                        "post-ingest hook failed for markdown source {}",
                        queue.records[index].relative_path
                    )
                })?;
                let writer = BrainWorkspaceWriter::open(paths.workspace_root.clone())?;
                writer.append_event(&markdown_source_compiled_event(
                    &queue.records[index],
                    Some(&manifest),
                )?)?;
            }
            Err(error) => {
                queue.records[index].status = "failed".into();
                queue.records[index].error_message = Some(error.to_string());
                result.failed += 1;
                result
                    .failed_sources
                    .push(queue.records[index].relative_path.clone());
                let writer = BrainWorkspaceWriter::open(paths.workspace_root.clone())?;
                writer.append_event(&markdown_source_compiled_event(
                    &queue.records[index],
                    None,
                )?)?;
            }
        }
        write_markdown_ingest_queue(paths, &queue)?;
    }

    Ok(result)
}

fn compile_queued_markdown_source(
    paths: &MarkdownIngestPaths,
    record: &MarkdownIngestQueueRecord,
    store: &KnowledgeProjectStore,
) -> Result<SourceArtifactManifest> {
    let source_path = PathBuf::from(&record.source_path);
    let markdown = fs::read_to_string(&source_path).with_context(|| {
        format!(
            "failed reading queued markdown source {}",
            source_path.display()
        )
    })?;
    let source_id = build_source_id(
        &format!("{}:{}", record.normalized_path, record.content_hash),
        0,
    );
    if let Some(manifest) = read_idempotent_markdown_ingest_manifest(paths, record, &source_id)? {
        let snapshot = read_json_artifact::<BrainRepoSnapshot>(
            &paths.workspace_root.join("brain-manifest.json"),
        )?;
        let writer = BrainWorkspaceWriter::open(paths.workspace_root.clone())?;
        writer.append_event(&markdown_ingest_idempotent_noop_event(
            record, &manifest, &snapshot,
        )?)?;
        return Ok(manifest);
    }
    let safe_name = sanitize_name(
        source_path
            .file_stem()
            .and_then(|value| value.to_str())
            .unwrap_or("markdown-source"),
    );
    let artifact_root = paths.workspace_root.join("artifacts").join(&source_id);
    let pages_dir = artifact_root.join("pages");
    fs::create_dir_all(&pages_dir)
        .with_context(|| format!("failed creating {}", pages_dir.display()))?;
    let markdown_path = artifact_root.join(format!("{safe_name}.md"));
    let page_markdown_path = pages_dir.join("page_1.md");
    write_file_atomic(&markdown_path, markdown.as_bytes())?;
    write_file_atomic(&page_markdown_path, markdown.as_bytes())?;

    let now = unix_timestamp_seconds();
    let manifest_path = artifact_root.join("source-manifest.json");
    let manifest = SourceArtifactManifest {
        workspace_id: record.workspace_id.clone(),
        source_id: source_id.clone(),
        original_path: record.source_path.clone(),
        source_path: record.source_path.clone(),
        markdown_path: markdown_path.display().to_string(),
        artifact_root: artifact_root.display().to_string(),
        manifest_path: manifest_path.display().to_string(),
        format: DocumentFormat::Markdown,
        output_name: safe_name,
        status: IngestStatus::Ingested,
        description: format!("Markdown source ingested from {}", record.relative_path),
        user_context: String::new(),
        ingest_instruction: "Autonomous markdown source ingest loop".into(),
        pages: vec![PageArtifact {
            index: 0,
            label: "Page 1".into(),
            image_path: None,
            markdown_path: Some(page_markdown_path.display().to_string()),
            plain_text_path: None,
            error_message: None,
        }],
        created_at: record.discovered_at,
        updated_at: now,
    };
    write_source_manifest(&manifest)?;
    let chunks = chunk_source_markdown(&manifest, &markdown);
    upsert_source_chunks(&paths.workspace_root, &manifest, &chunks)?;

    let request = CompileProjectRequest {
        source_markdown_path: markdown_path.display().to_string(),
        source_document_path: Some(record.source_path.clone()),
        source_manifest_path: Some(manifest.manifest_path.clone()),
        workspace_id: Some(record.workspace_id.clone()),
        source_id: Some(source_id.clone()),
        skip_graph_generation: None,
    };
    let project = compile_knowledge_project(&request, &markdown, Some(&manifest));
    store.save_project(&project, &request, Some(&manifest))?;
    let snapshot = read_materialized_brain_snapshot(&paths.workspace_root, &record.workspace_id)
        .unwrap_or_else(|_| empty_replayed_brain_snapshot(&record.workspace_id));
    let context = build_import_evidence_context(
        &paths.workspace_root,
        &manifest,
        &markdown,
        &snapshot,
        &chunks,
    )?;
    maybe_generate_provider_graph_materialization(
        &paths.workspace_root,
        &record.workspace_id,
        &manifest,
        &markdown,
        &artifact_root,
        &context,
    )?;
    Ok(manifest)
}

fn read_idempotent_markdown_ingest_manifest(
    paths: &MarkdownIngestPaths,
    record: &MarkdownIngestQueueRecord,
    source_id: &str,
) -> Result<Option<SourceArtifactManifest>> {
    let manifest_path = paths.workspace_root.join("brain-manifest.json");
    if !manifest_path.exists() || !materialized_markdown_source_files_exist(paths, source_id) {
        return Ok(None);
    }

    let snapshot: BrainRepoSnapshot = read_json_artifact(&manifest_path)?;
    let Some(source) = snapshot.sources.iter().find(|source| {
        source.source_id == source_id
            && source.workspace_id == record.workspace_id
            && source.original_path == record.source_path
            && source.source_path == record.source_path
            && source.status == "ingested"
    }) else {
        return Ok(None);
    };
    if !snapshot
        .wiki_pages
        .iter()
        .any(|page| page.source_refs.contains(&source.source_id))
    {
        return Ok(None);
    }

    let source_manifest_path = paths
        .workspace_root
        .join("artifacts")
        .join(source_id)
        .join("source-manifest.json");
    let source_manifest: SourceArtifactManifest = read_json_artifact(&source_manifest_path)?;
    if source_manifest.source_id != source.source_id
        || source_manifest.workspace_id != record.workspace_id
        || source_manifest.source_path != record.source_path
    {
        return Ok(None);
    }
    Ok(Some(source_manifest))
}

fn materialized_markdown_source_files_exist(paths: &MarkdownIngestPaths, source_id: &str) -> bool {
    [
        paths.workspace_root.join("brain-manifest.json"),
        paths.workspace_root.join("graph/nodes.json"),
        paths.workspace_root.join("graph/edges.json"),
        paths.workspace_root.join("graph/claims.json"),
        paths.workspace_root.join("memory/records.json"),
        paths.workspace_root.join("wiki/index.md"),
        paths
            .workspace_root
            .join("artifacts")
            .join(source_id)
            .join("source-manifest.json"),
    ]
    .iter()
    .all(|path| path.exists())
}

fn collect_markdown_source_file_states(
    root: &Path,
    dir: &Path,
    sources: &mut Vec<MarkdownSourceFileState>,
) -> Result<()> {
    let entries = fs::read_dir(dir).with_context(|| format!("failed reading {}", dir.display()))?;
    for entry in entries {
        let entry = entry.with_context(|| format!("failed reading entry in {}", dir.display()))?;
        let path = entry.path();
        let file_type = entry
            .file_type()
            .with_context(|| format!("failed reading metadata for {}", path.display()))?;
        if file_type.is_dir() {
            collect_markdown_source_file_states(root, &path, sources)?;
            continue;
        }
        if !file_type.is_file() || !is_markdown_source_path(&path) {
            continue;
        }
        let relative_path = path
            .strip_prefix(root)
            .with_context(|| {
                format!(
                    "failed deriving markdown source path {} relative to {}",
                    path.display(),
                    root.display()
                )
            })?
            .to_path_buf();
        let metadata = fs::metadata(&path)
            .with_context(|| format!("failed reading metadata for {}", path.display()))?;
        let modified_at = metadata
            .modified()
            .ok()
            .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|duration| duration.as_secs());
        let contents =
            fs::read(&path).with_context(|| format!("failed reading {}", path.display()))?;
        sources.push(MarkdownSourceFileState {
            normalized_absolute_path: normalize_ingest_path_for_compare(
                &path.display().to_string(),
            ),
            normalized_relative_path: normalize_ingest_path_for_compare(
                &relative_path.display().to_string(),
            ),
            source_path: path,
            relative_path,
            size_bytes: metadata.len(),
            modified_at,
            content_hash: format!("{:016x}", fnv1a_hash(&contents)),
        });
    }
    Ok(())
}

pub(crate) fn read_markdown_source_state(
    paths: &MarkdownIngestPaths,
) -> Result<MarkdownSourceStateFile> {
    read_optional_json_artifact(&paths.workspace_root.join(MARKDOWN_SOURCE_STATE_PATH))
}

pub(crate) fn write_markdown_source_state(
    paths: &MarkdownIngestPaths,
    state: &MarkdownSourceStateFile,
) -> Result<()> {
    write_json_pretty(
        &paths.workspace_root.join(MARKDOWN_SOURCE_STATE_PATH),
        state,
    )
}

pub(crate) fn read_markdown_ingest_queue(
    paths: &MarkdownIngestPaths,
) -> Result<MarkdownIngestQueueFile> {
    read_optional_json_artifact(&paths.workspace_root.join(MARKDOWN_INGEST_QUEUE_PATH))
}

fn write_markdown_ingest_queue(
    paths: &MarkdownIngestPaths,
    queue: &MarkdownIngestQueueFile,
) -> Result<()> {
    write_json_pretty(
        &paths.workspace_root.join(MARKDOWN_INGEST_QUEUE_PATH),
        queue,
    )
}

fn markdown_queue_dedupe_key(record: &MarkdownIngestQueueRecord) -> String {
    format!("{}:{}", record.normalized_path, record.content_hash)
}

fn source_state_by_path(
    state: &MarkdownSourceStateFile,
) -> BTreeMap<String, MarkdownSourceStateRecord> {
    let mut by_path = BTreeMap::new();
    for record in &state.sources {
        by_path.insert(record.normalized_path.clone(), record.clone());
        by_path.insert(
            normalize_ingest_path_for_compare(&record.source_path),
            record.clone(),
        );
        by_path.insert(
            normalize_ingest_path_for_compare(&record.relative_path),
            record.clone(),
        );
    }
    by_path
}

fn is_markdown_source_path(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .map(|extension| matches!(extension.to_ascii_lowercase().as_str(), "md" | "markdown"))
        .unwrap_or(false)
}

fn normalize_ingest_path_for_compare(path: &str) -> String {
    let path = PathBuf::from(path);
    path.components()
        .collect::<PathBuf>()
        .to_string_lossy()
        .replace('\\', "/")
}

fn read_brain_workspace_config(workspace_root: &Path) -> Result<BrainWorkspaceConfig> {
    let path = workspace_root.join("brain-config.json");
    if !path.exists() {
        return Ok(BrainWorkspaceConfig::default());
    }
    read_json_artifact(&path)
}

fn resolve_workspace_config_path(
    workspace_root: &Path,
    configured: Option<&str>,
    fallback: &str,
) -> Result<PathBuf> {
    let raw = configured.unwrap_or(fallback).trim();
    if raw.is_empty() {
        bail!("configured workspace path cannot be empty");
    }
    let path = PathBuf::from(raw);
    if path
        .components()
        .any(|component| matches!(component, std::path::Component::ParentDir))
    {
        bail!("configured workspace path cannot contain ..");
    }
    if path.is_absolute() {
        Ok(path)
    } else {
        Ok(workspace_root.join(path))
    }
}
