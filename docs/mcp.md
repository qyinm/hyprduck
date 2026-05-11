# MCP Brain Server

HyprDuck exposes the local brain through an MCP stdio server:

```bash
duckdocs mcp serve
```

For local development, run the same server through Cargo:

```bash
cargo run -p duckdocs-cli -- mcp serve
```

## Client Configuration

Example MCP client entry:

```json
{
  "mcpServers": {
    "hyprduck": {
      "command": "duckdocs",
      "args": ["mcp", "serve"]
    }
  }
}
```

## Read Tools

All tools accept optional `workspaceId` and `rootDir` arguments. If omitted,
`workspaceId` defaults to `default` and the engine resolves the local HyprDuck
brain root.

| Tool | Required arguments | Purpose |
| --- | --- | --- |
| `search_brain` | `query` | Return ranked source-backed brain IDs. |
| `get_context_pack` | `query` | Build an agent-ready pack with memories, claims, entities, relations, sources, evidence, and recent events. |
| `read_source` | `sourceId` | Read a source record with adjacent wiki and evidence. |
| `read_wiki_page` | `path` | Read a generated or save-back wiki page. |
| `read_node` | `nodeId` | Read a graph node with evidence and adjacent relations. |
| `read_recent_events` | none | Read append-only brain event history. |
| `read_health` | none | Read health and pending-review summary. |

## Proposed-Write Tools

Proposed-write tools do not overwrite original source artifacts. They call the
engine proposal path, create brain events, and return the proposal status.

| Tool | Required arguments | Policy result |
| --- | --- | --- |
| `propose_memory` | `title`, `body` | Safe project memory is auto-applied and audited. |
| `propose_claim` | `title`, `body` | Claim stays pending review before becoming trusted graph state. |
| `propose_link` | `title`, `body`, `targetNodeId`, `nodeRefs`, `relationKind` | Link stays pending review before becoming trusted graph state. |
| `append_observation` | `title`, `body` | Observation is auto-applied as safe project memory and audited. |
| `add_source_note` | `title`, `body`, `sourceId` | Source metadata note is applied through the source-note policy path. |
| `request_consolidation` | `title`, `body` | Consolidation request is recorded as an agent observation for later maintenance. |

Optional proposal arguments:

- `actorId`
- `targetSourceId`
- `sourceRefs`
- `nodeRefs`
- `evidenceRefs`
- `sourceDescription`
- `sourceUserContext`
- `sourceIngestInstruction`

Risky writes remain reviewable. Agents cannot resolve reviews, accept pending
claims or links, or overwrite source truth through MCP.

## Example Calls

```json
{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"example","version":"0.1.0"}}}
{"jsonrpc":"2.0","method":"notifications/initialized"}
{"jsonrpc":"2.0","id":2,"method":"tools/list","params":{}}
{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"search_brain","arguments":{"workspaceId":"default","query":"agent context pack","limit":5}}}
{"jsonrpc":"2.0","id":4,"method":"tools/call","params":{"name":"propose_claim","arguments":{"workspaceId":"default","actorId":"claude-code","title":"Agent context packs should cite evidence","body":"HyprDuck context packs should include source and evidence IDs before agents use them as durable memory.","evidenceRefs":["evidence-source-1-page-1"]}}}
```

Tool results return JSON as text content so MCP clients can pass the complete
source, evidence, node, claim, relation, memory, proposal, and event IDs back to
agents.
