use std::collections::BTreeSet;
use std::path::Path;

use crate::source_index::SourceChunk;
use crate::*;

pub(crate) fn build_source_chunk_graph_prompt(
    workspace_id: &str,
    manifest: &SourceArtifactManifest,
    chunks: &[SourceChunk],
    batch_evidence: &[EvidenceRef],
    snapshot: &BrainRepoSnapshot,
    context: &ImportEvidenceContext,
) -> Result<String> {
    let source = snapshot
        .sources
        .iter()
        .find(|source| source.source_id == manifest.source_id)
        .cloned()
        .into_iter()
        .collect::<Vec<_>>();
    let sources_json =
        serde_json::to_string_pretty(&source).context("failed to encode imported source")?;
    let evidence_json = serde_json::to_string_pretty(batch_evidence)
        .context("failed to encode imported evidence")?;
    let chunks_json =
        serde_json::to_string_pretty(chunks).context("failed to encode imported source chunks")?;
    let context_refs = import_evidence_context_allowed_refs(context);

    Ok(format!(
        r#"You are HyprDuck's source-local graph candidate extraction agent.

Task:
- Extract high-signal graph candidates only for the newly imported sourceId {source_id}.
- This request covers a bounded subset of source chunks, not the full document.
- Return candidate records grounded only in the provided source, evidence, and chunks.
- Include one source node using nodeId "source:{source_id}".
- If these chunks contain meaningful domain text, create only the most important non-source concept/topic nodes.
- Return at most 8 non-source concept/topic nodes for this chunk batch.
- Return at most 10 non-source relations for this chunk batch.
- Return at most 3 claims for this chunk batch.
- Return memories as [] unless the source explicitly states a durable decision or invariant.
- Return wikiPages as []; HyprDuck synthesizes wiki summaries after canonicalization.
- Do not emit source_of edges. HyprDuck will add canonical source_of edges after dedupe.
- Do not create nodes solely for document scaffolding headings unless the cited evidence shows they are actual domain concepts.
- Do not perform exhaustive term extraction. Prefer fewer high-signal concepts over broad coverage.
- Every non-source node must cite at least one existing evidenceId from sourceId {source_id}.
- Every non-source relation and claim must cite existing evidenceIds from the imported source.
- Preserve source and evidence records exactly as provided. Do not invent sourceIds or evidenceIds.
- Use stable raw nodeIds/relationIds/claimIds so repeated chunk runs remain readable. HyprDuck will assign canonical durable IDs later.
- Return JSON only. No markdown fence, no prose.

Output shape:
{{
  "materializedGraph": {{
    "generatedAt": <unix seconds or null>,
    "sources": [...copy exactly from Imported source],
    "evidence": [...copy exactly from Imported evidence],
    "nodes": [BrainNodeRecord...],
    "edges": [BrainRelationRecord...],
    "claims": [ClaimRecord...],
    "memories": [MemoryRecord...],
    "wikiPages": [WikiPage...],
    "entities": [],
    "extractions": []
  }}
}}

Workspace:
- workspaceId: {workspace_id}
- imported sourceId: {source_id}
- imported sourcePath: {source_path}
- imported markdownPath: {markdown_path}
- allowed context evidence refs from retrieval: {context_refs}

Imported source:
{sources_json}

Imported evidence:
{evidence_json}

Imported source chunks:
{chunks_json}
"#,
        workspace_id = workspace_id,
        source_id = manifest.source_id,
        source_path = manifest.source_path,
        markdown_path = manifest.markdown_path,
        context_refs = join_or_none(&context_refs),
        sources_json = sources_json,
        evidence_json = evidence_json,
        chunks_json = chunks_json,
    ))
}

