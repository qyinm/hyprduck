//! Internal helpers extracted from the engine facade module.

use anyhow::{bail, Context, Result};
use graphqlite::Graph;
use hyprduck_engine_types::ImportJobRecord;
use std::path::Path;

pub(super) fn read_import_job(
    db_path: &Path,
    workspace_id: &str,
    job_id: Option<&str>,
    source_id: Option<&str>,
) -> Result<Option<ImportJobRecord>> {
    let graph = Graph::open(db_path).context("GraphQLite failed to open knowledge DB")?;
    let sqlite = graph.connection().sqlite_connection();
    let mut query = String::from(
        "SELECT
            import_jobs.job_id,
            import_jobs.workspace_id,
            COALESCE(import_jobs.source_id, ''),
            import_jobs.status,
            import_jobs.citation_ready,
            import_jobs.graph_ready,
            import_jobs.graph_status,
            import_jobs.graph_error_category,
            import_jobs.graph_error_message_redacted,
            import_jobs.graph_retryable,
            import_jobs.graph_retry_attempt,
            import_jobs.graph_max_retry_attempts,
            import_jobs.graph_next_retry_at,
            import_jobs.manual_retry_available,
            COALESCE(sources.markdown_path, ''),
            COALESCE(sources.source_path, ''),
            COALESCE(sources.manifest_path, ''),
            import_jobs.updated_at
         FROM import_jobs
         LEFT JOIN sources ON sources.workspace_id = import_jobs.workspace_id
            AND sources.source_id = import_jobs.source_id
         WHERE import_jobs.workspace_id = ?1",
    );
    if job_id.is_some() {
        query.push_str(" AND import_jobs.job_id = ?2");
    } else if source_id.is_some() {
        query.push_str(" AND import_jobs.source_id = ?2");
    } else {
        bail!("read_import_job requires job_id or source_id");
    }
    query.push_str(" ORDER BY import_jobs.updated_at DESC LIMIT 1");

    let lookup = job_id.or(source_id).unwrap_or_default();
    let mut statement = sqlite
        .prepare(&query)
        .context("failed preparing import job query")?;
    let mut rows = statement
        .query((workspace_id, lookup))
        .context("failed querying import job")?;
    let Some(row) = rows.next().context("failed reading import job row")? else {
        return Ok(None);
    };
    Ok(Some(ImportJobRecord {
        job_id: row.get(0).context("read import job id")?,
        workspace_id: row.get(1).context("read import job workspace")?,
        source_id: row.get(2).context("read import job source")?,
        status: row.get(3).context("read import job status")?,
        citation_ready: row.get::<_, i64>(4).context("read citation_ready")? != 0,
        graph_ready: row.get::<_, i64>(5).context("read graph_ready")? != 0,
        graph_status: row.get(6).context("read graph_status")?,
        graph_error_category: row.get(7).context("read graph_error_category")?,
        graph_error_message_redacted: row.get(8).context("read graph error message")?,
        graph_retryable: row.get::<_, i64>(9).context("read graph_retryable")? != 0,
        graph_retry_attempt: row
            .get::<_, i64>(10)
            .context("read graph_retry_attempt")?
            .clamp(0, u8::MAX as i64) as u8,
        graph_max_retry_attempts: row
            .get::<_, i64>(11)
            .context("read graph_max_retry_attempts")?
            .clamp(0, u8::MAX as i64) as u8,
        graph_next_retry_at: row
            .get::<_, Option<i64>>(12)
            .context("read graph_next_retry_at")?
            .and_then(|value| (value >= 0).then_some(value as u64)),
        manual_retry_available: row
            .get::<_, i64>(13)
            .context("read manual_retry_available")?
            != 0,
        source_markdown_path: row.get(14).context("read source markdown path")?,
        source_document_path: row.get(15).context("read source document path")?,
        source_manifest_path: row.get(16).context("read source manifest path")?,
        updated_at: row
            .get::<_, i64>(17)
            .context("read import job updated_at")?
            .max(0) as u64,
    }))
}

#[allow(clippy::too_many_arguments)]
pub(super) fn update_import_job_graph_status_from_mcp(
    db_path: &Path,
    workspace_id: &str,
    source_id: &str,
    status: &str,
    graph_status: &str,
    graph_error_category: Option<&str>,
    graph_error_message_redacted: Option<&str>,
    graph_retryable: bool,
    graph_retry_attempt: u8,
    graph_max_retry_attempts: u8,
    graph_next_retry_at: Option<u64>,
    manual_retry_available: bool,
) -> Result<bool> {
    let graph = Graph::open(db_path).context("GraphQLite failed to open knowledge DB")?;
    let sqlite = graph.connection().sqlite_connection();
    let changed = sqlite
        .execute(
            "UPDATE import_jobs
             SET status = ?1,
                 graph_ready = CASE WHEN ?2 IN ('ready', 'rebuilt', 'partially_applied') THEN 1 ELSE 0 END,
                 graph_status = ?2,
                 graph_error_category = ?3,
                 graph_error_message_redacted = ?4,
                 graph_retryable = ?5,
                 graph_retry_attempt = ?6,
                 graph_max_retry_attempts = ?7,
                 graph_next_retry_at = ?8,
                 manual_retry_available = ?9,
                 updated_at = unixepoch()
             WHERE workspace_id = ?10 AND source_id = ?11 AND citation_ready = 1",
            (
                status,
                graph_status,
                graph_error_category.unwrap_or_default(),
                graph_error_message_redacted.unwrap_or_default(),
                if graph_retryable { 1_i64 } else { 0_i64 },
                graph_retry_attempt as i64,
                graph_max_retry_attempts as i64,
                graph_next_retry_at.map(|value| value as i64),
                if manual_retry_available { 1_i64 } else { 0_i64 },
                workspace_id,
                source_id,
            ),
        )
        .context("failed updating import job graph status")?;
    Ok(changed > 0)
}
