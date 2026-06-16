//! Internal helpers extracted from the engine facade module.

use anyhow::{Context, Result};
use graphqlite::Graph;
use hyprduck_engine_types::{
    ContextPackParseConfidence, EvidenceRef, EvidenceType, KnowledgeProject, SourceFormat,
    SourceRecord, SourceStatus, WikiPage,
};
use std::collections::BTreeMap;

use super::row_decode::non_empty_string;

#[derive(Debug, Clone)]
pub(crate) struct ContextPackEvidenceRow {
    pub(crate) evidence_id: String,
    pub(crate) source_id: String,
    pub(crate) page_index: Option<i64>,
    pub(crate) page_label: String,
    pub(crate) evidence_type: String,
    pub(crate) snippet: String,
    pub(crate) source_path_redacted: String,
    pub(crate) markdown_path_redacted: String,
    pub(crate) image_path_redacted: String,
    pub(crate) provenance: String,
    pub(crate) confidence: Option<f64>,
}

#[derive(Debug, Clone)]
pub(super) struct ContextPackSourceRow {
    pub(super) source_id: String,
    pub(super) workspace_id: String,
    pub(super) original_path: String,
    pub(super) source_path: String,
    pub(super) markdown_path: String,
    pub(super) original_path_redacted: String,
    pub(super) source_path_redacted: String,
    pub(super) markdown_path_redacted: String,
    pub(super) format: String,
    pub(super) status: String,
    pub(super) page_count: i64,
    pub(super) provider_route: String,
    pub(super) provider_locality: String,
    pub(super) content_hash: String,
    pub(super) updated_at: i64,
}

pub(crate) fn evidence_snippet_from_ids(evidence_ids: &[String]) -> String {
    if evidence_ids.is_empty() {
        String::new()
    } else {
        format!("Evidence refs: {}", evidence_ids.join(", "))
    }
}

pub(crate) fn load_context_pack_evidence_row(
    graph: &Graph,
    workspace_id: &str,
    evidence_id: &str,
) -> Result<Option<ContextPackEvidenceRow>> {
    let sqlite = graph.connection().sqlite_connection();
    let mut statement = sqlite
        .prepare(
            "SELECT
                evidence_id,
                source_id,
                page_index,
                page_label,
                evidence_type,
                snippet,
                source_path_redacted,
                markdown_path_redacted,
                image_path_redacted,
                provenance,
                confidence
             FROM evidence_items
             WHERE workspace_id = ?1
               AND evidence_id = ?2
               AND status = 'active'",
        )
        .context("failed preparing context pack evidence row query")?;
    let mut rows = statement
        .query((workspace_id, evidence_id))
        .context("failed querying context pack evidence row")?;
    let Some(row) = rows
        .next()
        .context("failed reading context pack evidence row")?
    else {
        return Ok(None);
    };
    Ok(Some(ContextPackEvidenceRow {
        evidence_id: row.get(0).context("read context evidence id")?,
        source_id: row.get(1).context("read context evidence source")?,
        page_index: row.get(2).context("read context evidence page index")?,
        page_label: row.get(3).context("read context evidence page label")?,
        evidence_type: row.get(4).context("read context evidence type")?,
        snippet: row.get(5).context("read context evidence snippet")?,
        source_path_redacted: row.get(6).context("read context evidence source path")?,
        markdown_path_redacted: row.get(7).context("read context evidence markdown path")?,
        image_path_redacted: row.get(8).context("read context evidence image path")?,
        provenance: row.get(9).context("read context evidence provenance")?,
        confidence: row.get(10).context("read context evidence confidence")?,
    }))
}

pub(super) fn load_context_pack_source_row(
    graph: &Graph,
    workspace_id: &str,
    source_id: &str,
) -> Result<Option<ContextPackSourceRow>> {
    let sqlite = graph.connection().sqlite_connection();
    let mut statement = sqlite
        .prepare(
            "SELECT
                source_id,
                workspace_id,
                original_path,
                source_path,
                markdown_path,
                original_path_redacted,
                source_path_redacted,
                markdown_path_redacted,
                format,
                status,
                page_count,
                provider_route,
                provider_locality,
                content_hash,
                updated_at
             FROM sources
             WHERE workspace_id = ?1
               AND source_id = ?2
               AND status NOT IN ('failed', 'stale', 'hash_mismatched', 'unapproved')",
        )
        .context("failed preparing context pack source row query")?;
    let mut rows = statement
        .query((workspace_id, source_id))
        .context("failed querying context pack source row")?;
    let Some(row) = rows
        .next()
        .context("failed reading context pack source row")?
    else {
        return Ok(None);
    };
    Ok(Some(ContextPackSourceRow {
        source_id: row.get(0).context("read context source id")?,
        workspace_id: row.get(1).context("read context source workspace")?,
        original_path: row.get(2).context("read context source original path")?,
        source_path: row.get(3).context("read context source source path")?,
        markdown_path: row.get(4).context("read context source markdown path")?,
        original_path_redacted: row
            .get(5)
            .context("read context source original path redacted")?,
        source_path_redacted: row
            .get(6)
            .context("read context source source path redacted")?,
        markdown_path_redacted: row
            .get(7)
            .context("read context source markdown path redacted")?,
        format: row.get(8).context("read context source format")?,
        status: row.get(9).context("read context source status")?,
        page_count: row.get(10).context("read context source page count")?,
        provider_route: row.get(11).context("read context source provider route")?,
        provider_locality: row
            .get(12)
            .context("read context source provider locality")?,
        content_hash: row.get(13).context("read context source content hash")?,
        updated_at: row.get(14).context("read context source updated at")?,
    }))
}

