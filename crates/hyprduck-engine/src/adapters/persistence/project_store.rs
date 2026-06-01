use crate::*;

pub(crate) fn build_project_id(request: &CompileProjectRequest) -> String {
    let stable_source = request
        .source_document_path
        .as_deref()
        .unwrap_or(&request.source_markdown_path);
    format!("project-{:016x}", fnv1a_hash(stable_source.as_bytes()))
}

pub(crate) fn build_source_backed_project_id(workspace_id: &str, source_id: &str) -> String {
    format!(
        "project-{:016x}",
        fnv1a_hash(format!("{workspace_id}/{source_id}").as_bytes())
    )
}

pub(crate) fn fnv1a_hash(bytes: &[u8]) -> u64 {
    let mut hash = 0xcbf29ce484222325u64;
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    hash
}

pub(crate) fn source_summary_from_sqlite_row(line: &str) -> Result<SourceSummary> {
    let columns: Vec<&str> = line.split('|').collect();
    if !matches!(columns.len(), 11 | 12 | 13 | 14) {
        bail!(
            "expected 11, 12, 13, or 14 source summary columns from sqlite, got {}",
            columns.len()
        );
    }
    let readiness_offset = (columns.len() >= 13).then_some(11);
    let manifest_index = if columns.len() >= 13 { 13 } else { 11 };
    let manifest = columns
        .get(manifest_index)
        .map(|encoded| decode_source_manifest_snapshot(encoded))
        .transpose()?;
    Ok(SourceSummary {
        workspace_id: decode_sqlite_hex_text(columns[0])?,
        source_id: decode_sqlite_hex_text(columns[1])?,
        original_path: decode_sqlite_hex_text(columns[2])?,
        source_path: decode_sqlite_hex_text(columns[3])?,
        markdown_path: decode_sqlite_hex_text(columns[4])?,
        format: document_format_from_slug(&decode_sqlite_hex_text(columns[5])?)?,
        status: ingest_status_from_slug(&decode_sqlite_hex_text(columns[6])?)?,
        page_count: columns[7]
            .parse()
            .context("failed to parse source page_count")?,
        success_count: columns[8]
            .parse()
            .context("failed to parse source success_count")?,
        failed_count: columns[9]
            .parse()
            .context("failed to parse source failed_count")?,
        citation_ready: readiness_offset
            .map(|offset| sqlite_bool(columns[offset]))
            .transpose()?
            .unwrap_or_else(|| columns[8].parse::<usize>().unwrap_or_default() > 0),
        graph_ready: readiness_offset
            .map(|offset| sqlite_bool(columns[offset + 1]))
            .transpose()?
            .unwrap_or(false),
        description: manifest
            .as_ref()
            .map(|manifest| manifest.description.clone())
            .unwrap_or_default(),
        user_context: manifest
            .as_ref()
            .map(|manifest| manifest.user_context.clone())
            .unwrap_or_default(),
        ingest_instruction: manifest
            .as_ref()
            .map(|manifest| manifest.ingest_instruction.clone())
            .unwrap_or_default(),
        updated_at: columns[10]
            .parse()
            .context("failed to parse source updated_at")?,
    })
}

