use anyhow::{Context, Result};
use graphqlite::Graph;
use hyprduck_engine_types::BrainRepoSnapshot;
use std::collections::{BTreeMap, BTreeSet};

use crate::policy::redact_path_for_agent;

use super::nodes::invalidate_live_graph_node_versions;

pub(super) fn persist_snapshot_sources_in_transaction(
    graph: &Graph,
    snapshot: &BrainRepoSnapshot,
) -> Result<()> {
    let sqlite = graph.connection().sqlite_connection();
    for source in &snapshot.sources {
        let original_path_redacted = redact_path_for_agent(&source.original_path);
        let source_path_redacted = redact_path_for_agent(&source.source_path);
        let markdown_path_redacted = redact_path_for_agent(&source.markdown_path);
        sqlite
            .execute(
                "INSERT INTO sources (
                    source_id,
                    workspace_id,
                    project_id,
                    title,
                    original_path,
                    source_path,
                    markdown_path,
                    original_path_redacted,
                    source_path_redacted,
                    markdown_path_redacted,
                    format,
                    status,
                    page_count,
                    success_count,
                    failed_count,
                    updated_at
                ) VALUES (?1, ?2, '', ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?12, 0, ?13)
                ON CONFLICT(source_id) DO UPDATE SET
                    workspace_id=excluded.workspace_id,
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
                    updated_at=excluded.updated_at",
                (
                    source.source_id.as_str(),
                    snapshot.workspace_id.as_str(),
                    source.source_id.as_str(),
                    source.original_path.as_str(),
                    source.source_path.as_str(),
                    source.markdown_path.as_str(),
                    original_path_redacted.as_str(),
                    source_path_redacted.as_str(),
                    markdown_path_redacted.as_str(),
                    source.format.as_str(),
                    source.status.as_str(),
                    source.page_count as i64,
                    source.updated_at as i64,
                ),
            )
            .with_context(|| format!("failed upserting source row {}", source.source_id))?;
    }
    for evidence in &snapshot.evidence {
        let Some(source_id) = evidence.source_id.as_deref() else {
            continue;
        };
        sqlite
            .execute(
                "INSERT OR IGNORE INTO sources (
                    source_id,
                    workspace_id,
                    project_id,
                    title,
                    original_path,
                    source_path,
                    markdown_path,
                    format,
                    status,
                    page_count,
                    success_count,
                    failed_count,
                    updated_at
                ) VALUES (?1, ?2, '', ?1, '', '', '', 'unknown', 'unknown', 0, 0, 0, ?3)",
                (
                    source_id,
                    snapshot.workspace_id.as_str(),
                    snapshot.generated_at as i64,
                ),
            )
            .with_context(|| format!("failed upserting evidence source row {source_id}"))?;
    }
    prune_workspace_sources_not_in_snapshot(graph, snapshot)?;
    Ok(())
}

pub(crate) fn purge_workspace_source_in_transaction(
    graph: &Graph,
    workspace_id: &str,
    source_id: &str,
    invalidated_at: i64,
) -> Result<()> {
    let sqlite = graph.connection().sqlite_connection();
    sqlite
        .execute(
            "DELETE FROM sources WHERE workspace_id = ?1 AND source_id = ?2",
            (workspace_id, source_id),
        )
        .with_context(|| format!("failed deleting workspace source row {source_id}"))?;
    sqlite
        .execute("DELETE FROM source_pages WHERE source_id = ?1", [source_id])
        .with_context(|| format!("failed deleting source pages for {source_id}"))?;
    sqlite
        .execute(
            "DELETE FROM source_page_fts WHERE source_id = ?1",
            [source_id],
        )
        .with_context(|| format!("failed deleting source page FTS for {source_id}"))?;
    sqlite
        .execute("DELETE FROM evidence_fts WHERE source_id = ?1", [source_id])
        .with_context(|| format!("failed deleting evidence FTS for {source_id}"))?;
    sqlite
        .execute(
            "DELETE FROM evidence_items WHERE source_id = ?1",
            [source_id],
        )
        .with_context(|| format!("failed deleting evidence items for {source_id}"))?;
    sqlite
        .execute(
            "DELETE FROM import_jobs WHERE workspace_id = ?1 AND source_id = ?2",
            (workspace_id, source_id),
        )
        .with_context(|| format!("failed deleting import job for {source_id}"))?;
    let source_node_id = format!("source:{source_id}");
    invalidate_live_graph_node_versions(
        graph,
        workspace_id,
        &source_node_id,
        None,
        invalidated_at,
        "workspace_source_deleted",
    )?;
    Ok(())
}

