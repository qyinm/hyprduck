use std::path::Path;

use anyhow::Result;
use hyprduck_engine_types::{
    BrainNodeRecord, BrainRelationRecord, BrainRepoSnapshot, ClaimRecord, SourceArtifactManifest,
};
use serde::{Deserialize, Serialize};

use crate::retrieval::{
    build_retrieval_queries_from_import, retrieve_import_evidence, RetrievalQuery,
    RetrievedEvidenceChunk,
};
use crate::source_index::SourceChunk;

const NEW_SOURCE_CONTEXT_CHUNK_LIMIT: usize = 6;
const OLD_SOURCE_CONTEXT_CHUNK_LIMIT: usize = 20;
const GRAPH_CONTEXT_LIMIT: usize = 40;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ImportEvidenceContext {
    pub schema_version: u32,
    pub workspace_id: String,
    pub trigger_source_id: String,
    pub new_source: NewSourceContext,
    pub retrieval_queries: Vec<RetrievalQuery>,
    pub retrieved_source_evidence: Vec<RetrievedEvidenceChunk>,
    pub existing_graph_context: ExistingGraphContext,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NewSourceContext {
    pub source_id: String,
    pub source_title: String,
    pub source_path: String,
    pub markdown_path: String,
    pub chunks: Vec<RetrievedEvidenceChunk>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ExistingGraphContext {
    pub nodes: Vec<BrainNodeRecord>,
    pub edges: Vec<BrainRelationRecord>,
    pub claims: Vec<ClaimRecord>,
}

pub fn build_import_evidence_context(
    workspace_root: &Path,
    manifest: &SourceArtifactManifest,
    markdown: &str,
    snapshot: &BrainRepoSnapshot,
    source_chunks: &[SourceChunk],
) -> Result<ImportEvidenceContext> {
    let retrieval_queries = build_retrieval_queries_from_import(manifest, markdown);
    let retrieved_source_evidence = retrieve_import_evidence(
        workspace_root,
        &manifest.source_id,
        &retrieval_queries,
        OLD_SOURCE_CONTEXT_CHUNK_LIMIT,
    )?;
    Ok(ImportEvidenceContext {
        schema_version: 1,
        workspace_id: manifest.workspace_id.clone(),
        trigger_source_id: manifest.source_id.clone(),
        new_source: NewSourceContext {
            source_id: manifest.source_id.clone(),
            source_title: source_title(manifest),
            source_path: manifest.source_path.clone(),
            markdown_path: manifest.markdown_path.clone(),
            chunks: source_chunks
                .iter()
                .take(NEW_SOURCE_CONTEXT_CHUNK_LIMIT)
                .map(retrieved_chunk_from_source_chunk)
                .collect(),
        },
        retrieval_queries,
        retrieved_source_evidence,
        existing_graph_context: existing_graph_context(snapshot),
    })
}

pub fn import_evidence_context_allowed_refs(context: &ImportEvidenceContext) -> Vec<String> {
    let mut refs = Vec::new();
    for chunk in &context.new_source.chunks {
        refs.push(chunk.evidence_ref_id.clone());
    }
    for chunk in &context.retrieved_source_evidence {
        refs.push(chunk.evidence_ref_id.clone());
    }
    refs
}

fn retrieved_chunk_from_source_chunk(chunk: &SourceChunk) -> RetrievedEvidenceChunk {
    RetrievedEvidenceChunk {
        evidence_ref_id: format!("retrieved:{}:{}", chunk.source_id, chunk.chunk_id),
        chunk_id: chunk.chunk_id.clone(),
        source_id: chunk.source_id.clone(),
        source_title: chunk.source_title.clone(),
        source_path: chunk.source_path.clone(),
        markdown_path: chunk.markdown_path.clone(),
        heading_path: chunk.heading_path.clone(),
        line_start: chunk.line_start,
        line_end: chunk.line_end,
        matched_terms: Vec::new(),
        score: 0.0,
        text_hash: chunk.text_hash.clone(),
        text: chunk.text.clone(),
    }
}

fn existing_graph_context(snapshot: &BrainRepoSnapshot) -> ExistingGraphContext {
    ExistingGraphContext {
        nodes: snapshot
            .nodes
            .iter()
            .take(GRAPH_CONTEXT_LIMIT)
            .cloned()
            .collect(),
        edges: snapshot
            .relations
            .iter()
            .take(GRAPH_CONTEXT_LIMIT)
            .cloned()
            .collect(),
        claims: snapshot
            .claims
            .iter()
            .take(GRAPH_CONTEXT_LIMIT / 2)
            .cloned()
            .collect(),
    }
}

fn source_title(manifest: &SourceArtifactManifest) -> String {
    if !manifest.output_name.trim().is_empty() {
        return manifest.output_name.clone();
    }
    Path::new(&manifest.source_path)
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or(&manifest.source_id)
        .to_string()
}
