use anyhow::{Context, Result};
use graphqlite::Graph;
use std::fs;

use super::context_pack_store::unique_project_evidence;
use super::row_decode::{json_string_slug, sql_literal, sql_optional_literal};
use crate::policy::redact_path_for_agent;
use crate::{KnowledgeProject, SourceArtifactManifest};

pub(super) fn persist_source_manifest_in_transaction(
    graph: &Graph,
    project: &KnowledgeProject,
    manifest: &SourceArtifactManifest,
) -> Result<()> {
    let sqlite = graph.connection().sqlite_connection();
    let manifest_json = serde_json::to_string(manifest)
        .context("failed encoding source manifest for knowledge DB")?;
    let format = json_string_slug(&manifest.format)?;
    let status = json_string_slug(&manifest.status)?;
    let original_path_redacted = redact_path_for_agent(&manifest.original_path);
    let source_path_redacted = redact_path_for_agent(&manifest.source_path);
    let markdown_path_redacted = redact_path_for_agent(&manifest.markdown_path);
    let failed_count = manifest
        .pages
        .iter()
        .filter(|page| page.error_message.is_some())
        .count();
    let success_count = manifest.pages.len().saturating_sub(failed_count);
    let project_evidence = unique_project_evidence(project);
    let source_evidence_count = project_evidence
        .iter()
        .filter(|evidence| {
            evidence.source_id.as_deref().unwrap_or(&manifest.source_id) == manifest.source_id
        })
        .count();
    let citation_ready = success_count > 0 || source_evidence_count > 0;
    let warnings_json = serde_json::to_string(
        &manifest
            .pages
            .iter()
            .filter_map(|page| {
                page.error_message.as_ref().map(|message| {
                    serde_json::json!({
                        "pageIndex": page.index,
                        "pageLabel": page.label,
                        "message": message
                    })
                })
            })
            .collect::<Vec<_>>(),
    )
    .context("failed encoding source warnings for knowledge DB")?;
    sqlite.execute_batch(&format!(
        "INSERT INTO import_jobs (job_id, workspace_id, source_id, status, citation_ready, graph_ready, graph_status, graph_error_category, graph_error_message_redacted, graph_retryable, graph_retry_attempt, graph_max_retry_attempts, graph_next_retry_at, manual_retry_available, created_at, updated_at, error_message)
                 VALUES ({job_id}, {workspace_id}, {source_id}, {status}, {citation_ready}, 0, '', '', '', 0, 0, 2, NULL, 0, {created_at}, {updated_at}, NULL)
                 ON CONFLICT(job_id) DO UPDATE SET
                   workspace_id=excluded.workspace_id,
                   source_id=excluded.source_id,
                   status=excluded.status,
                   citation_ready=excluded.citation_ready,
                   updated_at=excluded.updated_at,
                   error_message=excluded.error_message;
                 INSERT INTO sources (source_id, workspace_id, project_id, title, original_path, source_path, markdown_path, original_path_redacted, source_path_redacted, markdown_path_redacted, format, status, page_count, success_count, failed_count, parse_warnings_json, manifest_path, manifest_json, created_at, updated_at)
                 VALUES ({source_id}, {workspace_id}, {project_id}, {title}, {original_path}, {source_path}, {markdown_path}, {original_path_redacted}, {source_path_redacted}, {markdown_path_redacted}, {format}, {status}, {page_count}, {success_count}, {failed_count}, {warnings_json}, {manifest_path}, {manifest_json}, {created_at}, {updated_at})
                 ON CONFLICT(source_id) DO UPDATE SET
                   workspace_id=excluded.workspace_id,
                   project_id=excluded.project_id,
                   title=excluded.title,
                   original_path=excluded.original_path,
                   source_path=excluded.source_path,
                   markdown_path=excluded.markdown_path,
                   original_path_redacted=excluded.original_path_redacted,
                   source_path_redacted=excluded.source_path_redacted,
                   markdown_path_redacted=excluded.markdown_path_redacted,
                   format=excluded.format,
                   status=excluded.status,
                   page_count=excluded.page_count,
                   success_count=excluded.success_count,
                   failed_count=excluded.failed_count,
                   parse_warnings_json=excluded.parse_warnings_json,
                   manifest_path=excluded.manifest_path,
                   manifest_json=excluded.manifest_json,
                   updated_at=excluded.updated_at;
                 DELETE FROM source_pages WHERE source_id = {source_id};
                 DELETE FROM source_page_fts WHERE source_id = {source_id};
                 DELETE FROM evidence_fts WHERE source_id = {source_id};
                 DELETE FROM evidence_items WHERE source_id = {source_id};",
        job_id = sql_literal(&format!("import:{}", manifest.source_id)),
        workspace_id = sql_literal(&manifest.workspace_id),
        source_id = sql_literal(&manifest.source_id),
        status = sql_literal(&status),
        citation_ready = if citation_ready { 1 } else { 0 },
        created_at = manifest.created_at,
        updated_at = manifest.updated_at,
        project_id = sql_literal(&project.summary.project_id),
        title = sql_literal(&project.summary.title),
        original_path = sql_literal(&manifest.original_path),
        source_path = sql_literal(&manifest.source_path),
        markdown_path = sql_literal(&manifest.markdown_path),
        original_path_redacted = sql_literal(&original_path_redacted),
        source_path_redacted = sql_literal(&source_path_redacted),
        markdown_path_redacted = sql_literal(&markdown_path_redacted),
        format = sql_literal(&format),
        page_count = manifest.pages.len(),
        success_count = success_count,
        failed_count = failed_count,
        warnings_json = sql_literal(&warnings_json),
        manifest_path = sql_literal(&manifest.manifest_path),
        manifest_json = sql_literal(&manifest_json),
    ))?;

    for page in &manifest.pages {
        let markdown_path_redacted = page.markdown_path.as_deref().map(redact_path_for_agent);
        let image_path_redacted = page.image_path.as_deref().map(redact_path_for_agent);
        let plain_text = page
            .plain_text_path
            .as_deref()
            .and_then(|path| fs::read_to_string(path).ok())
            .unwrap_or_default();
        let parse_warnings_json = serde_json::to_string(
            &page
                .error_message
                .as_ref()
                .map(|message| vec![message.clone()])
                .unwrap_or_default(),
        )?;
        sqlite.execute_batch(&format!(
            "INSERT INTO source_pages (source_id, page_index, page_label, markdown_path_redacted, image_path_redacted, plain_text, parse_warnings_json)
                     VALUES ({source_id}, {page_index}, {page_label}, {markdown_path}, {image_path}, {plain_text}, {parse_warnings_json});",
            source_id = sql_literal(&manifest.source_id),
            page_index = page.index,
            page_label = sql_literal(&page.label),
            markdown_path = sql_optional_literal(markdown_path_redacted.as_deref()),
            image_path = sql_optional_literal(image_path_redacted.as_deref()),
            plain_text = sql_literal(&plain_text),
            parse_warnings_json = sql_literal(&parse_warnings_json),
        ))?;
        if !plain_text.trim().is_empty() {
            sqlite.execute_batch(&format!(
                "INSERT INTO source_page_fts (source_id, page_index, page_label, text)
                         VALUES ({source_id}, {page_index}, {page_label}, {plain_text});",
                source_id = sql_literal(&manifest.source_id),
                page_index = page.index,
                page_label = sql_literal(&page.label),
                plain_text = sql_literal(&plain_text),
            ))?;
        }
    }

    for evidence in project_evidence {
        let source_id = evidence.source_id.as_deref().unwrap_or(&manifest.source_id);
        if source_id != manifest.source_id {
            continue;
        }
        let page_index = evidence
            .page_index
            .map(|page| page.to_string())
            .unwrap_or_else(|| "NULL".into());
        let provenance = evidence.provenance.clone().unwrap_or_default();
        sqlite.execute_batch(&format!(
            "INSERT INTO evidence_items (evidence_id, workspace_id, source_id, page_index, page_label, evidence_type, snippet, source_path_redacted, markdown_path_redacted, image_path_redacted, provenance, span_json, region_json, status)
                     VALUES ({evidence_id}, {workspace_id}, {source_id}, {page_index}, {page_label}, 'text', {snippet}, {source_path}, {markdown_path}, {image_path}, {provenance}, '{{}}', '{{}}', 'active')
                     ON CONFLICT(evidence_id) DO UPDATE SET
                       workspace_id=excluded.workspace_id,
                       source_id=excluded.source_id,
                       page_index=excluded.page_index,
                       page_label=excluded.page_label,
                       evidence_type=excluded.evidence_type,
                       snippet=excluded.snippet,
                       source_path_redacted=excluded.source_path_redacted,
                       markdown_path_redacted=excluded.markdown_path_redacted,
                       image_path_redacted=excluded.image_path_redacted,
                       provenance=excluded.provenance,
                       status=excluded.status;
                     INSERT INTO evidence_fts (evidence_id, source_id, evidence_type, text)
                     VALUES ({evidence_id}, {source_id}, 'text', {snippet});",
            evidence_id = sql_literal(&evidence.id),
            workspace_id = sql_literal(&manifest.workspace_id),
            source_id = sql_literal(source_id),
            page_index = page_index,
            page_label = sql_literal(&evidence.page_label),
            snippet = sql_literal(&evidence.snippet),
            source_path = sql_literal(&redact_path_for_agent(evidence.source_path.as_deref().unwrap_or(&manifest.source_path))),
            markdown_path = sql_literal(&redact_path_for_agent(evidence.markdown_path.as_deref().unwrap_or(&manifest.markdown_path))),
            image_path = sql_literal(&redact_path_for_agent(evidence.image_path.as_deref().unwrap_or(""))),
            provenance = sql_literal(&provenance),
        ))?;
    }
    Ok(())
}