fn prune_workspace_sources_not_in_snapshot(
    graph: &Graph,
    snapshot: &BrainRepoSnapshot,
) -> Result<()> {
    let mut keep_source_ids = snapshot
        .sources
        .iter()
        .map(|source| source.source_id.as_str())
        .collect::<BTreeSet<_>>();
    for evidence in &snapshot.evidence {
        if let Some(source_id) = evidence.source_id.as_deref() {
            keep_source_ids.insert(source_id);
        }
    }
    for node in &snapshot.nodes {
        for source_id in &node.source_ids {
            keep_source_ids.insert(source_id.as_str());
        }
    }
    let sqlite = graph.connection().sqlite_connection();
    let mut statement = sqlite
        .prepare("SELECT source_id FROM sources WHERE workspace_id = ?1")
        .context("failed preparing workspace source prune query")?;
    let mut rows = statement
        .query([snapshot.workspace_id.as_str()])
        .context("failed querying workspace sources for prune")?;
    let mut stale_source_ids = Vec::new();
    while let Some(row) = rows
        .next()
        .context("failed reading workspace source row for prune")?
    {
        let source_id: String = row.get(0).context("read workspace source id for prune")?;
        if !keep_source_ids.contains(source_id.as_str()) {
            stale_source_ids.push(source_id);
        }
    }
    for source_id in stale_source_ids {
        purge_workspace_source_in_transaction(
            graph,
            &snapshot.workspace_id,
            &source_id,
            snapshot.generated_at as i64,
        )?;
    }
    Ok(())
}

#[derive(Debug, Default)]
struct SourcePageSnapshotRow {
    page_label: String,
    markdown_path_redacted: String,
    image_path_redacted: String,
    plain_text: String,
    parse_warnings_json: String,
    snippets: Vec<String>,
}

pub(super) fn persist_source_pages_snapshot_in_transaction(
    graph: &Graph,
    snapshot: &BrainRepoSnapshot,
) -> Result<()> {
    let sqlite = graph.connection().sqlite_connection();
    let source_ids = snapshot
        .sources
        .iter()
        .map(|source| source.source_id.clone())
        .chain(
            snapshot
                .evidence
                .iter()
                .filter_map(|evidence| evidence.source_id.clone()),
        )
        .collect::<BTreeSet<_>>();
    let mut pages = BTreeMap::<(String, usize), SourcePageSnapshotRow>::new();
    for source_id in &source_ids {
        let mut statement = sqlite
            .prepare(
                "SELECT page_index,
                        page_label,
                        COALESCE(markdown_path_redacted, ''),
                        COALESCE(image_path_redacted, ''),
                        plain_text,
                        parse_warnings_json
                 FROM source_pages
                 WHERE source_id = ?1",
            )
            .with_context(|| {
                format!("failed preparing source page preservation for {source_id}")
            })?;
        let mut rows = statement
            .query([source_id.as_str()])
            .with_context(|| format!("failed reading source pages for {source_id}"))?;
        while let Some(row) = rows
            .next()
            .with_context(|| format!("failed reading source page row for {source_id}"))?
        {
            let page_index = row
                .get::<_, i64>(0)
                .context("read preserved source page index")?;
            pages.insert(
                (source_id.clone(), page_index.max(0) as usize),
                SourcePageSnapshotRow {
                    page_label: row.get(1).context("read preserved source page label")?,
                    markdown_path_redacted: row
                        .get(2)
                        .context("read preserved source page markdown path")?,
                    image_path_redacted: row
                        .get(3)
                        .context("read preserved source page image path")?,
                    plain_text: row
                        .get(4)
                        .context("read preserved source page plain text")?,
                    parse_warnings_json: row
                        .get(5)
                        .context("read preserved source page warnings")?,
                    snippets: Vec::new(),
                },
            );
        }
    }
    for source_id in &source_ids {
        sqlite
            .execute("DELETE FROM source_pages WHERE source_id = ?1", [source_id])
            .with_context(|| format!("failed clearing source pages for {source_id}"))?;
        sqlite
            .execute(
                "DELETE FROM source_page_fts WHERE source_id = ?1",
                [source_id],
            )
            .with_context(|| format!("failed clearing source page FTS for {source_id}"))?;
    }

    for evidence in &snapshot.evidence {
        let (Some(source_id), Some(page_index)) =
            (evidence.source_id.as_ref(), evidence.page_index)
        else {
            continue;
        };
        let row = pages
            .entry((source_id.clone(), page_index))
            .or_insert_with(|| SourcePageSnapshotRow {
                page_label: evidence.page_label.clone(),
                markdown_path_redacted: evidence
                    .markdown_path
                    .as_deref()
                    .map(redact_path_for_agent)
                    .unwrap_or_default(),
                image_path_redacted: evidence
                    .image_path
                    .as_deref()
                    .map(redact_path_for_agent)
                    .unwrap_or_default(),
                plain_text: String::new(),
                parse_warnings_json: "[]".into(),
                snippets: Vec::new(),
            });
        if row.page_label.is_empty() {
            row.page_label = evidence.page_label.clone();
        }
        if row.markdown_path_redacted.is_empty() {
            row.markdown_path_redacted = evidence
                .markdown_path
                .as_deref()
                .map(redact_path_for_agent)
                .unwrap_or_default();
        }
        if row.image_path_redacted.is_empty() {
            row.image_path_redacted = evidence
                .image_path
                .as_deref()
                .map(redact_path_for_agent)
                .unwrap_or_default();
        }
        if !evidence.snippet.trim().is_empty() {
            row.snippets.push(evidence.snippet.clone());
        }
    }

    for ((source_id, page_index), row) in pages {
        let plain_text = if !row.plain_text.trim().is_empty() {
            row.plain_text
        } else if row.snippets.is_empty() {
            String::new()
        } else {
            row.snippets.join("\n\n")
        };
        sqlite
            .execute(
                "INSERT INTO source_pages (
                    source_id,
                    page_index,
                    page_label,
                    markdown_path_redacted,
                    image_path_redacted,
                    plain_text,
                    parse_warnings_json
                ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                (
                    source_id.as_str(),
                    page_index as i64,
                    row.page_label.as_str(),
                    row.markdown_path_redacted.as_str(),
                    row.image_path_redacted.as_str(),
                    plain_text.as_str(),
                    row.parse_warnings_json.as_str(),
                ),
            )
            .with_context(|| {
                format!("failed inserting migrated source page {source_id}:{page_index}")
            })?;
        if !plain_text.trim().is_empty() {
            sqlite
                .execute(
                    "INSERT INTO source_page_fts (source_id, page_index, page_label, text)
                 VALUES (?1, ?2, ?3, ?4)",
                    (
                        source_id.as_str(),
                        page_index as i64,
                        row.page_label.as_str(),
                        plain_text.as_str(),
                    ),
                )
                .with_context(|| format!("failed indexing source page {source_id}:{page_index}"))?;
        }
    }

    Ok(())
}

