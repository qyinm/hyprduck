# MCP Context Server

HyprDuck exposes local document context artifacts through an MCP stdio server:

```bash
hyprduck mcp serve
```

For production macOS installs, use the CLI bundled inside the installed app to
register HyprDuck with Codex or Claude Code. Open HyprDuck once first; the app
installs or refreshes the `hyprduck` shell command at `~/.local/bin/hyprduck`.
If `command -v hyprduck` does not print a path, use the `~/.local/bin/hyprduck`
fallback shown below or add `~/.local/bin` to `PATH`.

If `~/.local/bin/hyprduck` already points at an unmanaged development binary,
HyprDuck leaves it unchanged instead of overwriting an unrelated command. In
that case, use the installed app-bundled CLI path directly for proof runs or
refresh the shell command from the packaged app before claiming an installed-app
shell-shim proof.

```bash
hyprduck mcp install codex
hyprduck mcp install claude-code
```

That writes a `hyprduck` MCP server entry pointing at the installed app bundle,
so clients do not depend on a source checkout or `cargo run`.

See [`docs/agents/mcp-client-setup.md`](agents/mcp-client-setup.md) for Codex,
Claude Code, and Cursor setup details.

For local development, run the same server through Cargo:

```bash
cargo run -p hyprduck-cli -- mcp serve
```

If your shell does not include `~/.local/bin` on `PATH`, use the shim path
directly:

```bash
~/.local/bin/hyprduck mcp install codex
~/.local/bin/hyprduck mcp install claude-code
```

## Client Configuration

Example MCP client entry:

```json
{
  "mcpServers": {
    "hyprduck": {
      "command": "hyprduck",
      "args": ["mcp", "serve"]
    }
  }
}
```

## Tools

All tools accept an optional `workspaceId` argument. If omitted, `workspaceId`
defaults to `default` and the engine resolves the local HyprDuck workspace root.

`rootDir` is a development-only escape hatch for tests and local fixtures. It is
rejected unless the MCP server process is started with
`HYPRDUCK_MCP_ALLOW_ROOT_DIR=1` and `HYPRDUCK_MCP_ALLOWED_ROOTS` contains the
canonical workspace root. `HYPRDUCK_MCP_ALLOWED_ROOTS` uses the OS path-list
format, so macOS and Linux development sessions separate multiple roots with
`:`.

Production clients should pass `workspaceId` and let HyprDuck resolve the
canonical workspace root.

Tool responses redact absolute local filesystem paths by default. Debug clients
can pass `includeLocalPaths: true` to tool calls when they explicitly need raw
paths.

`import_source` is disabled unless the MCP server process is started with
`HYPRDUCK_MCP_ALLOWED_IMPORT_ROOTS` set to one or more canonical import roots.
The server canonicalizes `sourcePath`, requires it to be a regular file under
one of those roots, and imports it into HyprDuck's managed workspace artifacts.
This import allowlist is separate from the development-only `rootDir` allowlist.

| Tool | Required arguments | Purpose |
| --- | --- | --- |
| `import_source` | `sourcePath` | Start importing an allowlisted local PDF, DOCX, DOC, Markdown, or image file and return an import `jobId`. |
| `import_status` | `jobId` or `sourceId` | Poll an import job, or recover status for a persisted source after MCP restart. Agents can use source evidence once `citationReady` is true; graph/wiki inspection follows `graphReady`. |
| `import_cancel` | `jobId` | Request best-effort cancellation for a queued or running import job. |
| `import_retry_graph` | `jobId` or `sourceId` | Retry graph/wiki materialization for a citation-ready source without reparsing it. |
| `get_context_pack` | `query` | Build an agent-ready Context Pack v1 with selected sources, typed evidence, findings, warnings, and retrieval trace. |
| `read_context_pack` | none | Read the latest persisted `context_pack.json`, or pass optional `packId` for a historical pack under `context_packs/`; v1 packs are projected to the v0 compatibility shape for this reader. |
| `search_documents` | `query` | Return ranked source-backed document context IDs. |
| `search_brain` | `query` | Compatibility alias for `search_documents`. |
| `read_source` | `sourceId` | Read a source record with adjacent wiki and evidence. |
| `read_page_evidence` | `sourceId` | Read source evidence refs, optionally narrowed by 1-based `page`. |
| `read_wiki_page` | `path` | Read a generated or save-back wiki page. |
| `read_node` | `nodeId` | Read a graph node with up to 32 evidence refs and 64 adjacent relations. |
| `read_recent_events` | none | Read append-only document context event history. |
| `read_graph_history` | none | List prior materialized graph/wiki states, or pass optional lifecycle selectors for one graph node, relation, or wiki page. |
| `read_graph_snapshot` | none | Read the latest completed materialized graph/wiki snapshot. |
| `read_health` | none | Read workspace context readiness, including per-source status, failed-page counts, content-hash state, and provider route. |
| `graph_patch_apply` | `graphPatch` | Auto-apply an agent-generated `hyprduck.graph_patch.v1` graph/wiki patch after validating source IDs, evidence refs, relation endpoints, claim refs, and wiki refs. |
| `write_propose` | `contentType`, `title`, `body`, `evidenceRefs` | Stage an agent-proposed knowledge item after validating every evidence ref against the current workspace snapshot. |
| `write_commit` | `proposalId` | Approve one pending proposal and persist it as a `MemoryAccepted` brain event. |
| `write_commit_all` | `proposalIds` | Approve multiple pending proposals by explicit proposal ID list. |
| `write_list` | none | List pending proposals without mutating state. |
| `write_reject` | `proposalId` | Reject one pending proposal and remove it without creating a brain event. |