pub(super) fn load_context_pack_evidence_rows_for_source(
    graph: &Graph,
    workspace_id: &str,
    source_id: &str,
    page_index: Option<i64>,
) -> Result<Vec<ContextPackEvidenceRow>> {
    let sqlite = graph.connection().sqlite_connection();
    let mut statement = sqlite
        .prepare(
            "SELECT
                evidence_id,
                source_id,
                page_index,
                page_label,
                evidence_type,
                snippet,
                source_path_redacted,
                markdown_path_redacted,
                image_path_redacted,
                provenance,
                confidence
             FROM evidence_items
             WHERE workspace_id = ?1
               AND source_id = ?2
               AND (?3 IS NULL OR page_index = ?3)
               AND status = 'active'
             ORDER BY page_index ASC, evidence_id ASC",
        )
        .context("failed preparing source evidence rows query")?;
    let mut rows = statement
        .query((workspace_id, source_id, page_index))
        .context("failed querying source evidence rows")?;
    let mut evidence_rows = Vec::new();
    while let Some(row) = rows.next().context("failed reading source evidence row")? {
        evidence_rows.push(ContextPackEvidenceRow {
            evidence_id: row.get(0).context("read source evidence id")?,
            source_id: row.get(1).context("read source evidence source")?,
            page_index: row.get(2).context("read source evidence page index")?,
            page_label: row.get(3).context("read source evidence page label")?,
            evidence_type: row.get(4).context("read source evidence type")?,
            snippet: row.get(5).context("read source evidence snippet")?,
            source_path_redacted: row.get(6).context("read source evidence source path")?,
            markdown_path_redacted: row.get(7).context("read source evidence markdown path")?,
            image_path_redacted: row.get(8).context("read source evidence image path")?,
            provenance: row.get(9).context("read source evidence provenance")?,
            confidence: row.get(10).context("read source evidence confidence")?,
        });
    }
    Ok(evidence_rows)
}

pub(super) fn load_evidence_refs_for_source(
    graph: &Graph,
    workspace_id: &str,
    source_id: &str,
    page_index: Option<i64>,
) -> Result<Vec<EvidenceRef>> {
    Ok(
        load_context_pack_evidence_rows_for_source(graph, workspace_id, source_id, page_index)?
            .into_iter()
            .map(|row| EvidenceRef {
                id: row.evidence_id,
                page_label: row.page_label,
                page_index: row.page_index.map(|page_index| page_index.max(0) as usize),
                snippet: row.snippet,
                source_path: non_empty_string(row.source_path_redacted),
                source_id: Some(row.source_id),
                markdown_path: non_empty_string(row.markdown_path_redacted),
                image_path: non_empty_string(row.image_path_redacted),
                provenance: non_empty_string(row.provenance),
            })
            .collect(),
    )
}

pub(super) fn load_evidence_refs_by_ids(
    graph: &Graph,
    workspace_id: &str,
    evidence_ids: &[String],
) -> Result<Vec<EvidenceRef>> {
    let mut evidence = Vec::new();
    for evidence_id in evidence_ids {
        if let Some(row) = load_context_pack_evidence_row(graph, workspace_id, evidence_id)? {
            evidence.push(EvidenceRef {
                id: row.evidence_id,
                page_label: row.page_label,
                page_index: row.page_index.map(|page_index| page_index.max(0) as usize),
                snippet: row.snippet,
                source_path: non_empty_string(row.source_path_redacted),
                source_id: Some(row.source_id),
                markdown_path: non_empty_string(row.markdown_path_redacted),
                image_path: non_empty_string(row.image_path_redacted),
                provenance: non_empty_string(row.provenance),
            });
        }
    }
    Ok(evidence)
}

pub(super) fn load_wiki_page_for_source(
    graph: &Graph,
    workspace_id: &str,
    source_id: &str,
) -> Result<Option<WikiPage>> {
    let sqlite = graph.connection().sqlite_connection();
    let mut statement = sqlite
        .prepare(
            "SELECT wiki_page_id, path, title, body, evidence_refs_json, updated_at
             FROM wiki_pages
             WHERE workspace_id = ?1
               AND approval_status IN ('materialized', 'approved')
               AND valid_to <= 0
               AND evidence_refs_json <> '[]'
             ORDER BY updated_at DESC",
        )
        .context("failed preparing wiki page source query")?;
    let mut rows = statement
        .query([workspace_id])
        .context("failed querying wiki page source rows")?;
    while let Some(row) = rows.next().context("failed reading wiki page source row")? {
        let evidence_refs_json: String = row.get(4).context("read wiki evidence refs")?;
        let evidence_refs =
            serde_json::from_str::<Vec<String>>(&evidence_refs_json).unwrap_or_default();
        if !wiki_evidence_refs_source(graph, workspace_id, &evidence_refs, source_id)? {
            continue;
        }
        return Ok(Some(WikiPage {
            page_id: row.get(0).context("read wiki id")?,
            workspace_id: workspace_id.into(),
            path: row.get(1).context("read wiki path")?,
            title: row.get(2).context("read wiki title")?,
            body: row.get(3).context("read wiki body")?,
            node_refs: Vec::new(),
            source_refs: vec![source_id.into()],
            evidence_refs,
            updated_at: row.get::<_, i64>(5).context("read wiki updated at")?.max(0) as u64,
        }));
    }
    Ok(None)
}

