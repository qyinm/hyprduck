# Cloud Multi-Plane Storage Architecture

**Status:** Accepted / Frozen

This document freezes the cloud multi-plane storage model. The current
`etyma-server` runtime already uses this placement; future work must preserve
it and must not re-open graph-store or backend-shape debates for cloud.

The product-level Local Workspace and Cloud Workspace authority contract lives
in [`local-cloud-operating-model.md`](local-cloud-operating-model.md). This
document only owns cloud plane placement.

Local desktop and engine storage remain separate. See
`docs/agents/sqlite-graphqlite-knowledge-store-review.md` for the local SQLite +
GraphQLite knowledge store direction.

## Context

### Current cloud runtime (`etyma-server`)

| Store | Role today |
| --- | --- |
| Postgres `control.*` | Organizations, workspaces, users, memberships, sessions, API tokens, and audit metadata |
| Postgres `knowledge.*` | Source metadata, evidence index, import jobs, content hashes, and Blob references |
| Postgres `graph.*` | Versioned nodes, relations, and claims as the live graph projection |
| Blob backend | Captured Source Revision payloads (`blob_key` + content hash on source rows) |

The current server requires `ETYMA_DATABASE_URL` and has no SQLite fallback.
Earlier server SQLite spike behavior is historical and must not be used as the
current cloud storage description.

### Local / engine today (unchanged by this freeze)

| Store | Role today |
| --- | --- |
| Engine SQLite + GraphQLite (`knowledge.sqlite3`) | Local desktop/engine knowledge meta, evidence, FTS, and graph state |

Cloud graph must remain relational Postgres state. Postgres migration work must
not turn into a debate about whether **cloud** graph should use GraphQLite files.

Blob storage is already in place for cloud: captured Source Revision payloads
live in the Blob backend;
the database stores `blob_key` and content hash only, not raw file payloads.

## Decision

Cloud storage is organized into four planes. Control, knowledge, and graph are
Postgres. Blob remains the object store for bytes.

### Control plane → Postgres

Owns multi-tenant identity and access:

- Organization
- Workspace
- User
- Membership
- API tokens
- Audit records

### Knowledge plane → Postgres

Owns import and evidence meta (not original bytes):

- Source metadata
- Evidence index
- Import jobs
- Content hashes and `blob_key` references

### Graph plane (cloud) → Postgres relational projection

Cloud graph is a **relational projection** of nodes, edges, claims, and version
fields, written by materialize (and equivalent commit paths).

Cloud graph is **not** multi-tenant GraphQLite files.

### Blob plane → object storage (already in place)

- Original source bytes live in the blob backend.
- Postgres rows hold `blob_key` and content hash only.

### Database topology

One Postgres database is acceptable. Separate planes with schemas (`control`,
`knowledge`, `graph`) or stable table prefixes. Plane boundaries are logical;
they do not require separate physical databases unless operational needs force
that later.

### GraphQLite scope

GraphQLite is allowed for:

- Local desktop / engine
- Dev workflows
- A future offline SKU if productized

GraphQLite is **not** the cloud primary graph store.

## Explicit rejection: GraphQLite as cloud primary

**Rejected:** multi-tenant cloud graph on GraphQLite file-backed stores.

Reasons:

1. **File-backed model** — GraphQLite is oriented around workspace files, not a
   shared multi-tenant service database.
2. **Multi-tenant scaling** — one GraphQLite file (or file set) per tenant does
   not scale cleanly for ops, connection management, or shared cloud control.
3. **Concurrent writers** — cloud import, materialize, and agent mutation paths
   need concurrent, transactional writers that Postgres handles as a first-class
   multi-writer database.
4. **Backups and recovery** — relational projection in Postgres aligns with
   standard backup, PITR, and operational tooling; fleets of GraphQLite files do
   not.

Local engines may keep GraphQLite. Cloud materialize writes the Postgres
relational projection instead.

## Migration history and remaining work

The original migration sequence established the current plane placement. The
sequence is retained as an audit trail; it does not describe an unimplemented
server SQLite target.

| Step | Plane | What it unlocks |
| --- | --- | --- |
| 1 | Control foundation | Complete in current main: Postgres bootstrap, schemas, migrations, and server connectivity |
| 2 | Control plane | Complete in current main: organizations, workspaces, users, memberships, tokens, sessions, and audit boundaries |
| 3 | Knowledge / jobs | Complete in current main: Source metadata, evidence index, import jobs, content hashes, and Blob references in Postgres |
| 4 | Graph projection | Complete in current main: versioned nodes, relations, claims, and live graph reads in Postgres |
| 5 | Cloud cutover | Current server path: Postgres planes + Blob; no server SQLite or cloud GraphQLite primary path |

Blob remains in place through all steps. Migration does not re-store original
bytes in Postgres.

## Non-goals / out of scope

This freeze does **not**:

- Define new Postgres schemas, migrations, or runtime dual-write beyond the
  current plane contract
- Change local desktop/engine to Postgres
- Replace GraphQLite on the local path
- Redesign product UX, MCP tool surface, or agent workflow contracts
- Introduce graph-as-product positioning or memory-OS claims
- Choose a specific Postgres ORM, hosting vendor, or connection pool
- Define full table-level DDL (that belongs to migration implementation)
- Define every local-only artifact (wiki revisions, memory records, full event
  log schemas) as a cloud table; placement remains an implementation task unless
  listed under a plane

Implementation work follows this document; it does not renegotiate the planes.

## Consequences

### Cloud

- Primary durable stores: Postgres (control, knowledge, graph projection) + blob
  backend for bytes.
- Materialize and cloud commit paths write graph as relational projection.
- Multi-tenant ops, backups, and concurrent writers assume Postgres, not
  GraphQLite files.
- Agent-facing contracts (source packs, evidence index, context packs, citations)
  stay evidence-oriented; only the cloud persistence backend changes.

### Local desktop / engine

- Local path remains SQLite + GraphQLite as the authoritative knowledge/graph
  store.
- Offline and single-user file-backed behavior remains valid for desktop and
  engine.
- Local GraphQLite does not imply cloud GraphQLite.

### Product boundary

- Graph, wiki, claims, and event history remain retrieval and inspection
  infrastructure for private source evidence reuse—not a separate graph-first
  product promise.
- Storage plane choices must not reframe Etyma as a generic memory OS or
  graph-only product.

## Summary

| Plane | Cloud primary | Notes |
| --- | --- | --- |
| Control | Postgres | Org, workspace, user, membership, tokens, audit |
| Knowledge | Postgres | Source meta, evidence index, import jobs, hashes, `blob_key` |
| Graph | Postgres relational projection | Written by materialize; not GraphQLite files |
| Blob | Object storage | Already in place; DB holds key + hash only |
| GraphQLite | Local / dev / future offline only | Not cloud primary |

This multi-plane model is frozen for cloud. Product authority, workspace
movement, and any future synchronization policy are defined separately in
`docs/local-cloud-operating-model.md`.
