---
title: Agent graph patch MCP contract
date: 2026-06-01
category: architecture-patterns
module: MCP graph mutation and materialization
problem_type: architecture_pattern
component: assistant
severity: high
applies_when:
  - "Agents need to create graph/wiki state from HyprDuck evidence without HyprDuck owning the model call."
  - "A mutating MCP tool writes graph, claims, or wiki records that downstream readers consume immediately."
  - "Graph mutation must stay narrow, auditable, and evidence-scoped across Codex, Claude Code, or local agents."
related_components:
  - tooling
  - database
  - documentation
tags:
  - mcp
  - graph-patch
  - evidence-scoping
  - graphqlite
  - agent-workflow
  - materialization
  - cache-invalidation
  - schema-contract
---

# Agent Graph Patch MCP Contract

## Context

Provider graph generation remains useful when HyprDuck should call OpenRouter or Ollama during import and graph retry. It is the wrong boundary when an external coding agent has already read the source evidence through MCP and should use its own model runtime to produce graph/wiki changes.

The resolved pattern is `graph_patch_apply`: a controlled mutating MCP tool that accepts an agent-generated `hyprduck.graph_patch.v1` payload, validates it against existing source/evidence records, materializes it through the canonical graph snapshot path, and records an auditable brain event. HyprDuck owns the storage contract; the calling agent owns the model call.

Session history from the earlier MCP graph-readiness recovery work reinforced the same boundary: citation-ready evidence is useful before graph/wiki materialization catches up, but graph inspection surfaces need explicit readiness and cache contracts (session history). The later SQLite/GraphQLite planning notes also established that GraphQLite is the primary current graph store, while JSON artifacts are export/debug/read-model outputs rather than the source of truth (session history).

## Guidance

Expose agent graph creation as a typed patch contract, not as a provider-specific generation request. The shared engine types define the wire payload and keep CLI, engine client, MCP schema, and tests aligned:

```rust
pub const GRAPH_PATCH_SCHEMA_VERSION: &str = "hyprduck.graph_patch.v1";

pub struct GraphPatch {
    pub schema_version: String,
    pub source_ids: Vec<SourceId>,
    pub evidence_refs: Vec<String>,
    pub nodes: Vec<GraphPatchNode>,
    pub relations: Vec<GraphPatchRelation>,
    pub claims: Vec<GraphPatchClaim>,
    pub wiki_pages: Vec<GraphPatchWikiPage>,
    pub agent_metadata: BTreeMap<String, Value>,
}
```

Route the MCP tool to an engine command instead of applying graph state in the CLI:

```rust
"graph_patch_apply" => {
    let graph_patch: GraphPatch = serde_json::from_value(graph_patch_value)
        .context("argument graphPatch does not match HyprDuck graph patch schema")?;
    client.apply_graph_patch(ApplyGraphPatchRequest {
        scope,
        graph_patch,
        agent_id,
    })?
}
```

Inside the engine, validation happens before any write:

- `schemaVersion` must match `hyprduck.graph_patch.v1`.
- The patch must cite at least one source and one evidence ref.
- The patch must include at least one node, relation, claim, or wiki page.
- Referenced sources must exist in the requested workspace.
- Every evidence ref must belong to the cited source scope.
- Nodes, relations, claims, and wiki pages must carry their own direct evidence refs; cited `source:<sourceId>` source nodes are the narrow structural exception.
- Relation endpoints and claim/wiki refs may target patch-created nodes, cited source nodes such as `source:<sourceId>`, or existing nodes already backed by the same source/evidence scope.
- Wiki page paths must be workspace-relative.

After validation, use the same materialized workspace contract as provider graph generation: persist the GraphQLite-backed snapshot, write graph/wiki read models, append `events/brain_events.jsonl`, and publish `state/latest-readable-snapshot.json`. The event should identify the operation as `agent_graph_patch_apply` and the actor as the calling agent.

The MCP tool must be annotated as mutating:

```rust
tool_definition(
    "graph_patch_apply",
    "Apply an agent-generated, evidence-backed graph patch...",
    schema,
    vec!["graphPatch"],
    false,
)
```