pub(super) fn persist_evidence_snapshot_in_transaction(
    graph: &Graph,
    snapshot: &BrainRepoSnapshot,
) -> Result<()> {
    let sqlite = graph.connection().sqlite_connection();
    sqlite
        .execute(
            "DELETE FROM evidence_fts WHERE evidence_id IN (SELECT evidence_id FROM evidence_items WHERE workspace_id = ?1)",
            [snapshot.workspace_id.as_str()],
        )
        .context("failed clearing evidence FTS rows")?;
    sqlite
        .execute(
            "DELETE FROM evidence_items WHERE workspace_id = ?1",
            [snapshot.workspace_id.as_str()],
        )
        .context("failed clearing relational evidence rows")?;

    for evidence in &snapshot.evidence {
        let source_id = evidence.source_id.as_deref().unwrap_or_default();
        let page_index = evidence
            .page_index
            .map(|value| value.to_string())
            .unwrap_or_default();
        let source_path_redacted =
            redact_path_for_agent(evidence.source_path.as_deref().unwrap_or_default());
        let markdown_path_redacted =
            redact_path_for_agent(evidence.markdown_path.as_deref().unwrap_or_default());
        let image_path_redacted =
            redact_path_for_agent(evidence.image_path.as_deref().unwrap_or_default());
        sqlite
            .execute(
                "INSERT INTO evidence_items (
                    evidence_id,
                    workspace_id,
                    source_id,
                    page_index,
                    page_label,
                    evidence_type,
                    snippet,
                    source_path_redacted,
                    markdown_path_redacted,
                    image_path_redacted,
                    provenance,
                    status
                ) VALUES (?1, ?2, ?3, NULLIF(?4, ''), ?5, 'text_evidence', ?6, ?7, ?8, ?9, ?10, 'active')",
                (
                    evidence.id.as_str(),
                    snapshot.workspace_id.as_str(),
                    source_id,
                    page_index.as_str(),
                    evidence.page_label.as_str(),
                    evidence.snippet.as_str(),
                    source_path_redacted.as_str(),
                    markdown_path_redacted.as_str(),
                    image_path_redacted.as_str(),
                    evidence.provenance.as_deref().unwrap_or_default(),
                ),
            )
            .with_context(|| format!("failed inserting evidence row {}", evidence.id))?;
        sqlite
            .execute(
                "INSERT INTO evidence_fts (evidence_id, source_id, evidence_type, text)
                 VALUES (?1, ?2, 'text_evidence', ?3)",
                (evidence.id.as_str(), source_id, evidence.snippet.as_str()),
            )
            .with_context(|| format!("failed indexing evidence row {}", evidence.id))?;
    }

    Ok(())
}