## Read Resources

The server also exposes MCP resources for clients that prefer resource reads over
tool calls:

| Resource URI | MIME type | Purpose |
| --- | --- | --- |
| `hyprduck://brain/default/graph/snapshot` | `application/json` | Latest resolved materialized graph/wiki snapshot. |
| `hyprduck://brain/default/wiki/index.md` | `text/markdown` | Current materialized wiki index. |

Resource URIs may include `?rootDir=/path/to/workspace-root` only in the same
explicit development mode described above. The server canonicalizes both
`rootDir` and the configured allowed roots before accepting the override, which
prevents symlink and path-prefix escapes. By default, resource reads resolve
inside the application-supported HyprDuck workspace root and do not require full
local paths. Resource responses return public HyprDuck resource URIs without
`rootDir` query parameters.

## Typed Evidence Contract

New imports write `evidence_index.json` with
`schemaVersion: "hyprduck.evidence_index.v1"`. Source Pack stays on v0. The
Evidence Index v1 item contract adds `evidenceType` so agents can distinguish
text evidence from tables, image regions, OCR, captions, summaries, claims, and
relationships as those producers become available.

`get_context_pack` returns:

- `contextPack`: the primary Context Pack v1 payload.
- `contextPackV1`: the same explicit v1 payload for clients that do not want to
  rely on the primary alias.
- `contextPackV0`: a compatibility projection for older clients.

Context Pack v1 selected evidence includes `evidenceType`, and
`retrievalTrace.evidenceTypeTrace` reports how many evidence items of each type
were considered and selected. Legacy Evidence Index v0 artifacts are still read
and converted to `text` evidence. Unsupported Evidence Index schema versions are
reported as `evidence_index_schema_mismatch` warnings instead of being treated
as unreadable files.

## Graph Trail Read Context

When GraphQLite graph context is available, `get_context_pack` may add
`selectedEvidence[].graphTrail` to Context Pack v1. This is retrieval and
inspection context for agents, not a graph export and not final-answer control.
`contextPackV0` remains a source/page/evidence compatibility projection and does
not include graph trail fields.

`graphTrail.direct` lists graph records directly supported by the selected
evidence. `graphTrail.adjacent` lists bounded neighboring records that can help
inspection but should not be treated as cited answer support by themselves.
Agents should continue to cite `sourceId`, `page`, and `evidenceRef` for answer
claims.

Use `graphTrail.followUp` for targeted reads. Each handle includes:

- `tool`: one of the existing read tools, such as `read_node`, `read_source`,
  `read_page_evidence`, or `read_wiki_page`.
- `handleType`: the stable handle category.
- `arguments`: the exact safe arguments to pass to that tool.
- `reason`: why the follow-up is relevant.

Graph-specific handles live in `graphTrail.followUp`; `suggestedNextReads`
stays source/page-centric for older clients. If graph materialization is not
ready, citation-backed Context Pack and page evidence reads remain usable and
graph trail data may be absent.

## Materialized Snapshot Read Path

`events/brain_events.jsonl` is the source of truth for graph and wiki mutations.
The current UI graph, MCP `read_graph_snapshot`, MCP resource reads, and wiki
page reader consume the materialized workspace state after the engine publishes
`state/latest-readable-snapshot.json`.

The latest-readable marker is the loading contract for downstream readers. It
points at the completed graph materialization event and lists the workspace
relative files that make up the current read model:

- `brain-manifest.json`
- `graph/nodes.json`
- `graph/edges.json`
- `graph/claims.json`
- `memory/records.json`
- `events/brain_events.jsonl`
- `wiki/index.md`
- `wiki/*.md`

Readers use the marker when it resolves to a completed
`graph_materialized` event for the requested workspace. If the marker is absent,
stale, or points at another workspace, readers fall back to the latest completed
`graph_materialized` event, but they still read graph and wiki content from the
materialized files above. The wire contract is defined in
`schemas/graph-snapshot-read.schema.json`; responses include
`sourceOfTruthPath`, `latestReadableSnapshotPath`, and `materializedPaths` so UI,
MCP, and agent consumers can audit exactly which files were loaded.

