//! Query-time context pack assembly from the knowledge DB.

use anyhow::{Context, Result};
use graphqlite::Graph;
use etyma_engine_types::{
    BrainContextPack, ContextPackArtifactMetadataV0, ContextPackEvidenceMetadataV0,
    ContextPackSourceMetadataV0, ContextPackV1, EvidenceRef, SourceFormat, SourceRecord,
    SourceStatus,
};
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use super::context_pack_store::{
    db_context_evidence_type, db_parse_confidence, load_context_pack_evidence_row,
    load_context_pack_source_row, ContextPackEvidenceRow,
};
use super::graph_trail_store::{
    attach_context_pack_graph_trails, graph_trail_unavailable_warning,
};
use super::row_decode::non_empty_string;
use crate::domains::retrieval::brain_search::{
    db_context_window, db_search_terms, hybrid_retrieve_from_db,
};

pub(crate) fn assemble_context_pack_v1_from_db(
    path: &Path,
    workspace_id: &str,
    query: &str,
    budget: usize,
    pack_id: String,
    generated_at: String,
) -> Result<(BrainContextPack, ContextPackV1)> {
    let limit = budget.clamp(1, 24);
    let terms = db_search_terms(query);
    let hits = hybrid_retrieve_from_db(path, workspace_id, query, limit)
        .context("failed retrieving DB-backed context pack evidence")?;
    let graph = Graph::open(path).context("GraphQLite failed to open knowledge DB")?;

    let mut evidence_rows = Vec::new();
    for hit in &hits {
        if let Some(row) =
            load_context_pack_evidence_row(&graph, workspace_id, &hit.evidence_id)?
        {
            evidence_rows.push((row, hit.quoted_text.clone()));
        }
    }

    let source_ids = evidence_rows
        .iter()
        .map(|(row, _)| row.source_id.clone())
        .collect::<BTreeSet<_>>();
    let mut sources = Vec::new();
    let mut source_metadata = BTreeMap::new();
    for source_id in source_ids {
        let Some(source) = load_context_pack_source_row(&graph, workspace_id, &source_id)?
        else {
            continue;
        };
        source_metadata.insert(
            source.source_id.clone(),
            ContextPackSourceMetadataV0 {
                content_hash: source.content_hash.clone(),
                provider_route: source.provider_route.clone(),
                local_only: source.provider_locality != "hosted",
            },
        );
        sources.push(SourceRecord {
            source_id: source.source_id,
            workspace_id: source.workspace_id,
            original_path: source.original_path_redacted,
            source_path: source.source_path_redacted,
            markdown_path: source.markdown_path_redacted,
            format: SourceFormat::from(source.format),
            status: SourceStatus::from(source.status),
            page_count: source.page_count.max(0) as usize,
            description: String::new(),
            user_context: String::new(),
            ingest_instruction: String::new(),
            updated_at: source.updated_at.max(0) as u64,
        });
    }

    let mut evidence_metadata: BTreeMap<
        String,
        BTreeMap<String, ContextPackEvidenceMetadataV0>,
    > = BTreeMap::new();
    let evidence = evidence_rows
        .into_iter()
        .filter_map(|(row, quoted_text_override)| {
            if !source_metadata.contains_key(&row.source_id) {
                return None;
            }
            let page = row.page_index.unwrap_or(0).max(0) as usize + 1;
            let mut quoted_text = quoted_text_override
                .filter(|value| !value.trim().is_empty())
                .unwrap_or_else(|| row.snippet.clone());
            if let Some(page_text) = source_page_plain_text_for_evidence(&graph, &row) {
                let page_context = db_context_window(&page_text, &terms, 1_600);
                if !page_context.trim().is_empty() && page_context.trim() != quoted_text.trim()
                {
                    quoted_text = page_context;
                }
            }
            let metadata = ContextPackEvidenceMetadataV0 {
                source_id: row.source_id.clone(),
                page,
                region: None,
                span: None,
                quoted_text: quoted_text.clone(),
                parse_confidence: db_parse_confidence(row.confidence),
                content_hash: source_metadata
                    .get(&row.source_id)
                    .map(|metadata| metadata.content_hash.clone())
                    .unwrap_or_default(),
                markdown_path: non_empty_string(row.markdown_path_redacted.clone()),
                image_path: non_empty_string(row.image_path_redacted.clone()),
                evidence_type: db_context_evidence_type(&row.evidence_type),
            };
            evidence_metadata
                .entry(row.source_id.clone())
                .or_default()
                .insert(row.evidence_id.clone(), metadata);
            Some(EvidenceRef {
                id: row.evidence_id,
                page_label: row.page_label,
                page_index: row.page_index.map(|page_index| page_index.max(0) as usize),
                snippet: quoted_text,
                source_path: non_empty_string(row.source_path_redacted),
                source_id: Some(row.source_id),
                markdown_path: non_empty_string(row.markdown_path_redacted),
                image_path: non_empty_string(row.image_path_redacted),
                provenance: non_empty_string(row.provenance),
            })
        })
        .collect::<Vec<_>>();

    let mut warnings = Vec::new();
    if evidence.is_empty() {
        warnings.push("No active DB evidence matched the Context Pack query.".into());
    }

    let pack = BrainContextPack {
        workspace_id: workspace_id.into(),
        query: query.into(),
        token_budget: budget,
        summary: format!(
            "Context assembled from {} DB evidence item(s) using SQLite FTS5 retrieval and GraphQLite graph expansion.",
            evidence.len()
        ),
        wiki_pages: Vec::new(),
        nodes: Vec::new(),
        sources,
        memories: Vec::new(),
        entities: Vec::new(),
        claims: Vec::new(),
        relations: Vec::new(),
        evidence,
        recent_events: Vec::new(),
        warnings,
    };
    let artifact_metadata = ContextPackArtifactMetadataV0 {
        sources: source_metadata,
        evidence: evidence_metadata,
        warnings: Vec::new(),
    };
    let mut context_pack = ContextPackV1::from_brain_context_pack(
        &pack,
        pack_id,
        generated_at,
        &artifact_metadata,
    );
    context_pack.retrieval_trace.strategy = "sqlite-graphqlite-fts5-hybrid".into();
    if let Err(error) =
        attach_context_pack_graph_trails(&graph, workspace_id, &mut context_pack)
    {
        context_pack
            .warnings
            .push(graph_trail_unavailable_warning(&format!(
                "Graph trail unavailable for the selected evidence; citation evidence remains available. {error}"
            )));
    }
    if !context_pack.selected_evidence.is_empty()
        && context_pack
            .selected_evidence
            .iter()
            .all(|evidence| evidence.graph_trail.is_none())
    {
        context_pack
            .warnings
            .push(graph_trail_unavailable_warning(
                "Graph trail unavailable for the selected evidence; citation evidence remains available.",
            ));
    }
    Ok((pack, context_pack))
}

fn source_page_plain_text_for_evidence(
    graph: &Graph,
    row: &ContextPackEvidenceRow,
) -> Option<String> {
    let page_index = row.page_index?;
    let sqlite = graph.connection().sqlite_connection();
    let mut statement = sqlite
        .prepare(
            "SELECT plain_text
             FROM source_pages
             WHERE source_id = ?1
               AND page_index = ?2
             LIMIT 1",
        )
        .ok()?;
    let mut rows = statement.query((&row.source_id, page_index)).ok()?;
    let row = rows.next().ok()??;
    let text = row.get::<_, String>(0).ok()?;
    (!text.trim().is_empty()).then_some(text)
}