pub(crate) fn stored_source_row_from_sqlite_row(line: &str) -> Result<StoredSourceRow> {
    let columns: Vec<&str> = line.split('|').collect();
    if !matches!(columns.len(), 13 | 14 | 15 | 16) {
        bail!(
            "expected 13, 14, 15, or 16 stored source columns from sqlite, got {}",
            columns.len()
        );
    }
    let readiness_offset = (columns.len() >= 15).then_some(13);
    let manifest_index = if columns.len() >= 15 { 15 } else { 13 };
    let manifest = columns
        .get(manifest_index)
        .map(|encoded| decode_source_manifest_snapshot(encoded))
        .transpose()?;
    Ok(StoredSourceRow {
        summary: SourceSummary {
            workspace_id: decode_sqlite_hex_text(columns[0])?,
            source_id: decode_sqlite_hex_text(columns[1])?,
            original_path: decode_sqlite_hex_text(columns[2])?,
            source_path: decode_sqlite_hex_text(columns[3])?,
            markdown_path: decode_sqlite_hex_text(columns[4])?,
            format: document_format_from_slug(&decode_sqlite_hex_text(columns[5])?)?,
            status: ingest_status_from_slug(&decode_sqlite_hex_text(columns[6])?)?,
            page_count: columns[7]
                .parse()
                .context("failed to parse source page_count")?,
            success_count: columns[8]
                .parse()
                .context("failed to parse source success_count")?,
            failed_count: columns[9]
                .parse()
                .context("failed to parse source failed_count")?,
            citation_ready: readiness_offset
                .map(|offset| sqlite_bool(columns[offset]))
                .transpose()?
                .unwrap_or_else(|| columns[8].parse::<usize>().unwrap_or_default() > 0),
            graph_ready: readiness_offset
                .map(|offset| sqlite_bool(columns[offset + 1]))
                .transpose()?
                .unwrap_or(false),
            description: manifest
                .as_ref()
                .map(|manifest| manifest.description.clone())
                .unwrap_or_default(),
            user_context: manifest
                .as_ref()
                .map(|manifest| manifest.user_context.clone())
                .unwrap_or_default(),
            ingest_instruction: manifest
                .as_ref()
                .map(|manifest| manifest.ingest_instruction.clone())
                .unwrap_or_default(),
            updated_at: columns[10]
                .parse()
                .context("failed to parse source updated_at")?,
        },
        project_id: decode_sqlite_hex_text(columns[11])?,
        manifest_path: decode_sqlite_hex_text(columns[12])?,
    })
}

pub(crate) fn workspace_correction_from_sqlite_row(line: &str) -> Result<WorkspaceCorrection> {
    let columns: Vec<&str> = line.split('|').collect();
    if columns.len() != 9 {
        bail!(
            "expected 9 workspace correction columns from sqlite, got {}",
            columns.len()
        );
    }
    let target_node_id = decode_sqlite_hex_text(columns[4])?;
    let value = decode_sqlite_hex_text(columns[5])?;
    let evidence_ids_json = decode_sqlite_hex_text(columns[6])?;
    let source_node_ids_json = decode_sqlite_hex_text(columns[7])?;
    Ok(WorkspaceCorrection {
        id: decode_sqlite_hex_text(columns[0])?,
        workspace_id: decode_sqlite_hex_text(columns[1])?,
        aggregate_node_id: decode_sqlite_hex_text(columns[2])?,
        kind: correction_kind_from_slug(columns[3])?,
        target_node_id: (!target_node_id.is_empty()).then_some(target_node_id),
        value: (!value.is_empty()).then_some(value),
        evidence_ids: serde_json::from_str(&evidence_ids_json)
            .context("failed to decode workspace correction evidence ids")?,
        source_node_ids: serde_json::from_str(&source_node_ids_json)
            .context("failed to decode workspace correction source node ids")?,
        created_at: columns[8]
            .parse()
            .context("failed to parse workspace correction created_at")?,
    })
}

pub(crate) fn correction_kind_slug(kind: &CorrectionKind) -> &'static str {
    match kind {
        CorrectionKind::Merge => "merge",
        CorrectionKind::KeepSeparate => "keep_separate",
        CorrectionKind::Rename => "rename",
        CorrectionKind::Split => "split",
        CorrectionKind::Delete => "delete",
    }
}

pub(crate) fn correction_kind_from_slug(value: &str) -> Result<CorrectionKind> {
    match value {
        "merge" => Ok(CorrectionKind::Merge),
        "keep_separate" => Ok(CorrectionKind::KeepSeparate),
        "rename" => Ok(CorrectionKind::Rename),
        "split" => Ok(CorrectionKind::Split),
        "delete" => Ok(CorrectionKind::Delete),
        _ => bail!("unknown correction kind {value}"),
    }
}

pub(crate) fn decode_project_snapshot(encoded: &str) -> Result<KnowledgeProject> {
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(encoded)
        .context("failed to decode stored project snapshot")?;
    serde_json::from_slice(&bytes).context("failed to decode stored project")
}

pub(crate) fn decode_source_manifest_snapshot(encoded: &str) -> Result<SourceArtifactManifest> {
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(encoded)
        .context("failed to decode stored source manifest snapshot")?;
    serde_json::from_slice(&bytes).context("failed to decode stored source manifest")
}

pub(crate) fn decode_sqlite_hex_text(value: &str) -> Result<String> {
    if !value.len().is_multiple_of(2) {
        bail!("sqlite hex text had an odd byte count");
    }
    let bytes = (0..value.len())
        .step_by(2)
        .map(|index| {
            u8::from_str_radix(&value[index..index + 2], 16)
                .context("failed to decode sqlite hex text byte")
        })
        .collect::<Result<Vec<_>>>()?;
    String::from_utf8(bytes).context("sqlite hex text was not valid UTF-8")
}

