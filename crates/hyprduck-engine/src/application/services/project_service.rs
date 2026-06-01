use crate::*;

pub(crate) fn handle_compile_project(
    request: CompileProjectRequest,
) -> Result<CompileProjectResponseData> {
    let markdown = fs::read_to_string(&request.source_markdown_path).with_context(|| {
        format!(
            "failed reading markdown package {}",
            request.source_markdown_path
        )
    })?;
    let source_manifest = load_source_manifest(&request)?;
    let (workspace_id, source_id) = resolved_source_ids(&request, source_manifest.as_ref())?;
    let project = compile_knowledge_project(&request, &markdown, source_manifest.as_ref());
    let store = KnowledgeProjectStore::default()?;
    store.save_project(&project, &request, source_manifest.as_ref())?;
    let mut graph_generation_status = None;
    let mut graph_generation_skipped_reason = None;
    let mut graph_generation_error_message = None;
    let mut graph_generation_retryable = None;
    let mut graph_generation_failed_reason = None;
    let mut graph_generation_stage = None;
    if let Some(manifest) = &source_manifest {
        let workspace_root = compile_workspace_root(manifest, &workspace_id)?;
        let chunks = chunk_source_markdown(manifest, &markdown);
        upsert_source_chunks(&workspace_root, manifest, &chunks)?;
    }
    if let Some(manifest) = source_manifest
        .as_ref()
        .filter(|_| !request.skip_graph_generation.unwrap_or(false))
    {
        let workspace_root = compile_workspace_root(manifest, &workspace_id)?;
        let chunks = chunk_source_markdown(manifest, &markdown);
        let snapshot = read_materialized_brain_snapshot(&workspace_root, &workspace_id)
            .unwrap_or_else(|_| empty_replayed_brain_snapshot(&workspace_id));
        let context = build_import_evidence_context(
            &workspace_root,
            manifest,
            &markdown,
            &snapshot,
            &chunks,
        )?;
        let report = maybe_generate_provider_graph_materialization(
            &workspace_root,
            &workspace_id,
            manifest,
            &markdown,
            &PathBuf::from(&manifest.artifact_root),
            &context,
        )?;
        graph_generation_status = Some(report.status);
        graph_generation_skipped_reason = report.skipped_reason;
        graph_generation_error_message = report.error_message;
        graph_generation_retryable = Some(report.retryable);
        graph_generation_failed_reason = report.failed_reason;
        graph_generation_stage = Some(report.stage);
        store.update_import_job_graph_status(
            &workspace_id,
            &manifest.source_id,
            graph_generation_status.as_deref().unwrap_or("unknown"),
            graph_generation_failed_reason.as_deref(),
            graph_generation_error_message.as_deref(),
            graph_generation_retryable.unwrap_or(false),
        )?;
    }
    Ok(CompileProjectResponseData {
        project_id: project.summary.project_id,
        workspace_id,
        source_id,
        graph_generation_status,
        graph_generation_skipped_reason,
        graph_generation_error_message,
        graph_generation_retryable,
        graph_generation_failed_reason,
        graph_generation_stage,
    })
}

fn compile_workspace_root(
    manifest: &SourceArtifactManifest,
    workspace_id: &str,
) -> Result<PathBuf> {
    if let Some(root) =
        workspace_root_from_path_segments(&manifest.source_path, "sources", &manifest.source_id)
            .or_else(|| {
                workspace_root_from_path_segments(
                    &manifest.markdown_path,
                    "artifacts",
                    &manifest.source_id,
                )
            })
    {
        return Ok(root);
    }
    resolve_brain_workspace_root(&BrainReadScope {
        workspace_id: workspace_id.into(),
        root_dir: None,
    })
}

pub(crate) fn handle_read_import_job(
    request: ReadImportJobRequest,
) -> Result<ReadImportJobResponseData> {
    if request.job_id.is_none() && request.source_id.is_none() {
        bail!("read_import_job requires jobId or sourceId");
    }
    let root = resolve_brain_workspace_root(&request.scope)?;
    let store = KnowledgeStore::open(KnowledgeStore::default_path_for_root(&root))?;
    let job = store.read_import_job(
        &request.scope.workspace_id,
        request.job_id.as_deref(),
        request.source_id.as_deref(),
    )?;
    if job.is_some() {
        return Ok(ReadImportJobResponseData { job });
    }
    let project_store = KnowledgeProjectStore::default()?;
    if project_store.path == KnowledgeStore::default_path_for_root(&root) {
        return Ok(ReadImportJobResponseData { job: None });
    }
    let project_knowledge_store = KnowledgeStore::open(project_store.path)?;
    Ok(ReadImportJobResponseData {
        job: project_knowledge_store.read_import_job(
            &request.scope.workspace_id,
            request.job_id.as_deref(),
            request.source_id.as_deref(),
        )?,
    })
}

