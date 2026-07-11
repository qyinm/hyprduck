# etyma-server (Phase 0 spike)

Minimal multi-tenant Etyma cloud host: workspace API tokens, multi-source seed (document + synthetic issue), REST pack compose, and Cloud MCP `get_context_pack`.

## Requirements

- Rust workspace toolchain
- No external Postgres required for the spike (server uses SQLite metadata DB)

## Run

```bash
export ETYMA_SERVER_DATA=./.etyma-server-data
export ETYMA_SPIKE_ADMIN_TOKEN=dev-admin
cargo run -p etyma-server
```

Default bind: `127.0.0.1:8787` (`ETYMA_SERVER_BIND` to override).

## Spike operator flow

```bash
# 1) Create workspace
curl -sS -X POST http://127.0.0.1:8787/v1/spike/workspaces \
  -H "content-type: application/json" \
  -H "x-etyma-admin-token: dev-admin" \
  -d '{}' 

# 2) Mint token (use workspaceId from step 1)
curl -sS -X POST http://127.0.0.1:8787/v1/spike/workspaces/WS_ID/tokens \
  -H "content-type: application/json" \
  -H "x-etyma-admin-token: dev-admin" \
  -d '{"label":"dev"}'

# 3) Seed document + issue fixtures
curl -sS -X POST http://127.0.0.1:8787/v1/spike/workspaces/WS_ID/seed \
  -H "x-etyma-admin-token: dev-admin"

# 4) REST pack (desktop/curl path)
curl -sS -X POST http://127.0.0.1:8787/v1/packs \
  -H "authorization: Bearer TOKEN" \
  -H "content-type: application/json" \
  -d '{"query":"alpha-token"}'

# 5) MCP pack (agent path)
curl -sS -X POST http://127.0.0.1:8787/v1/mcp \
  -H "authorization: Bearer TOKEN" \
  -H "content-type: application/json" \
  -d '{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"get_context_pack","arguments":{"query":"alpha-token"}}}'
```

## Tests

```bash
cargo test -p etyma-server
```

## Out of scope (intentionally)

OIDC, org roles, real GitHub OAuth, full MCP catalog, desktop default switched to cloud.