pub(crate) fn sqlite_bool(value: &str) -> Result<bool> {
    match value {
        "0" => Ok(false),
        "1" => Ok(true),
        other => bail!("expected sqlite boolean 0 or 1, got {other}"),
    }
}

pub(crate) fn ingest_status_slug(status: &IngestStatus) -> &'static str {
    match status {
        IngestStatus::Added => "added",
        IngestStatus::Rendering => "rendering",
        IngestStatus::Ingesting => "ingesting",
        IngestStatus::Ingested => "ingested",
        IngestStatus::Partial => "partial",
        IngestStatus::Failed => "failed",
        IngestStatus::Stale => "stale",
    }
}

pub(crate) fn ingest_status_from_slug(value: &str) -> Result<IngestStatus> {
    match value {
        "added" => Ok(IngestStatus::Added),
        "rendering" => Ok(IngestStatus::Rendering),
        "ingesting" => Ok(IngestStatus::Ingesting),
        "ingested" => Ok(IngestStatus::Ingested),
        "partial" => Ok(IngestStatus::Partial),
        "failed" => Ok(IngestStatus::Failed),
        "stale" => Ok(IngestStatus::Stale),
        _ => bail!("unknown ingest status {value}"),
    }
}

pub(crate) fn document_format_slug(format: &DocumentFormat) -> &'static str {
    match format {
        DocumentFormat::Pdf => "pdf",
        DocumentFormat::Docx => "docx",
        DocumentFormat::Doc => "doc",
        DocumentFormat::Image => "image",
        DocumentFormat::Markdown => "markdown",
    }
}

pub(crate) fn document_format_from_slug(value: &str) -> Result<DocumentFormat> {
    match value {
        "pdf" => Ok(DocumentFormat::Pdf),
        "docx" => Ok(DocumentFormat::Docx),
        "doc" => Ok(DocumentFormat::Doc),
        "image" => Ok(DocumentFormat::Image),
        "markdown" | "md" => Ok(DocumentFormat::Markdown),
        _ => bail!("unknown document format {value}"),
    }
}

pub(crate) fn sanitize_name(value: &str) -> String {
    let sanitized = value
        .replace(['/', '\\', ':'], "-")
        .replace("..", "-")
        .trim()
        .chars()
        .take(100)
        .collect::<String>();
    if sanitized.is_empty() {
        "output".into()
    } else {
        sanitized
    }
}

pub(crate) fn chrono_like_timestamp() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};

    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    now.to_string()
}

pub(crate) struct KnowledgeProjectStore {
    pub(crate) path: PathBuf,
}

impl KnowledgeProjectStore {
    pub(crate) fn default() -> Result<Self> {
        if let Some(explicit_path) = std::env::var_os("HYPRDUCK_PROJECT_STORE") {
            return Ok(Self {
                path: PathBuf::from(explicit_path),
            });
        }

        let root = dirs::data_local_dir()
            .or_else(dirs::home_dir)
            .ok_or_else(|| anyhow!("failed to resolve local data directory"))?;
        Self::from_data_root(&root)
    }

    pub(crate) fn from_data_root(root: &Path) -> Result<Self> {
        let store_dir = root.join("HyprDuck");
        let new_path = KnowledgeStore::default_path_for_root(&store_dir);
        let legacy_path = store_dir.join("knowledge.sqlite3");
        migrate_legacy_project_store(&legacy_path, &new_path)?;
        Ok(Self {
            path: if new_path.exists() {
                new_path
            } else if legacy_path.exists() {
                legacy_path
            } else {
                new_path
            },
        })
    }

    #[cfg(test)]
    pub(crate) fn new(path: PathBuf) -> Self {
        Self { path }
    }