pub(crate) fn handle_update_import_job_graph_status(
    request: UpdateImportJobGraphStatusRequest,
) -> Result<UpdateImportJobGraphStatusResponseData> {
    let root = resolve_brain_workspace_root(&request.scope)?;
    let root_path = KnowledgeStore::default_path_for_root(&root);
    let root_store = KnowledgeStore::open(root_path.clone())?;
    let mut updated = root_store.update_import_job_graph_status_from_mcp(
        &request.scope.workspace_id,
        &request.source_id,
        &request.status,
        &request.graph_status,
        request.graph_error_category.as_deref(),
        request.graph_error_message_redacted.as_deref(),
        request.graph_retryable,
        request.graph_retry_attempt,
        request.graph_max_retry_attempts,
        request.graph_next_retry_at,
        request.manual_retry_available,
    )?;
    if !updated {
        let project_store = KnowledgeProjectStore::default()?;
        if project_store.path != root_path {
            let project_knowledge_store = KnowledgeStore::open(project_store.path)?;
            updated = project_knowledge_store.update_import_job_graph_status_from_mcp(
                &request.scope.workspace_id,
                &request.source_id,
                &request.status,
                &request.graph_status,
                request.graph_error_category.as_deref(),
                request.graph_error_message_redacted.as_deref(),
                request.graph_retryable,
                request.graph_retry_attempt,
                request.graph_max_retry_attempts,
                request.graph_next_retry_at,
                request.manual_retry_available,
            )?;
        }
    }
    Ok(UpdateImportJobGraphStatusResponseData { updated })
}

pub(crate) fn handle_load_project(request: LoadProjectRequest) -> Result<LoadProjectResponseData> {
    let store = KnowledgeProjectStore::default()?;
    if let Some(project_id) = request.project_id.as_deref() {
        if let Some(workspace_id) = project_id.strip_prefix("workspace:") {
            let project = store.load_workspace_project(workspace_id)?;
            let sources = store.load_sources(workspace_id)?;
            return Ok(LoadProjectResponseData {
                project,
                workspace_id: Some(workspace_id.to_string()),
                sources,
            });
        }

        let project = store
            .load_project(Some(project_id))?
            .map(source_ui_graph_projection);
        let stored_workspace_id = store.load_workspace_id_for_project(project_id)?;
        if let (Some(request_workspace_id), Some(actual_workspace_id)) = (
            request.workspace_id.as_deref(),
            stored_workspace_id.as_deref(),
        ) {
            if request_workspace_id != actual_workspace_id {
                bail!(
                    "project {project_id} belongs to workspace {actual_workspace_id}, not {request_workspace_id}"
                );
            }
        }
        let workspace_id = stored_workspace_id.or(request.workspace_id.clone());
        let sources = workspace_id
            .as_deref()
            .map(|workspace_id| store.load_sources(workspace_id))
            .transpose()?
            .unwrap_or_default();
        return Ok(LoadProjectResponseData {
            project,
            workspace_id,
            sources,
        });
    }

    let workspace_id = match request.workspace_id.clone() {
        Some(workspace_id) => Some(workspace_id),
        None => store.load_latest_workspace_id()?,
    };
    let mut project = workspace_id
        .as_deref()
        .map(|workspace_id| store.load_workspace_project(workspace_id))
        .transpose()?
        .flatten();
    if project.is_none() && request.workspace_id.is_none() {
        project = store.load_project(None)?.map(source_ui_graph_projection);
    }
    let sources = workspace_id
        .as_deref()
        .map(|workspace_id| store.load_sources(workspace_id))
        .transpose()?
        .unwrap_or_default();
    Ok(LoadProjectResponseData {
        project,
        workspace_id,
        sources,
    })
}