pub(super) fn load_wiki_page_by_path(
    graph: &Graph,
    workspace_id: &str,
    path: &str,
) -> Result<Option<WikiPage>> {
    let sqlite = graph.connection().sqlite_connection();
    let mut statement = sqlite
        .prepare(
            "SELECT wiki_page_id, title, body, evidence_refs_json, updated_at
             FROM wiki_pages
             WHERE workspace_id = ?1
               AND path = ?2
               AND approval_status IN ('materialized', 'approved')
               AND valid_to <= 0
             LIMIT 1",
        )
        .context("failed preparing wiki page by path query")?;
    let mut rows = statement
        .query((workspace_id, path))
        .context("failed querying wiki page by path")?;
    let Some(row) = rows.next().context("failed reading wiki page by path")? else {
        return Ok(None);
    };
    Ok(Some(WikiPage {
        page_id: row.get(0).context("read wiki id")?,
        workspace_id: workspace_id.into(),
        path: path.into(),
        title: row.get(1).context("read wiki title")?,
        body: row.get(2).context("read wiki body")?,
        node_refs: Vec::new(),
        source_refs: Vec::new(),
        evidence_refs: serde_json::from_str::<Vec<String>>(
            &row.get::<_, String>(3).context("read wiki evidence refs")?,
        )
        .unwrap_or_default(),
        updated_at: row.get::<_, i64>(4).context("read wiki updated at")?.max(0) as u64,
    }))
}

fn wiki_evidence_refs_source(
    graph: &Graph,
    workspace_id: &str,
    evidence_refs: &[String],
    source_id: &str,
) -> Result<bool> {
    for evidence_ref in evidence_refs {
        if load_context_pack_evidence_row(graph, workspace_id, evidence_ref)?
            .is_some_and(|row| row.source_id == source_id)
        {
            return Ok(true);
        }
    }
    Ok(false)
}

pub(super) fn source_record_from_context_row(
    source: ContextPackSourceRow,
    include_local_paths: bool,
) -> SourceRecord {
    SourceRecord {
        source_id: source.source_id,
        workspace_id: source.workspace_id,
        original_path: if include_local_paths {
            source.original_path
        } else {
            source.original_path_redacted
        },
        source_path: if include_local_paths {
            source.source_path
        } else {
            source.source_path_redacted
        },
        markdown_path: if include_local_paths {
            source.markdown_path
        } else {
            source.markdown_path_redacted
        },
        format: SourceFormat::from(source.format),
        status: SourceStatus::from(source.status),
        page_count: source.page_count.max(0) as usize,
        description: String::new(),
        user_context: String::new(),
        ingest_instruction: String::new(),
        updated_at: source.updated_at.max(0) as u64,
    }
}

pub(super) fn db_parse_confidence(confidence: Option<f64>) -> ContextPackParseConfidence {
    match confidence {
        Some(value) if value >= 0.8 => ContextPackParseConfidence::High,
        Some(value) if value >= 0.5 => ContextPackParseConfidence::Medium,
        Some(_) => ContextPackParseConfidence::Low,
        None => ContextPackParseConfidence::Unknown,
    }
}

pub(super) fn db_context_evidence_type(evidence_type: &str) -> EvidenceType {
    match evidence_type {
        "text_evidence" => EvidenceType::Text,
        "table_evidence" => EvidenceType::Table,
        "image_region_evidence" => EvidenceType::ImageRegion,
        "ocr_evidence" => EvidenceType::Ocr,
        "caption_evidence" => EvidenceType::Caption,
        "summary_evidence" | "wiki_evidence" => EvidenceType::Summary,
        "claim_evidence" => EvidenceType::Claim,
        "relationship_evidence" => EvidenceType::Relationship,
        _ => EvidenceType::Unknown,
    }
}

pub(super) fn unique_project_evidence(project: &KnowledgeProject) -> Vec<EvidenceRef> {
    let mut evidence_by_id = BTreeMap::new();
    for evidence in project
        .details_by_node_id
        .values()
        .flat_map(|detail| detail.evidence.iter())
        .chain(
            project
                .edge_details_by_id
                .values()
                .flat_map(|detail| detail.evidence.iter()),
        )
    {
        evidence_by_id
            .entry(evidence.id.clone())
            .or_insert_with(|| evidence.clone());
    }
    evidence_by_id.into_values().collect()
}