    pub(crate) fn save_project(
        &self,
        project: &KnowledgeProject,
        request: &CompileProjectRequest,
        source_manifest: Option<&SourceArtifactManifest>,
    ) -> Result<()> {
        self.ensure_schema()?;
        if let Some(source_manifest) = source_manifest {
            self.save_source(project, source_manifest)?;
        }
        let snapshot_json =
            serde_json::to_string(project).context("failed to encode knowledge project")?;
        let snapshot_base64 = base64::engine::general_purpose::STANDARD.encode(snapshot_json);
        let source_document_path = request
            .source_document_path
            .as_ref()
            .map(|path| format!("'{}'", escape_sqlite(path)))
            .unwrap_or_else(|| "NULL".into());
        let status = match project.summary.status {
            ProjectStatus::Preview => "preview",
            ProjectStatus::Ready => "ready",
            ProjectStatus::Degraded => "degraded",
        };
        let sql = format!(
            "INSERT INTO projects (project_id, title, source_markdown_path, source_document_path, status, updated_at, snapshot_base64) \
             VALUES ('{project_id}', '{title}', '{markdown_path}', {source_document_path}, '{status}', {updated_at}, '{snapshot_base64}') \
             ON CONFLICT(project_id) DO UPDATE SET \
               title=excluded.title, \
               source_markdown_path=excluded.source_markdown_path, \
               source_document_path=excluded.source_document_path, \
               status=excluded.status, \
               updated_at=excluded.updated_at, \
               snapshot_base64=excluded.snapshot_base64;",
            project_id = escape_sqlite(&project.summary.project_id),
            title = escape_sqlite(&project.summary.title),
            markdown_path = escape_sqlite(&request.source_markdown_path),
            source_document_path = source_document_path,
            status = status,
            updated_at = unix_timestamp_seconds(),
            snapshot_base64 = snapshot_base64,
        );
        self.run_sql(&sql)?;
        if let Some(source_manifest) = source_manifest {
            self.materialize_workspace_brain_repo(&source_manifest.workspace_id)?;
        }
        Ok(())
    }

    pub(crate) fn update_project(&self, project: &KnowledgeProject) -> Result<()> {
        self.ensure_schema()?;
        let snapshot_json =
            serde_json::to_string(project).context("failed to encode knowledge project")?;
        let snapshot_base64 = base64::engine::general_purpose::STANDARD.encode(snapshot_json);
        let status = match project.summary.status {
            ProjectStatus::Preview => "preview",
            ProjectStatus::Ready => "ready",
            ProjectStatus::Degraded => "degraded",
        };
        let sql = format!(
            "UPDATE projects SET title = '{title}', status = '{status}', updated_at = {updated_at}, snapshot_base64 = '{snapshot_base64}' \
             WHERE project_id = '{project_id}';",
            title = escape_sqlite(&project.summary.title),
            status = status,
            updated_at = unix_timestamp_seconds(),
            snapshot_base64 = snapshot_base64,
            project_id = escape_sqlite(&project.summary.project_id),
        );
        self.run_sql(&sql).map(|_| ())
    }

    pub(crate) fn load_project(
        &self,
        project_id: Option<&str>,
    ) -> Result<Option<KnowledgeProject>> {
        self.ensure_schema()?;
        let sql = match project_id {
            Some(project_id) => format!(
                "SELECT snapshot_base64 FROM projects WHERE project_id = '{}' LIMIT 1;",
                escape_sqlite(project_id)
            ),
            None => "SELECT snapshot_base64 FROM projects ORDER BY updated_at DESC LIMIT 1;".into(),
        };
        let output = self.run_sql(&sql)?;
        let encoded = output.trim();
        if encoded.is_empty() {
            return Ok(None);
        }
        decode_project_snapshot(encoded).map(Some)
    }

    pub(crate) fn load_projects_by_ids(
        &self,
        project_ids: &[String],
    ) -> Result<BTreeMap<String, KnowledgeProject>> {
        self.ensure_schema()?;
        let unique_project_ids = project_ids
            .iter()
            .cloned()
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect::<Vec<_>>();
        if unique_project_ids.is_empty() {
            return Ok(BTreeMap::new());
        }
        let mut projects = BTreeMap::new();
        for chunk in unique_project_ids.chunks(PROJECT_SNAPSHOT_BATCH_SIZE) {
            let quoted_ids = chunk
                .iter()
                .map(|project_id| format!("'{}'", escape_sqlite(project_id)))
                .collect::<Vec<_>>()
                .join(", ");
            let sql = format!(
                "SELECT hex(project_id), snapshot_base64 FROM projects WHERE project_id IN ({quoted_ids});"
            );
            let output = self.run_sql(&sql)?;
            for line in output.lines().filter(|line| !line.trim().is_empty()) {
                let columns = line.split('|').collect::<Vec<_>>();
                if columns.len() != 2 {
                    bail!(
                        "expected 2 project snapshot columns from sqlite, got {}",
                        columns.len()
                    );
                }
                projects.insert(
                    decode_sqlite_hex_text(columns[0])?,
                    decode_project_snapshot(columns[1])?,
                );
            }
        }
        Ok(projects)
    }

