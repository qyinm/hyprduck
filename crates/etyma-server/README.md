# etyma-server

SaaS host for workspace-scoped Etyma context packs. The server exposes
operator bootstrap routes, source listing, REST pack composition, and Cloud MCP
`get_context_pack`.

## Storage

| Plane | Backend | Contents |
| --- | --- | --- |
| Control | Postgres `control.*` | Orgs, workspaces, users, OIDC identities, sessions, API tokens, audit stubs |
| Knowledge | Postgres `knowledge.*` | Source metadata, evidence index, import jobs |
| Graph | Postgres `graph.*` | Versioned nodes / relations / claims (live projection) |
| Blob | Blob adapter | Original source bytes |

`etyma-server` has no SQLite fallback. `ETYMA_DATABASE_URL` is mandatory and
startup fails if Postgres cannot connect or migrate. Original bytes are never
stored in Postgres; source rows retain `blob_key`, content hash, size, and type.

### Graph plane (S-PG4 / PON-19)

Cloud graph is a **Postgres relational projection**, not GraphQLite:

- Tables: `graph.nodes`, `graph.relations`, `graph.claims`
- Rows are versioned (`version_id`, `valid_from` / `valid_to`, `superseded_by`)
- **Live** projection = `valid_to IS NULL` (one live version per logical id per workspace)
- Write boundary: `GraphStore` upserts live versions (materialize target for S7)
- Read: `GET /v1/graph/snapshot` and pack `graph_trail` from live PG nodes
- **GraphQLite is local/engine only** — `etyma-server` never opens `knowledge.sqlite3`

Full engine materialize that *writes* the projection is **S7 (PON-11)**. Cutover that
drops residual cloud SQLite/GraphQLite paths is **S-PG5 (PON-20)**.

## Environment

| Variable | Default | Purpose |
| --- | --- | --- |
| `ETYMA_DATABASE_URL` | required | Postgres DSN for control, knowledge, and graph planes |
| `ETYMA_SERVER_DATA` | `./.etyma-server-data` | Parent for the default blob root only |
| `ETYMA_BLOB_ROOT` | `$ETYMA_SERVER_DATA/blobs` | Local filesystem blob adapter root |
| `ETYMA_SERVER_BIND` | `127.0.0.1:8787` | Listen address |
| `ETYMA_SPIKE_ADMIN_TOKEN` | unset | Required for operator bootstrap routes |
| `ETYMA_OIDC_ISSUER_URL` | unset | OIDC issuer, for example `https://accounts.google.com` |
| `ETYMA_OIDC_CLIENT_ID` | unset | OIDC confidential-client id |
| `ETYMA_OIDC_CLIENT_SECRET` | unset | OIDC confidential-client secret; never log or commit it |
| `ETYMA_OIDC_REDIRECT_URL` | unset | Registered callback, normally `https://HOST/v1/auth/callback` |
| `ETYMA_AUTH_COOKIE_SECURE` | `true` | Set `false` only for local HTTP development |
| `ETYMA_AUTH_SESSION_TTL_SECONDS` | `604800` | Session lifetime, constrained to 5 minutes through 30 days |
| `ETYMA_AUTH_SUCCESS_REDIRECT` | `/` | Local path after successful login; external URLs are rejected |

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

## OIDC login flow

The four `ETYMA_OIDC_*` values are required together. If all four are unset,
the server remains usable for the spike admin flow and returns `503` from
`/v1/auth/login`. The server discovers the configured provider, uses the
Authorization Code + PKCE flow, validates the returned ID token, and stores
only the issuer/subject and profile fields. Login-state and session values are
stored as SHA-256 hashes.

Register the exact value of `ETYMA_OIDC_REDIRECT_URL` with the provider. For
local HTTP development, use `ETYMA_AUTH_COOKIE_SECURE=false`; deployed HTTPS
callbacks should keep the secure-cookie default.

```bash
export ETYMA_OIDC_ISSUER_URL=https://accounts.google.com
# OIDC_CLIENT_ID and OIDC_CLIENT_SECRET are injected by the deployment secret manager.
export ETYMA_OIDC_CLIENT_ID="$OIDC_CLIENT_ID"
export ETYMA_OIDC_CLIENT_SECRET="$OIDC_CLIENT_SECRET"
export ETYMA_OIDC_REDIRECT_URL=http://127.0.0.1:8787/v1/auth/callback
export ETYMA_AUTH_COOKIE_SECURE=false
```

