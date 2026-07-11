# etyma-server (Phase 0+ spike)

Minimal multi-tenant Etyma cloud host: **Org → Workspace** hierarchy, workspace API tokens, multi-source seed (document + synthetic issue), **blob-backed source bytes**, REST pack compose, and Cloud MCP `get_context_pack`.

## Product model (locked)

| Layer | Meaning |
| --- | --- |
| **Org** | Organization, or a solo user's personal org (personal org auto-provision is later). Members/billing later. |
| **Workspace** | Project unit under an org. Tokens, sources, evidence, and packs are scoped **here only**. |
| **Control store** | Orgs, workspaces, API tokens (and future users/memberships/audit stubs). **Cloud:** Postgres `control.*`. **Spike:** SQLite. |
| **Knowledge meta** | Source/evidence indexes. **Still process-local SQLite** until S-PG3 (hybrid when DSN is set). |
| **Blob store** | Original source bytes (local FS adapter for dev/CI; S3/presign later). |

**Postgres is the cloud control plane.** When `ETYMA_DATABASE_URL` is set, boot connects a pool, applies migrations, and opens a **hybrid** store: control on Postgres, sources/evidence on local SQLite. No control-plane reads/writes hit `server.sqlite3` in that mode.

SQLite-only boot remains for spike/dev when no DSN is set (default allow unless `ETYMA_ALLOW_SQLITE=0`). The frozen multi-plane target is Postgres for control, knowledge, and graph projection, plus blob for original bytes — see [`docs/storage-planes.md`](../../docs/storage-planes.md).

- Same org **does not** grant cross-workspace access; sibling workspaces stay isolated.
- There is **no “local desktop workspace”** product concept. Desktop is a cloud client only.
- **Wipe on schema change:** local SQLite is spike/knowledge-meta only. Delete the data dir (DB **and** blob root) if you hit migration errors after upgrades. **There is no one-shot migrate of control rows from spike SQLite → Postgres** — wipe/re-seed is OK for the spike.

## Requirements

- Rust workspace toolchain
- **Postgres (cloud / hybrid):** local Postgres via `docker compose up -d` at repo root (or any Postgres accepting the DSN)
- **Spike path:** no Postgres required — full SQLite product meta + local FS blobs

## Environment

| Variable | Default | Purpose |
| --- | --- | --- |
| `ETYMA_DATABASE_URL` | _(unset)_ | Postgres DSN. When set, boot connects a pool, runs migrations, and uses hybrid Store (control=PG). |
| `ETYMA_CLOUD_MODE` | off (`0` / unset) | When `1`/`true`/`yes`, fail-fast at config parse if `ETYMA_DATABASE_URL` is missing. |
| `ETYMA_ALLOW_SQLITE` | `1` (allow) | Spike/dev only when not cloud mode. Set `0`/`false`/`no` to refuse boot without a Postgres DSN. |
| `ETYMA_SERVER_DATA` | `./.etyma-server-data` | Parent dir for default SQLite path + blob root |
| `ETYMA_SERVER_DB` | `$ETYMA_SERVER_DATA/server.sqlite3` | Spike: full meta SQLite. Hybrid: knowledge-only SQLite (sources/evidence; no control tables written). |
| `ETYMA_BLOB_ROOT` | `$ETYMA_SERVER_DATA/blobs` | Local filesystem blob root (dev/CI) |
| `ETYMA_SERVER_BIND` | `127.0.0.1:8787` | Listen address |
| `ETYMA_SPIKE_ADMIN_TOKEN` | _(unset)_ | Required for spike operator routes |

### Wipe + blob root

```bash
# Full local reset (SQLite metadata + blobs). Control rows on Postgres are separate.
rm -rf ./.etyma-server-data

# Or only blobs (re-seed after)
rm -rf "${ETYMA_BLOB_ROOT:-./.etyma-server-data/blobs}"
```

After a wipe, recreate org → workspace → token → seed.

Blob object keys use: `w/{workspace_id}/sha256/{hex}` with content hash `sha256:{hex}` of the raw bytes. Keys are content-addressed (hash embedded in the key); caller-supplied hashes are checked on write, and content-addressed reads can re-verify on get.

Ingest is **blob put then knowledge meta** and is not a single transaction. If meta insert fails after put, an orphan blob may remain until you wipe the data dir.

## Run

### Boot with Postgres (cloud control plane)

```bash
docker compose up -d
export ETYMA_DATABASE_URL=postgres://etyma:etyma@127.0.0.1:5432/etyma
export ETYMA_CLOUD_MODE=1   # optional for cloud-mode fail-fast
export ETYMA_SPIKE_ADMIN_TOKEN=dev-admin
cargo run -p etyma-server
curl -sS http://127.0.0.1:8787/health
```

With a DSN, health reports `postgres: "up"` and `mode: "cloud-foundation"` after migrate. Control (orgs/workspaces/tokens) lives in Postgres; sources/evidence remain on the local knowledge SQLite path until S-PG3.

### Spike SQLite path (transitional, no DSN)

Still works without Postgres for local spike/dev:

```bash
export ETYMA_SERVER_DATA=./.etyma-server-data
export ETYMA_BLOB_ROOT=./.etyma-server-data/blobs   # optional; default under data dir
export ETYMA_SPIKE_ADMIN_TOKEN=dev-admin
cargo run -p etyma-server
```

Health reports `postgres: "skipped"` and `mode: "spike"`. Do not use this path as the cloud primary.

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
# Default suite (Postgres hybrid/isolation tests are #[ignore]d)
cargo test -p etyma-server

# Full suite including Postgres control plane
docker compose up -d
export ETYMA_DATABASE_URL=postgres://etyma:etyma@127.0.0.1:5432/etyma
cargo test -p etyma-server -- --include-ignored
```

**CI:** [`.github/workflows/etyma-server-pg.yml`](../../.github/workflows/etyma-server-pg.yml) runs `cargo test -p etyma-server -- --include-ignored` against Postgres 16 with `ETYMA_DATABASE_URL` set.

## Out of scope (intentionally)

OIDC, org members/invites/roles, org-scoped tokens, personal-org auto-provision, real GitHub OAuth, full MCP catalog, desktop cloud client default, S3/presign upload, human upload UI, knowledge meta migration to Postgres (S-PG3), graph projection (S-PG4), cutover drop of residual SQLite (S-PG5).