    pub(crate) fn load_latest_workspace_id(&self) -> Result<Option<WorkspaceId>> {
        self.ensure_schema()?;
        let output =
            self.run_sql("SELECT workspace_id FROM sources ORDER BY updated_at DESC LIMIT 1;")?;
        let workspace_id = output.trim();
        Ok((!workspace_id.is_empty()).then(|| workspace_id.to_string()))
    }

    pub(crate) fn update_import_job_graph_status(
        &self,
        workspace_id: &str,
        source_id: &str,
        graph_status: &str,
        failed_reason: Option<&str>,
        error_message: Option<&str>,
        retryable: bool,
    ) -> Result<()> {
        self.ensure_schema()?;
        KnowledgeStore::open(self.path.clone())?;
        let graph_ready = if graph_status_is_ready(Some(graph_status)) {
            1
        } else {
            0
        };
        let manual_retry_available = if graph_ready == 0 && graph_status != "skipped" {
            1
        } else {
            0
        };
        let status = if graph_ready == 1 {
            "context_ready"
        } else if graph_status == "skipped" {
            "citation_ready_graph_skipped"
        } else {
            "citation_ready_graph_pending"
        };
        let sql = format!(
            "UPDATE import_jobs
             SET status = '{status}',
                 graph_ready = {graph_ready},
                 graph_status = '{graph_status}',
                 graph_error_category = '{failed_reason}',
                 graph_error_message_redacted = '{error_message}',
                 graph_retryable = {retryable},
                 graph_max_retry_attempts = CASE WHEN graph_max_retry_attempts = 0 THEN 2 ELSE graph_max_retry_attempts END,
                 manual_retry_available = {manual_retry_available},
                 updated_at = {updated_at}
             WHERE workspace_id = '{workspace_id}' AND source_id = '{source_id}' AND citation_ready = 1;",
            status = escape_sqlite(status),
            graph_ready = graph_ready,
            graph_status = escape_sqlite(graph_status),
            failed_reason = escape_sqlite(failed_reason.unwrap_or_default()),
            error_message = escape_sqlite(error_message.unwrap_or_default()),
            retryable = if retryable { 1 } else { 0 },
            manual_retry_available = manual_retry_available,
            updated_at = unix_timestamp_seconds(),
            workspace_id = escape_sqlite(workspace_id),
            source_id = escape_sqlite(source_id),
        );
        self.run_sql(&sql).map(|_| ())
    }

    pub(crate) fn load_workspace_id_for_project(
        &self,
        project_id: &str,
    ) -> Result<Option<WorkspaceId>> {
        self.ensure_schema()?;
        let sql = format!(
            "SELECT workspace_id FROM sources WHERE project_id = '{}' ORDER BY updated_at DESC LIMIT 1;",
            escape_sqlite(project_id)
        );
        let output = self.run_sql(&sql)?;
        let workspace_id = output.trim();
        Ok((!workspace_id.is_empty()).then(|| workspace_id.to_string()))
    }

    pub(crate) fn load_sources(&self, workspace_id: &str) -> Result<Vec<SourceSummary>> {
        self.ensure_schema()?;
        let sql = format!(
            "SELECT hex(sources.workspace_id), hex(sources.source_id), hex(original_path), hex(source_path), hex(markdown_path), hex(format), hex(sources.status), page_count, success_count, failed_count, sources.updated_at, COALESCE(import_jobs.citation_ready, CASE WHEN success_count > 0 THEN 1 ELSE 0 END), COALESCE(import_jobs.graph_ready, 0), manifest_base64 \
             FROM sources LEFT JOIN import_jobs ON import_jobs.workspace_id = sources.workspace_id AND import_jobs.source_id = sources.source_id WHERE sources.workspace_id = '{}' ORDER BY sources.updated_at DESC;",
            escape_sqlite(workspace_id)
        );
        let output = self.run_sql(&sql)?;
        output
            .lines()
            .filter(|line| !line.trim().is_empty())
            .map(source_summary_from_sqlite_row)
            .collect()
    }