pub(crate) fn handle_apply_correction(
    request: ApplyCorrectionRequest,
) -> Result<ApplyCorrectionResponseData> {
    let store = KnowledgeProjectStore::default()?;
    if let Some(workspace_id) = workspace_id_from_project_id(&request.project_id) {
        return handle_apply_workspace_correction(&store, workspace_id, &request);
    }

    let mut project = store
        .load_project(Some(&request.project_id))?
        .ok_or_else(|| anyhow!("project {} was not found", request.project_id))?;
    apply_correction(&mut project, &request)?;
    store.update_project(&project)?;
    if let Some(workspace_id) = store.load_workspace_id_for_project(&project.summary.project_id)? {
        store.materialize_workspace_brain_repo(&workspace_id)?;
    }
    Ok(ApplyCorrectionResponseData { project })
}

fn handle_apply_workspace_correction(
    store: &KnowledgeProjectStore,
    workspace_id: &str,
    request: &ApplyCorrectionRequest,
) -> Result<ApplyCorrectionResponseData> {
    let rows = store.load_projects_for_workspace(workspace_id)?;
    if rows.is_empty() {
        if request.kind == CorrectionKind::Delete {
            return handle_delete_materialized_workspace_node(store, workspace_id, &rows, request);
        }
        bail!("workspace {workspace_id} was not found");
    }

    let aggregate = aggregate_workspace_project(workspace_id, rows.clone());
    let selected_detail = match aggregate.details_by_node_id.get(&request.node_id) {
        Some(detail) => detail,
        None if request.kind == CorrectionKind::Delete => {
            return handle_delete_materialized_workspace_node(store, workspace_id, &rows, request);
        }
        None => bail!("workspace node {} was not found", request.node_id),
    };
    if request.kind == CorrectionKind::Delete && is_source_like_node_kind(selected_detail.node.kind)
    {
        return handle_delete_workspace_source_node(store, workspace_id, selected_detail, request);
    }
    if selected_detail.node.kind != GraphNodeKind::Concept {
        bail!("workspace corrections only support concept nodes");
    }
    let target_detail = match request.kind {
        CorrectionKind::Merge => {
            let target_node_id = request
                .target_node_id
                .as_deref()
                .ok_or_else(|| anyhow!("merge needs a target concept"))?;
            let detail = aggregate
                .details_by_node_id
                .get(target_node_id)
                .ok_or_else(|| anyhow!("workspace target node {target_node_id} was not found"))?;
            if detail.node.kind != GraphNodeKind::Concept {
                bail!("merge only supports concept nodes");
            }
            Some(detail)
        }
        CorrectionKind::KeepSeparate
        | CorrectionKind::Rename
        | CorrectionKind::Split
        | CorrectionKind::Delete => None,
    };

    let mut replayed_source_node_ids = BTreeSet::new();
    let mut changed_projects = Vec::new();
    for (row, project) in rows {
        let Some(mut project) = project else {
            continue;
        };
        let selected_source_node_ids = matching_source_concept_node_ids(&project, selected_detail);
        if selected_source_node_ids.is_empty() {
            continue;
        }

        let target_source_node_id = target_detail.and_then(|detail| {
            matching_source_concept_node_ids(&project, detail)
                .into_iter()
                .find(|node_id| !selected_source_node_ids.contains(node_id))
        });
        if request.kind == CorrectionKind::Merge && target_source_node_id.is_none() {
            continue;
        }

        let mut changed = false;
        for source_node_id in selected_source_node_ids {
            let source_request = ApplyCorrectionRequest {
                project_id: row.project_id.clone(),
                node_id: source_node_id.clone(),
                kind: request.kind.clone(),
                target_node_id: target_source_node_id.clone(),
                value: request.value.clone(),
            };
            apply_correction(&mut project, &source_request)?;
            replayed_source_node_ids.insert(format!("{}:{source_node_id}", row.project_id));
            changed = true;
            if request.kind == CorrectionKind::Merge {
                break;
            }
        }

        if changed {
            store.update_project(&project)?;
            changed_projects.push(row.project_id);
        }
    }

    if changed_projects.is_empty() {
        bail!(
            "workspace correction could not resolve node {} to any source snapshots",
            request.node_id
        );
    }

    store.append_workspace_correction(&WorkspaceCorrection {
        id: Uuid::now_v7().to_string(),
        workspace_id: workspace_id.to_string(),
        aggregate_node_id: request.node_id.clone(),
        kind: request.kind.clone(),
        target_node_id: request.target_node_id.clone(),
        value: request.value.clone(),
        evidence_ids: selected_detail
            .evidence
            .iter()
            .map(|evidence| evidence.id.clone())
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect(),
        source_node_ids: replayed_source_node_ids.into_iter().collect(),
        created_at: unix_timestamp_seconds(),
    })?;
    store.materialize_workspace_brain_repo(workspace_id)?;

    let project = store
        .load_workspace_project(workspace_id)?
        .unwrap_or_else(|| empty_workspace_project(workspace_id));
    Ok(ApplyCorrectionResponseData { project })
}

