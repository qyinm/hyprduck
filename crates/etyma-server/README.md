# etyma-server (Phase 0+ spike)

Minimal multi-tenant Etyma cloud host: **Org → Workspace** hierarchy, workspace API tokens, multi-source seed (document + synthetic issue), **blob-backed source bytes**, REST pack compose, and Cloud MCP `get_context_pack`.

## Product model (locked)

| Layer | Meaning |
| --- | --- |
| **Org** | Organization, or a solo user's personal org (personal org auto-provision is later). Members/billing later. |
| **Workspace** | Project unit under an org. Tokens, sources, evidence, and packs are scoped **here only**. |
| **Metadata store** | Tenant rows, tokens, source/evidence indexes. **Spike today:** one SQLite file for the process (transitional). Source rows hold `blob_key`, `content_hash`, size, and content type — **not** original bytes. |
| **Blob store** | Original source bytes (local FS adapter for dev/CI; S3/presign later). |

Spike SQLite is transitional. The frozen **cloud** multi-plane target is Postgres for control, knowledge, and graph projection, plus blob for original bytes — see [`docs/storage-planes.md`](../../docs/storage-planes.md). Postgres is **not** implemented in this spike; the run instructions below still use SQLite + local FS blobs.

- Same org **does not** grant cross-workspace access; sibling workspaces stay isolated.
- There is **no “local desktop workspace”** product concept. Desktop is a cloud client only.
- **Wipe on schema change:** local SQLite is spike-only. Delete the data dir (DB **and** blob root) if you hit migration errors after upgrades.

## Requirements

- Rust workspace toolchain
- No external Postgres or object storage required for the spike (SQLite metadata + local FS blobs)

## Environment

| Variable | Default | Purpose |
| --- | --- | --- |
| `ETYMA_SERVER_DATA` | `./.etyma-server-data` | Parent dir for default DB + blob root |
| `ETYMA_SERVER_DB` | `$ETYMA_SERVER_DATA/server.sqlite3` | Metadata SQLite path |
| `ETYMA_BLOB_ROOT` | `$ETYMA_SERVER_DATA/blobs` | Local filesystem blob root (dev/CI) |
| `ETYMA_SERVER_BIND` | `127.0.0.1:8787` | Listen address |
| `ETYMA_SPIKE_ADMIN_TOKEN` | _(unset)_ | Required for spike operator routes |

### Wipe + blob root

```bash
# Full local reset (metadata + blobs)
rm -rf ./.etyma-server-data

# Or only blobs (re-seed after)
rm -rf "${ETYMA_BLOB_ROOT:-./.etyma-server-data/blobs}"
```

After a wipe, recreate org → workspace → token → seed.

Blob object keys use: `w/{workspace_id}/sha256/{hex}` with content hash `sha256:{hex}` of the raw bytes. Keys are content-addressed (hash embedded in the key); caller-supplied hashes are checked on write, and content-addressed reads can re-verify on get.

Ingest is **blob put then SQLite meta** and is not a single transaction. If meta insert fails after put, an orphan blob may remain until you wipe the data dir.

## Run

```bash
export ETYMA_SERVER_DATA=./.etyma-server-data
export ETYMA_BLOB_ROOT=./.etyma-server-data/blobs   # optional; default under data dir
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

# 4) Seed document + issue sources (writes blob objects, then metadata)
curl -sS -X POST http://127.0.0.1:8787/v1/spike/workspaces/WS_ID/seed \
  -H "x-etyma-admin-token: dev-admin"
# → includes blobs[].blobKey / contentHash / byteSize

# 5) REST pack — Context Pack V1 only (multi-source)
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

List helpers: `GET /v1/spike/orgs`, `GET /v1/spike/orgs/{orgId}/workspaces`, `GET /v1/sources` (metadata + blob keys, no bodies).

Flat `POST /v1/spike/workspaces` (no org) is **removed**.

## Tests

```bash
cargo test -p etyma-server
```

## Out of scope (intentionally)

OIDC, org members/invites/roles, org-scoped tokens, personal-org auto-provision, real GitHub OAuth, full MCP catalog, desktop cloud client default, S3/presign upload, human upload UI.
