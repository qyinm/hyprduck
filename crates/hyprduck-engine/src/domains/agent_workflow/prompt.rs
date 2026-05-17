use std::collections::BTreeSet;
use std::path::Path;

use crate::*;

pub(crate) fn build_source_local_graph_prompt(
    workspace_id: &str,
    manifest: &SourceArtifactManifest,
    markdown: &str,
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
    let evidence = snapshot
        .evidence
        .iter()
        .filter(|evidence| evidence.source_id.as_deref() == Some(manifest.source_id.as_str()))
        .cloned()
        .collect::<Vec<_>>();
    let sources_json =
        serde_json::to_string_pretty(&source).context("failed to encode imported source")?;
    let evidence_json =
        serde_json::to_string_pretty(&evidence).context("failed to encode imported evidence")?;
    let context_refs = import_evidence_context_allowed_refs(context);

    Ok(format!(
        r#"You are HyprDuck's source-local graph construction agent.

Task:
- Build a graph only for the newly imported sourceId {source_id}.
- Do not rebuild, rewrite, or summarize the rest of the workspace.
- Return materialized graph records that are grounded only in the provided source and evidence.
- Include one source node using nodeId "source:{source_id}".
- If the imported source has meaningful domain text, create durable non-source concept/topic nodes.
- Every non-source node must cite sourceId {source_id} or evidence from that source.
- Every non-source node must be connected from "source:{source_id}" with a source_of edge, or otherwise be connected to a source-grounded node.
- Every edge, claim, memory, and wiki page must cite existing evidenceIds from the imported source.
- Preserve source and evidence records exactly as provided. Do not invent sourceIds or evidenceIds.
- Use stable nodeIds/relationIds/claimIds/memoryIds so repeated imports remain readable.
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

Imported markdown:
{markdown}
"#,
        workspace_id = workspace_id,
        source_id = manifest.source_id,
        source_path = manifest.source_path,
        markdown_path = manifest.markdown_path,
        context_refs = join_or_none(&context_refs),
        sources_json = sources_json,
        evidence_json = evidence_json,
        markdown = truncate_for_prompt(markdown, 24000)
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
    let chunks = read_workspace_source_chunks(workspace_root)?
        .into_iter()
        .filter(|chunk| valid_source_ids.contains(chunk.source_id.as_str()))
        .collect::<Vec<_>>();
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
- Actively look for grounded links between algorithms, data structures, complexity topics, graph concepts, tree concepts, sorting/searching topics, and prerequisite/follow-up ideas across sources.
- Return only records needed for cross-source linking. Return no new edges only when no grounded relationship exists after comparing the imported markdown with the workspace chunks.
- Every returned edge, claim, memory, and wiki page must cite existing sourceIds/evidenceIds.
- Every returned edge must cite evidence from both endpoint source sides: at least one evidenceId from sourceId {source_id} and at least one evidenceId from the other endpoint's sourceIds.
- Do not return sources, evidence, nodes, entities, or extractions. Endpoint nodes must already exist in the current graph.
- Do not invent sourceIds, evidenceIds, or nodeIds.
- Use stable ids so repeated linking runs remain readable.
- Return JSON only. No markdown fence, no prose.

Output shape:
{{
  "materializedGraph": {{
    "generatedAt": <unix seconds or null>,
    "edges": [cross-source BrainRelationRecord...],
    "claims": [cross-source ClaimRecord...],
    "memories": [cross-source MemoryRecord...],
    "wikiPages": [cross-source WikiPage...]
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

fn truncate_for_prompt(value: &str, max_chars: usize) -> String {
    let mut output = value.chars().take(max_chars).collect::<String>();
    if value.chars().count() > max_chars {
        output.push_str("\n...[truncated]");
    }
    output
}
