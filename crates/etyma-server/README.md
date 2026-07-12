# etyma-server

SaaS host for workspace-scoped Etyma context packs. The server exposes
operator bootstrap routes, source listing, REST pack composition, and Cloud MCP
`get_context_pack`.

## Storage

| Plane | Backend | Contents |
| --- | --- | --- |
| Control | Postgres `control.*` | Orgs, workspaces, API tokens, identity/audit stubs |
| Knowledge | Postgres `knowledge.*` | Source metadata, evidence index, import jobs |
| Blob | Blob adapter | Original source bytes |
| Graph | Postgres `graph.*` | Added by PON-19 |

`etyma-server` has no SQLite fallback. `ETYMA_DATABASE_URL` is mandatory and
startup fails if Postgres cannot connect or migrate. Original bytes are never
stored in Postgres; source rows retain `blob_key`, content hash, size, and type.

The engine's legacy `knowledge.sqlite3` and GraphQLite path is outside PON-18.
It will be removed after the PON-19 Postgres graph replacement is available.

## Environment

| Variable | Default | Purpose |
| --- | --- | --- |
| `ETYMA_DATABASE_URL` | required | Postgres DSN for control and knowledge planes |
| `ETYMA_SERVER_DATA` | `./.etyma-server-data` | Parent for the default blob root only |
| `ETYMA_BLOB_ROOT` | `$ETYMA_SERVER_DATA/blobs` | Local filesystem blob adapter root |
| `ETYMA_SERVER_BIND` | `127.0.0.1:8787` | Listen address |
| `ETYMA_SPIKE_ADMIN_TOKEN` | unset | Required for operator bootstrap routes |

Blob keys use `w/{workspace_id}/sha256/{hex}`. Blob put and Postgres metadata
insert are separate operations, so a failed metadata insert can leave an orphan
blob for later garbage collection.

## Run

```bash
docker compose up -d
export ETYMA_DATABASE_URL=postgres://etyma:etyma@127.0.0.1:5432/etyma
export ETYMA_SPIKE_ADMIN_TOKEN=dev-admin
cargo run -p etyma-server
curl -sS http://127.0.0.1:8787/health
```

Healthy SaaS instances report `postgres: "up"` and `mode: "saas"`.

## Operator and pack flow

```bash
curl -sS -X POST http://127.0.0.1:8787/v1/spike/orgs \
  -H 'content-type: application/json' \
  -H 'x-etyma-admin-token: dev-admin' \
  -d '{"name":"Demo Org"}'

curl -sS -X POST http://127.0.0.1:8787/v1/spike/orgs/ORG_ID/workspaces \
  -H 'content-type: application/json' \
  -H 'x-etyma-admin-token: dev-admin' -d '{}'
curl -sS -X POST http://127.0.0.1:8787/v1/spike/workspaces/WS_ID/tokens \
  -H 'content-type: application/json' \
  -H 'x-etyma-admin-token: dev-admin' -d '{"label":"dev"}'
curl -sS -X POST http://127.0.0.1:8787/v1/spike/workspaces/WS_ID/seed \
  -H 'x-etyma-admin-token: dev-admin'

curl -sS -X POST http://127.0.0.1:8787/v1/packs \
  -H 'authorization: Bearer TOKEN' \
  -H 'content-type: application/json' \
  -d '{"query":"alpha-token"}'
```

Operator route names retain `/spike/` for wire compatibility; their storage is
Postgres-only. Same-org sibling workspaces remain isolated.

## Tests

```bash
export ETYMA_DATABASE_URL=postgres://etyma:etyma@127.0.0.1:5432/etyma
cargo test -p etyma-server -- --include-ignored
```

The Postgres suite covers migrations, workspace isolation, source/evidence
upserts, concurrent import-job leases, REST packs, MCP packs, and health.