    pub(crate) fn load_source_rows(&self, workspace_id: &str) -> Result<Vec<StoredSourceRow>> {
        self.ensure_schema()?;
        let sql = format!(
            "SELECT hex(sources.workspace_id), hex(sources.source_id), hex(original_path), hex(source_path), hex(markdown_path), hex(format), hex(sources.status), page_count, success_count, failed_count, sources.updated_at, hex(project_id), hex(manifest_path), COALESCE(import_jobs.citation_ready, CASE WHEN success_count > 0 THEN 1 ELSE 0 END), COALESCE(import_jobs.graph_ready, 0), manifest_base64 \
             FROM sources LEFT JOIN import_jobs ON import_jobs.workspace_id = sources.workspace_id AND import_jobs.source_id = sources.source_id WHERE sources.workspace_id = '{}' ORDER BY sources.updated_at DESC;",
            escape_sqlite(workspace_id)
        );
        let output = self.run_sql(&sql)?;
        output
            .lines()
            .filter(|line| !line.trim().is_empty())
            .map(stored_source_row_from_sqlite_row)
            .collect()
    }

    pub(crate) fn delete_workspace_source(
        &self,
        workspace_id: &str,
        source_id: &str,
    ) -> Result<Option<StoredSourceRow>> {
        self.ensure_schema()?;
        let row = self
            .load_source_rows(workspace_id)?
            .into_iter()
            .find(|row| row.summary.source_id == source_id);
        let Some(row) = row else {
            return Ok(None);
        };
        let sql = format!(
            "DELETE FROM sources WHERE workspace_id = '{workspace_id}' AND source_id = '{source_id}'; \
             DELETE FROM projects WHERE project_id = '{project_id}' AND NOT EXISTS (SELECT 1 FROM sources WHERE project_id = '{project_id}');",
            workspace_id = escape_sqlite(workspace_id),
            source_id = escape_sqlite(source_id),
            project_id = escape_sqlite(&row.project_id),
        );
        self.run_sql(&sql)?;
        Ok(Some(row))
    }

    pub(crate) fn load_projects_for_workspace(
        &self,
        workspace_id: &str,
    ) -> Result<Vec<(StoredSourceRow, Option<KnowledgeProject>)>> {
        let rows = self.load_source_rows(workspace_id)?;
        let project_ids = rows
            .iter()
            .map(|row| row.project_id.clone())
            .collect::<Vec<_>>();
        let projects_by_id = self.load_projects_by_ids(&project_ids)?;
        rows.into_iter()
            .map(|row| {
                let project = projects_by_id.get(&row.project_id).cloned();
                Ok((row, project))
            })
            .collect()
    }

    pub(crate) fn load_workspace_project(
        &self,
        workspace_id: &str,
    ) -> Result<Option<KnowledgeProject>> {
        let rows = self.load_projects_for_workspace(workspace_id)?;
        if rows.is_empty() {
            return Ok(None);
        }
        Ok(Some(workspace_ui_graph_projection(
            aggregate_workspace_project(workspace_id, rows),
        )))
    }

    pub(crate) fn workspace_root(&self, workspace_id: &str) -> Result<PathBuf> {
        let rows = self.load_projects_for_workspace(workspace_id)?;
        Ok(workspace_root_for_rows(&rows)
            .unwrap_or_else(|| fallback_workspace_root(&self.path, workspace_id)))
    }

