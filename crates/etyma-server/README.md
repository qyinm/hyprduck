# etyma-server (Phase 0+ spike)

Minimal multi-tenant Etyma cloud host: **Org → Workspace** hierarchy, workspace API tokens, multi-source seed (document + synthetic issue), REST pack compose, and Cloud MCP `get_context_pack`.

## Product model (locked)

| Layer | Meaning |
| --- | --- |
| **Org** | Organization, or a solo user's personal org (personal org auto-provision is later). Members/billing later. |
| **Workspace** | Project unit under an org. Tokens, sources, evidence, and packs are scoped **here only**. |
| **Metadata store** | Tenant rows, tokens, source/evidence indexes (spike: one SQLite file for the process). |
| **Blob store** | Original source bytes and large artifacts (target). Not a per-workspace local folder. |

- Same org **does not** grant cross-workspace access; sibling workspaces stay isolated.
- There is **no “local desktop workspace”** product concept. Desktop is a cloud client only.
- Spike shortcut: source bodies still sit in SQLite. **Next storage step is blob**.
- Local SQLite schema is spike-only: wipe `.etyma-server-data` if you hit migration errors after upgrades.

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
# 1) Create org
curl -sS -X POST http://127.0.0.1:8787/v1/spike/orgs \
  -H "content-type: application/json" \
  -H "x-etyma-admin-token: dev-admin" \
  -d '{"name":"Demo Org"}'
# → { "orgId": "org_…", "name": "Demo Org" }

# 2) Create workspace under org
curl -sS -X POST http://127.0.0.1:8787/v1/spike/orgs/ORG_ID/workspaces \
  -H "content-type: application/json" \
  -H "x-etyma-admin-token: dev-admin" \
  -d '{}'
# → { "workspaceId": "ws_…", "orgId": "org_…" }

# 3) Mint workspace token
curl -sS -X POST http://127.0.0.1:8787/v1/spike/workspaces/WS_ID/tokens \
  -H "content-type: application/json" \
  -H "x-etyma-admin-token: dev-admin" \
  -d '{"label":"dev"}'

# 4) Seed document + issue sources
curl -sS -X POST http://127.0.0.1:8787/v1/spike/workspaces/WS_ID/seed \
  -H "x-etyma-admin-token: dev-admin"

# 5) REST pack — Context Pack V1 only
curl -sS -X POST http://127.0.0.1:8787/v1/packs \
  -H "authorization: Bearer TOKEN" \
  -H "content-type: application/json" \
  -d '{"query":"alpha-token"}'

# 6) MCP pack
curl -sS -X POST http://127.0.0.1:8787/v1/mcp \
  -H "authorization: Bearer TOKEN" \
  -H "content-type: application/json" \
  -d '{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"get_context_pack","arguments":{"query":"alpha-token"}}}'
```

List helpers: `GET /v1/spike/orgs`, `GET /v1/spike/orgs/{orgId}/workspaces`.

Flat `POST /v1/spike/workspaces` (no org) is **removed**.

## Tests

```bash
cargo test -p etyma-server
```

## Out of scope (intentionally)

OIDC, org members/invites/roles, org-scoped tokens, personal-org auto-provision, real GitHub OAuth, full MCP catalog, desktop cloud client default, object-storage blob backend.
