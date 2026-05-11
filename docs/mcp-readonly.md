# Read-Only MCP

HyprDuck exposes the local brain through a read-only MCP stdio server:

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

## Tools

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

No MCP tool in this server writes, proposes, resolves, or mutates brain
artifacts. Agent write paths stay behind the review/policy work planned for the
next P2 item.

## Example Calls

```json
{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"example","version":"0.1.0"}}}
{"jsonrpc":"2.0","method":"notifications/initialized"}
{"jsonrpc":"2.0","id":2,"method":"tools/list","params":{}}
{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"search_brain","arguments":{"workspaceId":"default","query":"agent context pack","limit":5}}}
```

Tool results return JSON as text content so MCP clients can pass the complete
source, evidence, node, claim, relation, memory, and event IDs back to agents.