Return cache metadata for the tool so MCP clients can detect that graph/wiki readers should refresh after a successful patch. Treat `graph_patch_apply` as cache-sensitive in the same path as `read_health` cache reporting.

## Why This Matters

This boundary keeps HyprDuck from becoming a generic model router while still making the graph canvas and graph/wiki read model useful for agent-generated knowledge. Agents can use Codex, Claude Code, Pi, or another runtime to reason over retrieved evidence, then hand HyprDuck a small, typed patch.

The validation rules are the product safety boundary. Without per-record evidence validation, an agent could add unsupported nodes, claims, or wiki pages. Without scoped relation endpoint checks, a patch could accidentally join one source's concept graph to unrelated existing nodes. Without publishing the latest-readable marker through the canonical materialization path, desktop, MCP, and resource readers can drift after mutation.

Keeping the patch schema in `hyprduck-engine-types` also prevents MCP schema drift. The JSON schema exposed by `tools/list` needs tests that cover every engine contract field so an agent can construct valid patches without reverse-engineering Rust internals.

## When to Apply

- Use this pattern when an external agent already has evidence through MCP and should produce graph/wiki updates with its own model runtime.
- Keep provider generation for import-time or retry-time OpenRouter/Ollama graph materialization.
- Auto-apply only after source/evidence/scope validation passes; do not accept broad filesystem writes or arbitrary graph merge commands.
- Reuse the canonical materialized graph/wiki writer and latest-readable marker instead of writing duplicate graph artifacts.
- Add contract tests across engine types, MCP tool schema, engine materialization, and end-to-end MCP server behavior.

## Examples

The intended agent flow is:

1. `import_source` with `skipGraphGeneration: true`, or use an already citation-ready source.
2. `read_source`, `read_page_evidence`, `get_context_pack`, or `search_documents` to collect evidence refs.
3. Construct a `hyprduck.graph_patch.v1` payload.
4. Call `graph_patch_apply`.
5. Verify `status: applied`, `graphReady: true`, `read_graph_snapshot` includes the new records, and `read_node` returns the cited evidence.

A minimal patch shape:

```json
{
  "schemaVersion": "hyprduck.graph_patch.v1",
  "sourceIds": ["source-example"],
  "evidenceRefs": ["ev-source-example-p1-0001"],
  "nodes": [{
    "nodeId": "concept-agent-context",
    "kind": "concept",
    "label": "Agent context",
    "sourceIds": ["source-example"],
    "evidenceIds": ["ev-source-example-p1-0001"]
  }],
  "relations": [{
    "relationId": "rel-source-agent-context",
    "kind": "mentions",
    "sourceNodeId": "source:source-example",
    "targetNodeId": "concept-agent-context",
    "label": "mentions",
    "evidenceIds": ["ev-source-example-p1-0001"]
  }]
}
```

Useful verification targets:

```bash
cargo test -p hyprduck-engine -p hyprduck-engine-client -p hyprduck-cli -p hyprduck-engine-types
cargo test -p hyprduck-engine-types brain_api_requests_round_trip
cargo test -p hyprduck-cli graph_patch_mcp_schema_covers_engine_contract_fields
cargo test -p hyprduck-engine agent_graph_patch
cargo test -p hyprduck-cli --test mcp_server mcp_server_exposes_read_and_agent_session_write_brain_tools
git diff --check
```

## Related

- [MCP import graph readiness recovery](../integration-issues/mcp-import-graph-readiness-recovery.md) documents the adjacent `citationReady` versus `graphReady` recovery contract.
- [docs/mcp.md](../../mcp.md) documents `graph_patch_apply`, the example JSON-RPC call, and MCP security notes.
- [docs/agents/mcp-client-setup.md](../../agents/mcp-client-setup.md) defines the Codex proof path for agent-generated graph patches.
- [docs/ARCHITECTURE.md](../../ARCHITECTURE.md) describes the two supported graph materialization paths: provider generation and agent patching.
- GitHub issue search for `graph_patch MCP graph readiness GraphQLite` returned no matching issues at documentation time.