Graph nodes and relations may retain lifecycle metadata for audit and replay.
MCP and desktop graph reads expose the live projection: records with `validTo`
set are excluded from current graph snapshots, node reads, search, and graph
trail retrieval. See `docs/graph-event-history.md` for the replay contract.

## Lifecycle History Read Path

`read_graph_history` has two modes:

- Without record selectors, it returns the compatible `states` list for prior
  completed graph/wiki materializations.
- With record selectors, it also returns `recordHistory` for one logical graph
  record or wiki page.

Selector arguments are optional and additive:

- `recordKind: "node"` with `recordId`: inspect versions for one logical node.
- `recordKind: "relation"` with `recordId`: inspect versions for one logical relation.
- `recordKind: "wiki_page"` with `recordId`: inspect revisions for one wiki page ID.
- `wikiPath`: inspect revisions for one wiki page path, such as `wiki/index.md`.
- `includeDiff: true`: include stored wiki revision diff JSON when available.

`recordHistory.versions[]` includes logical IDs, version IDs, creating event
IDs, lifecycle fields, evidence refs, source refs, graph refs, and redacted
storage labels. It is an inspection surface, not a rollback API: responses do
not include raw local paths, rollback targets, or replay selectors. Default
Context Pack, wiki, node, graph snapshot, and graph trail reads continue to use
only the live projection.

## Example Calls

```json
{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"example","version":"0.1.0"}}}
{"jsonrpc":"2.0","method":"notifications/initialized"}
{"jsonrpc":"2.0","id":2,"method":"tools/list","params":{}}
{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"import_source","arguments":{"workspaceId":"default","sourcePath":"/allowed/imports/source.md","format":"markdown"}}}
{"jsonrpc":"2.0","id":4,"method":"tools/call","params":{"name":"import_status","arguments":{"workspaceId":"default","jobId":"import-job-id-from-import_source"}}}
{"jsonrpc":"2.0","id":5,"method":"tools/call","params":{"name":"get_context_pack","arguments":{"workspaceId":"default","query":"source-backed claims about agent context packs","budget":4000}}}
{"jsonrpc":"2.0","id":6,"method":"tools/call","params":{"name":"search_documents","arguments":{"workspaceId":"default","query":"agent context pack","limit":5}}}
{"jsonrpc":"2.0","id":7,"method":"tools/call","params":{"name":"read_page_evidence","arguments":{"workspaceId":"default","sourceId":"source-example","page":1}}}
{"jsonrpc":"2.0","id":8,"method":"tools/call","params":{"name":"graph_patch_apply","arguments":{"workspaceId":"default","agentId":"codex","graphPatch":{"schemaVersion":"hyprduck.graph_patch.v1","sourceIds":["source-example"],"evidenceRefs":["ev-source-example-p1-0001"],"nodes":[{"nodeId":"concept-agent-context","kind":"concept","label":"Agent context","sourceIds":["source-example"],"evidenceIds":["ev-source-example-p1-0001"]}],"relations":[{"relationId":"rel-source-agent-context","kind":"mentions","sourceNodeId":"source:source-example","targetNodeId":"concept-agent-context","label":"mentions","evidenceIds":["ev-source-example-p1-0001"]}]}}}}
{"jsonrpc":"2.0","id":9,"method":"tools/call","params":{"name":"read_graph_history","arguments":{"workspaceId":"default","recordKind":"node","recordId":"concept-agent-context","limit":5}}}
{"jsonrpc":"2.0","id":10,"method":"tools/call","params":{"name":"read_graph_history","arguments":{"workspaceId":"default","wikiPath":"wiki/index.md","includeDiff":true,"limit":5}}}
{"jsonrpc":"2.0","id":11,"method":"tools/call","params":{"name":"write_propose","arguments":{"workspaceId":"default","contentType":"memory","title":"Context pack reuse note","body":"Agents can reuse approved HyprDuck knowledge through get_context_pack.","evidenceRefs":["ev-source-example-p1-0001"]}}}
{"jsonrpc":"2.0","id":12,"method":"tools/call","params":{"name":"write_commit","arguments":{"workspaceId":"default","proposalId":"proposal-id-from-write_propose"}}}
```

`import_source` returns before parsing and graph/wiki materialization finish.
Poll `import_status` until `citationReady` is true before asking for a
Context Pack or page evidence. `graphReady` is a separate inspection signal for
graph/wiki surfaces; a graph failure after citation readiness does not erase the
source evidence.

A typical agent-chat approval flow for controlled graph/wiki/memory mutation is:

1. Agent calls `write_propose` with evidence-backed content.
2. HyprDuck validates `evidenceRefs` before staging the proposal.
3. The chat UI asks the user to approve, reject, or approve a visible batch.
4. Approval maps to `write_commit` or `write_commit_all`; rejection maps to `write_reject`.
5. Approved memory writes append `MemoryAccepted` events and update `memory/records.json`, so later `search_brain` and `get_context_pack` calls can retrieve the saved knowledge.

Agent-generated graph creation is separate from the proposal flow. If an agent
has already read source/evidence through MCP and is responsible for creating the
graph with its own model runtime, it calls `graph_patch_apply` directly. Valid
patches are auto-applied and audited as `agent_graph_patch_apply` graph
materialization events. Provider graph generation remains available through the
normal import path and `import_retry_graph`, using the configured
OpenRouter/Ollama provider.

Tool results return JSON as text content so MCP clients can pass the complete
source, evidence, node, claim, relation, memory, and event IDs back to agents.

### Mutating Tool Policy Matrix

| Tool | Scope policy | Evidence policy | Audit behavior | Replay behavior |
| --- | --- | --- | --- | --- |
| `import_source` | Import-root allowlist | Source evidence is generated by import | Import job and source records | Duplicate source/path behavior is tracked by import job/source IDs |
| `import_cancel` | Workspace and live job scope | Not applicable | Import job cancellation state | Terminal jobs remain terminal |
| `import_retry_graph` | Workspace and source/job scope | Requires citation-ready source evidence | Import job graph retry state | Retry availability is controlled by persisted graph retry metadata |
| `graph_patch_apply` | Workspace, source, and evidence scope | Required before write and per changed record | `agent_graph_patch_apply` graph materialization event | Record IDs are upserted only within validated source/evidence scope |
| `write_propose` | Workspace scope | Evidence refs required before staging | Validated proposal record | New `proposalId` per accepted proposal payload |
| `write_commit` | Workspace and proposal scope | Proposal evidence revalidated before commit | `MemoryAccepted` brain event and committed proposal status | Already committed, rejected, or missing proposal IDs fail with `proposal_state` |
| `write_commit_all` | Workspace and explicit proposal list scope | Each proposal evidence is revalidated before commit | Per-proposal `MemoryAccepted` brain event on success | Per-item committed or failed result with `errorCategory` |
| `write_reject` | Workspace and proposal scope | Not applicable | Rejected proposal status without brain event | Already rejected or missing proposal IDs are stable `proposal_state` failures |

Agent-facing MCP tool failures include a stable `hyprduckErrorCategory` in tool
result metadata when possible. Current categories are `path_policy`,
`evidence_scope`, `schema`, `provider`, `graph_materialization`, `lifecycle`,
`persistence`, and `unknown`. `write_commit_all` also returns per-item
`errorCategory` values so agents can safely retry or surface partial failures.

## Security Notes

- MCP is a controlled read/write agent workflow surface, not a read-only API.
  Agents may propose evidence-backed changes to HyprDuck knowledge state,
  including memory proposals and direct evidence-backed graph patches. Mutating
  tools must be narrow, auditable, accurately annotated with
  `readOnlyHint: false`, and backed by source/evidence validation.
- Pending proposal inspection such as `write_list` is read-only even though it
  belongs to the write workflow. Approval happens through explicit
  `write_commit` / `write_commit_all` MCP calls, and rejected proposals are
  discarded without producing brain events.
- `import_source` is a mutating tool. It accepts only regular files below
  `HYPRDUCK_MCP_ALLOWED_IMPORT_ROOTS`, never arbitrary agent-provided paths,
  and returns an import job with managed source/evidence IDs available through
  `import_status` once citation readiness is reached.
- Workspace IDs must be single path segments. The engine rejects `..`, absolute
  path components, and symlink escapes after canonicalization.
- `rootDir` is disabled by default and, when explicitly enabled for development,
  must resolve under a canonical path in `HYPRDUCK_MCP_ALLOWED_ROOTS`.
- Existing materialized artifact reads are canonicalized under the workspace
  root before the engine reads them.
- MCP tool responses redact absolute local filesystem paths unless
  `includeLocalPaths: true` is explicitly provided. MCP resource responses omit
  `rootDir` query parameters from returned `contents[].uri` values.
- Context packs include provider-route fields, but they may currently be
  `unknown` when the source artifact does not expose an effective route. Source
  Pack and Evidence Index artifacts carry the import-time provider route.
- `read_health` reports per-source readiness without local paths: source status,
  failed-page count, content-hash state, provider route, local/hosted flag, and
  source warnings.
- Imported documents can contain prompt-injection text. Agents should treat
  document content as untrusted source material and rely on selected evidence,
  page refs, content hashes, and warnings rather than following instructions
  embedded in imported documents.