| Method | Path | Result |
| --- | --- | --- |
| `GET` | `/v1/auth/login` | Redirects to the configured OIDC provider |
| `GET` | `/v1/auth/callback?code=...&state=...` | Validates OIDC, sets `etyma_session`, then redirects to the local success path |
| `GET` | `/v1/me` | Returns the authenticated human principal or `401` |
| `POST` | `/v1/auth/logout` | Revokes the current session and clears the cookie |

The session cookie is `HttpOnly`, `SameSite=Lax`, scoped to `/`, and secure
when `ETYMA_AUTH_COOKIE_SECURE=true`. Human sessions do not grant workspace
agent access: `/v1/packs`, `/v1/sources`, `/v1/graph/snapshot`, and `/v1/mcp`
continue to require workspace API bearer tokens. S4 human routes use the
session cookie and organization membership:

| Method | Path | Result |
| --- | --- | --- |
| `GET` | `/v1/orgs` | Lists the authenticated user's organizations and roles |
| `GET` | `/v1/orgs/{org_id}/members` | Lists members for an authorized organization |
| `GET` | `/v1/orgs/{org_id}/workspaces` | Lists workspaces in an authorized organization |
| `POST` | `/v1/workspaces/{workspace_id}/tokens` | Owner-only token mint; raw secret is returned once |
| `GET` | `/v1/workspaces/{workspace_id}/tokens` | Owner-only token metadata, without secrets or hashes |
| `DELETE` | `/v1/workspaces/{workspace_id}/tokens/{token_id}` | Owner-only idempotent token revocation |

OIDC signup provisions one personal organization and an `owner` membership in
the same Postgres transaction as the new identity. Members can discover their
organizations and workspaces but cannot manage tokens. Requests for a workspace
outside the session user's memberships return a scoped `404`; insufficient
organization roles return `403`. `/v1/spike/*` continues to use
`x-etyma-admin-token` for operator bootstrap compatibility.

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

# Live graph projection (empty until materialize/S7 or GraphStore writes)
curl -sS http://127.0.0.1:8787/v1/graph/snapshot \
  -H 'authorization: Bearer TOKEN'
```

Operator route names retain `/spike/` for wire compatibility; their storage is
Postgres-only. Same-org sibling workspaces remain isolated.

### Upload and import status

After workspace and token creation, operator uploads use the source route and
return `202 Accepted` with a queued job:

```bash
UPLOAD=$(curl -sS -X POST \
  "http://127.0.0.1:8787/v1/spike/workspaces/$WS_ID/sources" \
  -H "x-etyma-admin-token: $ETYMA_SPIKE_ADMIN_TOKEN" \
  -H "x-etyma-source-title: design.md" \
  -H "x-etyma-source-kind: document" \
  -H "content-type: text/markdown" \
  --data-binary $'# Design\n\nWorkspace-scoped evidence.')

JOB_ID=$(printf '%s' "$UPLOAD" | jq -r '.job.id')
curl -sS \
  "http://127.0.0.1:8787/v1/spike/workspaces/$WS_ID/import-jobs/$JOB_ID" \
  -H "x-etyma-admin-token: $ETYMA_SPIKE_ADMIN_TOKEN"
```

Job status is stored in Postgres, source bytes live in the blob adapter, and
the S2 soft path writes one page-1 evidence record for UTF-8 content. Full
binary parsing and GraphQLite materialization belong to PON-11 / PON-19.
Queued and lease-expired running upload jobs are re-dispatched by a bounded
server recovery loop after startup.

## Tests

```bash
export ETYMA_DATABASE_URL=postgres://etyma:etyma@127.0.0.1:5432/etyma
cargo test -p etyma-server -- --include-ignored
```

The Postgres suite covers migrations (including identity/session and graph
planes), workspace isolation, source/evidence upserts, graph live projection
supersede, concurrent import-job leases, REST packs, MCP packs, health, and the
deterministic OIDC login → session → `/v1/me` acceptance flow.
