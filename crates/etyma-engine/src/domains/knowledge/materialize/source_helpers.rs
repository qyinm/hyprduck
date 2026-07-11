use super::*;

pub(crate) fn workspace_root_for_rows(
    rows: &[(StoredSourceRow, Option<KnowledgeProject>)],
) -> Option<PathBuf> {
    rows.iter()
        .find_map(|(row, _)| workspace_root_from_summary(&row.summary))
}

pub(crate) fn workspace_root_from_summary(summary: &SourceSummary) -> Option<PathBuf> {
    workspace_root_from_path_segments(&summary.source_path, "sources", &summary.source_id).or_else(
        || {
            workspace_root_from_path_segments(
                &summary.markdown_path,
                "artifacts",
                &summary.source_id,
            )
        },
    )
}

pub(crate) fn workspace_root_from_path_segments(
    path: &str,
    marker: &str,
    source_id: &str,
) -> Option<PathBuf> {
    let marker = format!("/{marker}/{source_id}");
    path.find(&marker)
        .filter(|index| *index > 0)
        .map(|index| PathBuf::from(&path[..index]))
}

pub(crate) fn fallback_workspace_root(store_path: &Path, workspace_id: &str) -> PathBuf {
    if let Some(output_root) = std::env::var_os("ETYMA_OUTPUT_DIR") {
        return PathBuf::from(output_root).join(workspace_id);
    }
    store_path
        .parent()
        .map(|parent| parent.join(workspace_id))
        .unwrap_or_else(|| PathBuf::from(workspace_id))
}

pub(crate) fn concept_identity_keys(detail: &GraphNodeDetail) -> Vec<String> {
    let mut keys = Vec::new();
    let canonical_key = normalize_key(&detail.canonical_name);
    if !canonical_key.is_empty() {
        keys.push(canonical_key);
    }
    for alias in &detail.aliases {
        let key = normalize_key(alias);
        if !key.is_empty() && !keys.contains(&key) {
            keys.push(key);
        }
    }
    keys
}

pub(crate) fn source_node_position(index: usize, total: usize) -> GraphNodePosition {
    if total <= 1 {
        return GraphNodePosition { x: 50.0, y: 12.0 };
    }
    let x = 14.0 + (72.0 / (total.saturating_sub(1) as f32)) * (index as f32);
    GraphNodePosition { x, y: 12.0 }
}

pub(crate) fn source_node_id(source_id: &str) -> String {
    format!("source:{source_id}")
}

pub(crate) fn is_source_like_node_kind(kind: GraphNodeKind) -> bool {
    matches!(kind, GraphNodeKind::Source | GraphNodeKind::Document)
}

pub(crate) fn source_label_from_manifest(manifest: &SourceArtifactManifest) -> String {
    Path::new(&manifest.original_path)
        .file_name()
        .or_else(|| Path::new(&manifest.source_path).file_name())
        .map(|value| value.to_string_lossy().into_owned())
        .unwrap_or_else(|| manifest.output_name.clone())
}

pub(crate) fn source_backing_from_manifest(manifest: &SourceArtifactManifest) -> SourceBacking {
    SourceBacking {
        workspace_id: manifest.workspace_id.clone(),
        source_id: manifest.source_id.clone(),
        original_path: manifest.original_path.clone(),
        source_path: manifest.source_path.clone(),
        markdown_path: manifest.markdown_path.clone(),
        format: document_format_slug(&manifest.format).into(),
        status: ingest_status_slug(&manifest.status).into(),
        page_count: manifest.pages.len(),
        success_count: manifest
            .pages
            .iter()
            .filter(|page| page.error_message.is_none())
            .count(),
        failed_count: manifest
            .pages
            .iter()
            .filter(|page| page.error_message.is_some())
            .count(),
        description: manifest.description.clone(),
        user_context: manifest.user_context.clone(),
        ingest_instruction: manifest.ingest_instruction.clone(),
        updated_at: manifest.updated_at,
        manifest_path: Some(manifest.manifest_path.clone()),
    }
}

pub(crate) fn workspace_root_from_manifest(manifest: &SourceArtifactManifest) -> PathBuf {
    Path::new(&manifest.artifact_root)
        .parent()
        .and_then(Path::parent)
        .map(Path::to_path_buf)
        .unwrap_or_else(|| {
            Path::new(&manifest.manifest_path)
                .parent()
                .and_then(Path::parent)
                .and_then(Path::parent)
                .map(Path::to_path_buf)
                .unwrap_or_else(|| PathBuf::from(&manifest.artifact_root))
        })
}
