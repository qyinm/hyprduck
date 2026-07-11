# Cloud Multi-Plane Storage Architecture

**Status:** Accepted / Frozen

This document freezes the cloud multi-plane storage model. Later Postgres
migration work implements this model; it does not re-open graph-store or
backend-shape debates for cloud.

Local desktop and engine storage remain separate. See
`docs/agents/sqlite-graphqlite-knowledge-store-review.md` for the local SQLite +
GraphQLite knowledge store direction.

## Context

Today cloud storage is split across several backends:

| Store | Role today |
| --- | --- |
| Server SQLite (`server.sqlite3`) | Control and server meta (orgs, workspaces, users, membership, tokens, audit-shaped records) |
| Engine SQLite + GraphQLite (`knowledge.sqlite3`) | Knowledge meta, evidence, import jobs, and graph state on the engine path |
| Blob backend | Original document bytes |

That layout works for early cloud, but it couples multi-tenant control data,
knowledge lifecycle, and graph state to file-backed SQLite/GraphQLite patterns.
Postgres migration must not turn into a debate about whether cloud graph should
stay on GraphQLite files.

Blob storage is already in place: original bytes live in the blob backend; the
database stores `blob_key` and content hash only, not raw file payloads.

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

## Migration order

Implement and cut over by plane. Sequence is fixed; later steps do not redefine
earlier plane ownership.

| Step | Plane | What it unlocks |
| --- | --- | --- |
| 1 | Control foundation | Postgres bootstrap, schemas/prefixes, migrations, connectivity from the cloud server |
| 2 | Control plane | Orgs, workspaces, users, membership, tokens, and audit live in Postgres; multi-tenant control no longer depends on `server.sqlite3` |
| 3 | Knowledge / jobs | Source meta, evidence index, import jobs, content hashes, and `blob_key` move to Postgres; import lifecycle and citation readiness no longer depend on engine SQLite as cloud primary |
| 4 | Graph projection | Nodes, edges, claims, and version fields materialize into Postgres relational tables; cloud graph reads/writes leave GraphQLite files |
| 5 | Cutover | Cloud primary path is Postgres planes + blob; retire cloud reliance on server/engine SQLite and GraphQLite as authoritative stores |

Blob remains in place through all steps. Migration does not re-store original
bytes in Postgres.

## Non-goals / out of scope

This freeze does **not**:

- Implement Postgres schemas, migrations, or runtime dual-write
- Change local desktop/engine to Postgres
- Replace GraphQLite on the local path
- Redesign product UX, MCP tool surface, or agent workflow contracts
- Introduce graph-as-product positioning or memory-OS claims
- Choose a specific Postgres ORM, hosting vendor, or connection pool
- Define full table-level DDL (that belongs to migration implementation)

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

- Local path may keep SQLite + GraphQLite as the authoritative knowledge/graph
  store.
- Offline and single-user file-backed behavior remains valid for desktop and
  engine.
- Local GraphQLite does not imply cloud GraphQLite.

### Product boundary

- Graph, wiki, claims, and event history remain retrieval and inspection
  infrastructure for private document evidence reuse—not a separate graph-first
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

This multi-plane model is frozen for cloud. Subsequent Postgres migration work
implements it in the order above.
