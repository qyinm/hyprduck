use std::collections::BTreeSet;
use std::path::Path;

use super::super::*;

pub(crate) fn build_full_workspace_graph_rebuild_prompt(
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
    let chunks = read_workspace_source_chunks(workspace_root)?
        .into_iter()
        .filter(|chunk| valid_source_ids.contains(chunk.source_id.as_str()))
        .collect::<Vec<_>>();
    let sources_json =
        serde_json::to_string_pretty(&snapshot.sources).context("failed to encode sources")?;
    let evidence_json =
        serde_json::to_string_pretty(&snapshot.evidence).context("failed to encode evidence")?;
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
    let context_refs = import_evidence_context_allowed_refs(context);

    Ok(format!(
        r#"You are HyprDuck's autonomous workspace graph rebuild agent.

Task:
- Rebuild the entire workspace graph after importing sourceId {source_id}.
- Use every workspace source chunk below, not only the latest import.
- Prioritize the latest imported markdown and make sure its domain content is represented.
- Return the complete materialized graph state, not proposals.
- Decide all durable nodes, edges, claims, memories, and wiki pages yourself from the source evidence.
- Reconnect or restructure the workspace graph when the full source set proves better relationships.
- Treat the current materialized graph as a hint only. Do not simply copy it when source chunks support new nodes.
- Preserve all source and evidence records exactly as provided. Do not invent sourceIds or evidenceIds.
- Every non-source node must have sourceIds or evidenceIds.
- Every edge, claim, memory, and wiki page must cite existing sourceIds/evidenceIds.
- Include one source node for every source record, using nodeId "source:<sourceId>".
- If the imported source has meaningful domain text, it must not be represented by only its source node.
- For the imported source, create durable non-source concept/topic nodes and source_of/related_to edges grounded to its provided evidence.
- Use stable nodeIds/relationIds/claimIds/memoryIds so repeated rebuilds remain readable.
- Return JSON only. No markdown fence, no prose.

Output shape:
{{
  "materializedGraph": {{
    "generatedAt": <unix seconds or null>,
    "sources": [...copy exactly from Provided sources],
    "evidence": [...copy exactly from Provided evidence],
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

Provided sources:
{sources_json}

Provided evidence:
{evidence_json}

Current materialized graph before rebuild:
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
        markdown = truncate_for_prompt(markdown, 24000)
    ))
}

fn truncate_for_prompt(value: &str, max_chars: usize) -> String {
    let mut output = value.chars().take(max_chars).collect::<String>();
    if value.chars().count() > max_chars {
        output.push_str("\n...[truncated]");
    }
    output
}