pub(crate) fn build_workspace_linking_prompt(
    workspace_root: &Path,
    workspace_id: &str,
    manifest: &SourceArtifactManifest,
    markdown: &str,
    snapshot: &BrainRepoSnapshot,
    context: &ImportEvidenceContext,
) -> Result<String> {
    let valid_source_ids = snapshot
        .sources
        .iter()
        .map(|source| source.source_id.as_str())
        .collect::<BTreeSet<_>>();
    let chunks = select_workspace_linking_candidate_chunks(
        read_workspace_source_chunks(workspace_root)?
            .into_iter()
            .filter(|chunk| valid_source_ids.contains(chunk.source_id.as_str()))
            .collect(),
        &manifest.source_id,
        markdown,
    );
    let current_graph_json = serde_json::to_string_pretty(&json!({
        "nodes": snapshot.nodes,
        "edges": snapshot.relations,
        "claims": snapshot.claims,
        "memories": snapshot.memories,
        "wikiPages": snapshot.wiki_pages,
    }))
    .context("failed to encode current graph")?;
    let all_chunks = chunks
        .iter()
        .map(|chunk| {
            format!(
                "- sourceId: {}, chunkId: {}, title: {}, path: {}, heading: {}, lines: {}-{}, text: {}",
                chunk.source_id,
                chunk.chunk_id,
                chunk.source_title,
                chunk.markdown_path,
                join_or_none(&chunk.heading_path),
                chunk.line_start,
                chunk.line_end,
                chunk.text
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    let sources_json =
        serde_json::to_string_pretty(&snapshot.sources).context("failed to encode sources")?;
    let evidence_json =
        serde_json::to_string_pretty(&snapshot.evidence).context("failed to encode evidence")?;
    let context_refs = import_evidence_context_allowed_refs(context);

    Ok(format!(
        r#"You are HyprDuck's workspace linking agent.

Task:
- The imported sourceId {source_id} already has its own source-local graph.
- Add only meaningful cross-source links between the imported source graph and the existing workspace graph.
- Do not rebuild, replace, or delete existing nodes, edges, claims, memories, wiki pages, sources, or evidence.
- Prefer edges where one endpoint is grounded in sourceId {source_id} and the other endpoint is grounded in a different source.
- Actively look for grounded cross-source links such as shared concepts, prerequisites, contrasts, refinements, dependencies, repeated claims, or related methods.
- Return only records needed for cross-source linking. Return no new edges only when no grounded relationship exists after comparing the imported markdown with the workspace chunks.
- Return at most 24 cross-source relations and 8 claims.
- Return memories as [].
- Return wikiPages as []; HyprDuck synthesizes wiki pages after validated linking.
- Every returned edge and claim must cite existing sourceIds/evidenceIds.
- Every returned edge must cite evidence from both endpoint source sides: at least one evidenceId from sourceId {source_id} and at least one evidenceId from the other endpoint's sourceIds.
- Do not return sources, evidence, nodes, entities, or extractions. Endpoint nodes must already exist in the current graph.
- Do not return source_of edges. HyprDuck owns source edges.
- Do not invent sourceIds, evidenceIds, or nodeIds.
- Use stable ids so repeated linking runs remain readable.
- Return JSON only. No markdown fence, no prose.

Output shape:
{{
    "materializedGraph": {{
    "generatedAt": <unix seconds or null>,
    "edges": [cross-source BrainRelationRecord...],
    "claims": [cross-source ClaimRecord...],
    "memories": [],
    "wikiPages": []
  }}
}}

Workspace:
- workspaceId: {workspace_id}
- imported sourceId: {source_id}
- imported sourcePath: {source_path}
- imported markdownPath: {markdown_path}
- allowed context evidence refs from retrieval: {context_refs}

Provided sources:
{sources_json}

Provided evidence:
{evidence_json}

Current materialized graph after source-local graph:
{current_graph_json}

All workspace source chunks:
{all_chunks}

Latest imported markdown:
{markdown}
"#,
        workspace_id = workspace_id,
        source_id = manifest.source_id,
        source_path = manifest.source_path,
        markdown_path = manifest.markdown_path,
        context_refs = join_or_none(&context_refs),
        sources_json = sources_json,
        evidence_json = evidence_json,
        current_graph_json = current_graph_json,
        all_chunks = if all_chunks.is_empty() {
            "(none)".into()
        } else {
            all_chunks
        },
        markdown = truncate_for_prompt(markdown, 16000)
    ))
}

fn select_workspace_linking_candidate_chunks(
    chunks: Vec<SourceChunk>,
    imported_source_id: &str,
    imported_markdown: &str,
) -> Vec<SourceChunk> {
    const MAX_WORKSPACE_LINKING_CHUNKS: usize = 24;
    let query_terms = search_terms(imported_markdown)
        .into_iter()
        .take(80)
        .collect::<BTreeSet<_>>();
    let mut scored = chunks
        .into_iter()
        .filter(|chunk| chunk.source_id != imported_source_id)
        .map(|chunk| {
            let text = format!(
                "{} {} {}",
                chunk.source_title,
                chunk.heading_path.join(" "),
                chunk.text
            );
            let score = search_terms(&text)
                .into_iter()
                .filter(|term| query_terms.contains(term))
                .count();
            (score, chunk)
        })
        .filter(|(score, _)| *score > 0)
        .collect::<Vec<_>>();
    scored.sort_by(|left, right| {
        right
            .0
            .cmp(&left.0)
            .then(left.1.source_id.cmp(&right.1.source_id))
            .then(left.1.line_start.cmp(&right.1.line_start))
    });
    scored
        .into_iter()
        .take(MAX_WORKSPACE_LINKING_CHUNKS)
        .map(|(_, chunk)| chunk)
        .collect()
}

fn truncate_for_prompt(value: &str, max_chars: usize) -> String {
    let mut output = value.chars().take(max_chars).collect::<String>();
    if value.chars().count() > max_chars {
        output.push_str("\n...[truncated]");
    }
    output
}
