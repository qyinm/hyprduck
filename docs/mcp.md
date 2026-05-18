# MCP Context Server

HyprDuck exposes local document context artifacts through an MCP stdio server:

```bash
hyprduck mcp serve
```

For production macOS installs, use the CLI bundled inside the installed app to
install the shell command and register HyprDuck with Codex or Claude Code:

```bash
/Applications/HyprDuck.app/Contents/Resources/binaries/hyprduck-aarch64-apple-darwin mcp install codex
/Applications/HyprDuck.app/Contents/Resources/binaries/hyprduck-aarch64-apple-darwin mcp install claude-code
```

That writes a `hyprduck` MCP server entry pointing at the installed app bundle
and creates `~/.local/bin/hyprduck`, so clients and shells do not depend on a
source checkout or `cargo run`.

See [`docs/mcp-client-setup.md`](mcp-client-setup.md) for Codex, Claude Code,
and Cursor setup details.

For local development, run the same server through Cargo:

```bash
cargo run -p hyprduck-cli -- mcp serve
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

## Read Tools

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

| Tool | Required arguments | Purpose |
| --- | --- | --- |
| `get_context_pack` | `query` | Build an agent-ready document context pack with selected sources, evidence, findings, warnings, and retrieval trace. |
| `read_context_pack` | none | Read the latest persisted `context_pack.json`, or pass optional `packId` for a historical pack under `context_packs/`. |
| `search_documents` | `query` | Return ranked source-backed document context IDs. |
| `search_brain` | `query` | Compatibility alias for `search_documents`. |
| `read_source` | `sourceId` | Read a source record with adjacent wiki and evidence. |
| `read_page_evidence` | `sourceId` | Read source evidence refs, optionally narrowed by 1-based `page`. |
| `read_wiki_page` | `path` | Read a generated or save-back wiki page. |
| `read_node` | `nodeId` | Read a graph node with evidence and adjacent relations. |
| `read_recent_events` | none | Read append-only document context event history. |
| `read_graph_history` | none | List prior materialized graph/wiki states for audit and debugging. |
| `read_graph_snapshot` | none | Read the latest completed materialized graph/wiki snapshot. |
| `read_health` | none | Read workspace context readiness, including per-source status, failed-page counts, content-hash state, and provider route. |

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

## Example Calls

```json
{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"example","version":"0.1.0"}}}
{"jsonrpc":"2.0","method":"notifications/initialized"}
{"jsonrpc":"2.0","id":2,"method":"tools/list","params":{}}
{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"get_context_pack","arguments":{"workspaceId":"default","query":"source-backed claims about agent context packs","budget":4000}}}
{"jsonrpc":"2.0","id":4,"method":"tools/call","params":{"name":"search_documents","arguments":{"workspaceId":"default","query":"agent context pack","limit":5}}}
{"jsonrpc":"2.0","id":5,"method":"tools/call","params":{"name":"read_page_evidence","arguments":{"workspaceId":"default","sourceId":"source-example","page":1}}}
```

Tool results return JSON as text content so MCP clients can pass the complete
source, evidence, node, claim, relation, memory, and event IDs back to agents.

## Security Notes

- MCP is read-only by default; the default `tools/list` exposes no proposal,
  review, write, rollback, or mutation tools.
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