fn handle_delete_materialized_workspace_node(
    store: &KnowledgeProjectStore,
    workspace_id: &str,
    rows: &[(StoredSourceRow, Option<KnowledgeProject>)],
    request: &ApplyCorrectionRequest,
) -> Result<ApplyCorrectionResponseData> {
    let workspace_root = workspace_root_for_rows(rows)
        .unwrap_or_else(|| fallback_workspace_root(&store.path, workspace_id));
    let snapshot = read_materialized_brain_snapshot(&workspace_root, workspace_id)?;
    let Some(node) = snapshot
        .nodes
        .iter()
        .find(|node| node.node_id == request.node_id)
    else {
        store.materialize_workspace_brain_repo(workspace_id)?;
        let project = store
            .load_workspace_project(workspace_id)?
            .unwrap_or_else(|| empty_workspace_project(workspace_id));
        return Ok(ApplyCorrectionResponseData { project });
    };
    store.append_workspace_correction(&WorkspaceCorrection {
        id: Uuid::now_v7().to_string(),
        workspace_id: workspace_id.to_string(),
        aggregate_node_id: request.node_id.clone(),
        kind: request.kind.clone(),
        target_node_id: None,
        value: request.value.clone(),
        evidence_ids: node.evidence_ids.clone(),
        source_node_ids: vec![format!("materialized:{}", node.node_id)],
        created_at: unix_timestamp_seconds(),
    })?;
    store.materialize_workspace_brain_repo(workspace_id)?;

    let project = store
        .load_workspace_project(workspace_id)?
        .unwrap_or_else(|| empty_workspace_project(workspace_id));
    Ok(ApplyCorrectionResponseData { project })
}

fn handle_delete_workspace_source_node(
    store: &KnowledgeProjectStore,
    workspace_id: &str,
    selected_detail: &GraphNodeDetail,
    request: &ApplyCorrectionRequest,
) -> Result<ApplyCorrectionResponseData> {
    let source_id = selected_detail
        .source
        .as_ref()
        .map(|source| source.source_id.clone())
        .or_else(|| {
            request
                .node_id
                .strip_prefix("source:")
                .map(ToOwned::to_owned)
        })
        .ok_or_else(|| anyhow!("source node {} has no source backing", request.node_id))?;
    let deleted_row = store
        .delete_workspace_source(workspace_id, &source_id)?
        .ok_or_else(|| anyhow!("source {source_id} was not found in workspace {workspace_id}"))?;
    store.append_workspace_correction(&WorkspaceCorrection {
        id: Uuid::now_v7().to_string(),
        workspace_id: workspace_id.to_string(),
        aggregate_node_id: request.node_id.clone(),
        kind: request.kind.clone(),
        target_node_id: None,
        value: request.value.clone(),
        evidence_ids: selected_detail
            .evidence
            .iter()
            .map(|evidence| evidence.id.clone())
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect(),
        source_node_ids: vec![format!("{}:{}", deleted_row.project_id, request.node_id)],
        created_at: unix_timestamp_seconds(),
    })?;
    store.materialize_workspace_brain_repo(workspace_id)?;

    let project = store
        .load_workspace_project(workspace_id)?
        .unwrap_or_else(|| empty_workspace_project(workspace_id));
    Ok(ApplyCorrectionResponseData { project })
}

pub(crate) fn empty_workspace_project(workspace_id: &str) -> KnowledgeProject {
    finalize_workspace_project(
        workspace_id,
        Vec::new(),
        Vec::new(),
        BTreeMap::new(),
        BTreeMap::new(),
        BTreeMap::new(),
        0,
    )
}