    pub(crate) fn save_source(
        &self,
        project: &KnowledgeProject,
        manifest: &SourceArtifactManifest,
    ) -> Result<()> {
        KnowledgeStore::open(self.path.clone())?.persist_source_manifest(project, manifest)?;
        let manifest_json =
            serde_json::to_string(manifest).context("failed to encode source manifest snapshot")?;
        let manifest_base64 = base64::engine::general_purpose::STANDARD.encode(manifest_json);
        let summary = source_summary_from_manifest(manifest);
        let status = ingest_status_slug(&summary.status);
        let format = document_format_slug(&summary.format);
        let sql = format!(
            "INSERT INTO sources (source_id, workspace_id, project_id, original_path, source_path, markdown_path, format, status, page_count, success_count, failed_count, updated_at, manifest_path, manifest_base64) \
             VALUES ('{source_id}', '{workspace_id}', '{project_id}', '{original_path}', '{source_path}', '{markdown_path}', '{format}', '{status}', {page_count}, {success_count}, {failed_count}, {updated_at}, '{manifest_path}', '{manifest_base64}') \
             ON CONFLICT(source_id) DO UPDATE SET \
               workspace_id=excluded.workspace_id, \
               project_id=excluded.project_id, \
               original_path=excluded.original_path, \
               source_path=excluded.source_path, \
               markdown_path=excluded.markdown_path, \
               format=excluded.format, \
               status=excluded.status, \
               page_count=excluded.page_count, \
               success_count=excluded.success_count, \
               failed_count=excluded.failed_count, \
               updated_at=excluded.updated_at, \
               manifest_path=excluded.manifest_path, \
               manifest_base64=excluded.manifest_base64;",
            source_id = escape_sqlite(&summary.source_id),
            workspace_id = escape_sqlite(&summary.workspace_id),
            project_id = escape_sqlite(&project.summary.project_id),
            original_path = escape_sqlite(&summary.original_path),
            source_path = escape_sqlite(&summary.source_path),
            markdown_path = escape_sqlite(&summary.markdown_path),
            format = format,
            status = status,
            page_count = summary.page_count,
            success_count = summary.success_count,
            failed_count = summary.failed_count,
            updated_at = summary.updated_at,
            manifest_path = escape_sqlite(&manifest.manifest_path),
            manifest_base64 = manifest_base64,
        );
        self.run_sql(&sql).map(|_| ())
    }

    pub(crate) fn append_workspace_correction(
        &self,
        correction: &WorkspaceCorrection,
    ) -> Result<()> {
        self.ensure_schema()?;
        let evidence_ids_json = serde_json::to_string(&correction.evidence_ids)
            .context("failed to encode workspace correction evidence ids")?;
        let source_node_ids_json = serde_json::to_string(&correction.source_node_ids)
            .context("failed to encode workspace correction source node ids")?;
        let target_node_id = correction
            .target_node_id
            .as_ref()
            .map(|value| format!("'{}'", escape_sqlite(value)))
            .unwrap_or_else(|| "NULL".into());
        let value = correction
            .value
            .as_ref()
            .map(|value| format!("'{}'", escape_sqlite(value)))
            .unwrap_or_else(|| "NULL".into());
        let sql = format!(
            "INSERT INTO workspace_corrections (id, workspace_id, aggregate_node_id, kind, target_node_id, value, evidence_ids_json, source_node_ids_json, created_at) \
             VALUES ('{id}', '{workspace_id}', '{aggregate_node_id}', '{kind}', {target_node_id}, {value}, '{evidence_ids_json}', '{source_node_ids_json}', {created_at});",
            id = escape_sqlite(&correction.id),
            workspace_id = escape_sqlite(&correction.workspace_id),
            aggregate_node_id = escape_sqlite(&correction.aggregate_node_id),
            kind = correction_kind_slug(&correction.kind),
            target_node_id = target_node_id,
            value = value,
            evidence_ids_json = escape_sqlite(&evidence_ids_json),
            source_node_ids_json = escape_sqlite(&source_node_ids_json),
            created_at = correction.created_at,
        );
        self.run_sql(&sql).map(|_| ())
    }

    pub(crate) fn load_workspace_corrections(
        &self,
        workspace_id: &str,
    ) -> Result<Vec<WorkspaceCorrection>> {
        self.ensure_schema()?;
        let sql = format!(
            "SELECT hex(id), hex(workspace_id), hex(aggregate_node_id), kind, hex(COALESCE(target_node_id, '')), hex(COALESCE(value, '')), hex(evidence_ids_json), hex(source_node_ids_json), created_at \
             FROM workspace_corrections WHERE workspace_id = '{}' ORDER BY created_at ASC, id ASC;",
            escape_sqlite(workspace_id)
        );
        let output = self.run_sql(&sql)?;
        output
            .lines()
            .filter(|line| !line.trim().is_empty())
            .map(workspace_correction_from_sqlite_row)
            .collect()
    }

