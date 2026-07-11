# etyma-server (Phase 0 spike)

Minimal multi-tenant Etyma cloud host: workspace API tokens, multi-source seed (document + synthetic issue), REST pack compose, and Cloud MCP `get_context_pack`.

## Product model (locked)

| Layer | Meaning |
| --- | --- |
| **Org** | Organization, or a solo user's personal org. Members, billing, default policy. |
| **Workspace** | Project unit under an org (or personal org). Tokens, sources, evidence, and packs are scoped here. |
| **Metadata store** | Tenant rows, tokens, source/evidence indexes (spike: one SQLite file for the process). |
| **Blob store** | Original source bytes and large artifacts (target). Not a per-workspace local folder. |

- There is **no “local desktop workspace”** product concept for cloud Etyma. Desktop is a client of cloud org/workspace; do not design compatibility with legacy local `rootDir` project folders.
- Spike shortcut: source bodies still sit in SQLite. **Next storage step is blob** (S3-compatible), with metadata rows holding `blob_key` / content hash — not engine-style per-tenant directories.

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
# 1) Create workspace (tenant row)
curl -sS -X POST http://127.0.0.1:8787/v1/spike/workspaces \
  -H "content-type: application/json" \
  -H "x-etyma-admin-token: dev-admin" \
  -d '{}'

# 2) Mint token (use workspaceId from step 1)
curl -sS -X POST http://127.0.0.1:8787/v1/spike/workspaces/WS_ID/tokens \
  -H "content-type: application/json" \
  -H "x-etyma-admin-token: dev-admin" \
  -d '{"label":"dev"}'

# 3) Seed document + issue sources into the tenant DB
curl -sS -X POST http://127.0.0.1:8787/v1/spike/workspaces/WS_ID/seed \
  -H "x-etyma-admin-token: dev-admin"

# 4) REST pack — body is Context Pack V1 only
curl -sS -X POST http://127.0.0.1:8787/v1/packs \
  -H "authorization: Bearer TOKEN" \
  -H "content-type: application/json" \
  -d '{"query":"alpha-token"}'

# 5) MCP pack (agent path) — tool text is V1 JSON
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

OIDC, org CRUD/roles, real GitHub OAuth, full MCP catalog, desktop default switched to cloud, engine knowledge DB mount, local project-folder “workspaces”, object-storage blob backend (planned; not in Phase 0).
