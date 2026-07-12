use crate::blob::{get_verified, BlobStore};
use crate::graph::{GraphStore, UpsertClaim, UpsertNode, UpsertRelation};
use crate::knowledge::{EvidenceRow, EvidenceWrite, KnowledgeStore, SourceRow};

const MAX_EVIDENCE_CHUNKS: usize = 16;

pub async fn materialize_source(
    knowledge: &KnowledgeStore,
    graph: &GraphStore,
    blobs: &dyn BlobStore,
    source: &SourceRow,
) -> Result<usize, String> {
    let bytes = get_verified(blobs, &source.blob_key).map_err(|error| error.to_string())?;
    let text = std::str::from_utf8(&bytes)
        .map_err(|error| format!("source blob is not valid UTF-8: {error}"))?;
    if text.trim().is_empty() {
        return Err("source blob is empty".into());
    }
    if !is_supported_content_type(&source.content_type) {
        return Err(format!(
            "unsupported source content type for server materialization: {}",
            source.content_type
        ));
    }

    let chunks = evidence_chunks(text);
    let warnings = parse_warnings(source, text);
    let mut evidence_rows = Vec::new();
    for (idx, chunk) in chunks.iter().enumerate() {
        let page = (idx + 1) as i32;
        let locator = format!("page:{page}");
        let evidence_id = stable_evidence_id(&source.id, idx);
        let row = knowledge
            .upsert_evidence_record(&EvidenceWrite {
                workspace_id: &source.workspace_id,
                id: &evidence_id,
                source_id: &source.id,
                source_kind: &source.kind,
                quote: chunk,
                locator: &locator,
                evidence_type: evidence_type_for_source_kind(&source.kind),
                page: Some(page),
                region: Some(&locator),
                span_start: None,
                span_end: None,
                parse_warnings: &warnings,
                retrieval_text: Some(chunk),
            })
            .await
            .map_err(|error| error.to_string())?;
        evidence_rows.push(row);
    }

    project_source_graph(graph, source, &evidence_rows).await?;
    Ok(evidence_rows.len())
}

fn is_supported_content_type(content_type: &str) -> bool {
    let lower = content_type.to_ascii_lowercase();
    lower.starts_with("text/")
        || lower.contains("markdown")
        || lower.contains("json")
        || lower.contains("xml")
}

fn evidence_chunks(text: &str) -> Vec<String> {
    let mut chunks: Vec<String> = text
        .split("\n\n")
        .map(str::trim)
        .filter(|chunk| !chunk.is_empty())
        .take(MAX_EVIDENCE_CHUNKS)
        .map(ToOwned::to_owned)
        .collect();
    if chunks.is_empty() {
        chunks.push(text.trim().to_owned());
    }
    chunks
}

fn parse_warnings(source: &SourceRow, text: &str) -> Vec<String> {
    let mut warnings = Vec::new();
    if text.len() > 64 * 1024 {
        warnings.push("source truncated to first materialization chunks".to_string());
    }
    if source.content_type.to_ascii_lowercase().contains("json")
        && !text.trim_start().starts_with(['{', '['])
    {
        warnings.push("json content type did not start with a JSON container".to_string());
    }
    warnings
}

fn stable_evidence_id(source_id: &str, index: usize) -> String {
    if index == 0 {
        format!("ev_{source_id}_root")
    } else {
        format!("ev_{source_id}_{index}")
    }
}

fn evidence_type_for_source_kind(kind: &str) -> &'static str {
    match kind {
        "issue" | "pull_request" => "claim",
        _ => "text_evidence",
    }
}

async fn project_source_graph(
    graph: &GraphStore,
    source: &SourceRow,
    evidence: &[EvidenceRow],
) -> Result<(), String> {
    if evidence.is_empty() {
        return Ok(());
    }
    let version = materialized_version();
    let source_node_id = format!("source:{}", source.id);
    let evidence_ids: Vec<String> = evidence.iter().map(|row| row.id.clone()).collect();
    graph
        .upsert_live_node(
            &source.workspace_id,
            &UpsertNode {
                logical_id: source_node_id.clone(),
                kind: "source".into(),
                label: source.title.clone(),
                scope: "workspace".into(),
                aliases: vec![source.title.clone()],
                evidence_ids: evidence_ids.clone(),
                source_ids: vec![source.id.clone()],
                confidence: Some(1.0),
                created_by_event_id: Some(format!("materialize:{}", source.id)),
                valid_from: version,
            },
        )
        .await
        .map_err(|error| error.to_string())?;

    for (idx, row) in evidence.iter().enumerate() {
        let evidence_node_id = format!("evidence:{}", row.id);
        graph
            .upsert_live_node(
                &source.workspace_id,
                &UpsertNode {
                    logical_id: evidence_node_id.clone(),
                    kind: "evidence".into(),
                    label: summarize_label(&row.quote),
                    scope: "workspace".into(),
                    aliases: vec![],
                    evidence_ids: vec![row.id.clone()],
                    source_ids: vec![source.id.clone()],
                    confidence: Some(0.9),
                    created_by_event_id: Some(format!("materialize:{}", source.id)),
                    valid_from: version + idx as i64 + 1,
                },
            )
            .await
            .map_err(|error| error.to_string())?;
        graph
            .upsert_live_relation(
                &source.workspace_id,
                &UpsertRelation {
                    logical_id: format!("source_evidence:{}:{}", source.id, row.id),
                    kind: "contains_evidence".into(),
                    source_logical_id: source_node_id.clone(),
                    target_logical_id: evidence_node_id,
                    label: "contains evidence".into(),
                    evidence_ids: vec![row.id.clone()],
                    confidence: Some(0.9),
                    created_by_event_id: Some(format!("materialize:{}", source.id)),
                    valid_from: version + idx as i64 + 100,
                },
            )
            .await
            .map_err(|error| error.to_string())?;
    }

    if let Some(first) = evidence.first() {
        graph
            .upsert_live_claim(
                &source.workspace_id,
                &UpsertClaim {
                    logical_id: format!("claim:{}", first.id),
                    statement: summarize_label(&first.quote),
                    status: "open".into(),
                    topic_refs: vec![source_node_id],
                    source_refs: vec![source.id.clone()],
                    evidence_refs: vec![first.id.clone()],
                    created_by_event_id: Some(format!("materialize:{}", source.id)),
                    valid_from: version + 10_000,
                },
            )
            .await
            .map_err(|error| error.to_string())?;
    }

    Ok(())
}

fn summarize_label(text: &str) -> String {
    let normalized = text.split_whitespace().collect::<Vec<_>>().join(" ");
    normalized.chars().take(96).collect()
}

fn materialized_version() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_millis() as i64)
        .unwrap_or(0)
}