    pub(crate) fn materialize_workspace_brain_repo(&self, workspace_id: &str) -> Result<()> {
        let rows = self.load_projects_for_workspace(workspace_id)?;
        if rows.is_empty() {
            let workspace_root = fallback_workspace_root(&self.path, workspace_id);
            let mut snapshot = empty_replayed_brain_snapshot(workspace_id);
            snapshot.generated_at = unix_timestamp_seconds();
            KnowledgeStore::open(self.path.clone())?.persist_graph_snapshot(&snapshot)?;
            return write_materialized_brain_repo(&workspace_root, &snapshot);
        }
        let workspace_root = workspace_root_for_rows(&rows)
            .unwrap_or_else(|| fallback_workspace_root(&self.path, workspace_id));
        let aggregate = aggregate_workspace_project(workspace_id, rows.clone());
        let corrections = self.load_workspace_corrections(workspace_id)?;
        let existing_memories = read_memory_records(&workspace_root)?;
        let existing_nodes = read_existing_graph_nodes(&workspace_root)?;
        let existing_relations = read_existing_graph_relations(&workspace_root)?;
        let snapshot = build_brain_repo_snapshot(
            workspace_id,
            &rows,
            &aggregate,
            &corrections,
            &existing_memories,
            &existing_nodes,
            &existing_relations,
        );
        KnowledgeStore::open(self.path.clone())?.persist_graph_snapshot(&snapshot)?;
        write_materialized_brain_repo(&workspace_root, &snapshot)
    }

    pub(crate) fn ensure_schema(&self) -> Result<()> {
        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("failed creating {}", parent.display()))?;
        }
        KnowledgeStore::open(self.path.clone())?;
        self.run_sql(
            "CREATE TABLE IF NOT EXISTS projects (
                project_id TEXT PRIMARY KEY,
                title TEXT NOT NULL,
                source_markdown_path TEXT NOT NULL,
                source_document_path TEXT,
                status TEXT NOT NULL,
                updated_at INTEGER NOT NULL,
                snapshot_base64 TEXT NOT NULL
            );
            CREATE INDEX IF NOT EXISTS idx_projects_updated_at ON projects(updated_at DESC);
            CREATE TABLE IF NOT EXISTS sources (
                source_id TEXT PRIMARY KEY,
                workspace_id TEXT NOT NULL,
                project_id TEXT NOT NULL,
                original_path TEXT NOT NULL,
                source_path TEXT NOT NULL,
                markdown_path TEXT NOT NULL,
                format TEXT NOT NULL,
                status TEXT NOT NULL,
                page_count INTEGER NOT NULL,
                success_count INTEGER NOT NULL,
                failed_count INTEGER NOT NULL,
                updated_at INTEGER NOT NULL,
                manifest_path TEXT NOT NULL,
                manifest_base64 TEXT NOT NULL
            );
            CREATE INDEX IF NOT EXISTS idx_sources_workspace_updated_at ON sources(workspace_id, updated_at DESC);
            CREATE TABLE IF NOT EXISTS workspace_corrections (
                id TEXT PRIMARY KEY,
                workspace_id TEXT NOT NULL,
                aggregate_node_id TEXT NOT NULL,
                kind TEXT NOT NULL,
                target_node_id TEXT,
                value TEXT,
                evidence_ids_json TEXT NOT NULL,
                source_node_ids_json TEXT NOT NULL,
                created_at INTEGER NOT NULL
            );
            CREATE INDEX IF NOT EXISTS idx_workspace_corrections_workspace_created_at ON workspace_corrections(workspace_id, created_at ASC);",
        )
        .map(|_| ())
    }

    pub(crate) fn run_sql(&self, sql: &str) -> Result<String> {
        let output = Command::new(resolve_binary("sqlite3", &["/usr/bin/sqlite3"]))
            .arg(&self.path)
            .arg(sql)
            .output()
            .with_context(|| format!("failed to launch sqlite3 for {}", self.path.display()))?;

        if !output.status.success() {
            bail!(
                "sqlite3 failed: {}",
                String::from_utf8_lossy(&output.stderr).trim()
            );
        }

        String::from_utf8(output.stdout).context("sqlite3 output was not valid UTF-8")
    }
}

pub(crate) fn migrate_legacy_project_store(legacy_path: &Path, new_path: &Path) -> Result<()> {
    if new_path.exists() || !legacy_path.exists() {
        return Ok(());
    }
    if let Some(parent) = new_path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed creating {}", parent.display()))?;
    }
    fs::copy(legacy_path, new_path).with_context(|| {
        format!(
            "failed migrating legacy project store from {} to {}",
            legacy_path.display(),
            new_path.display()
        )
    })?;
    Ok(())
}

pub(crate) fn escape_sqlite(value: &str) -> String {
    value.replace('\'', "''")
}
