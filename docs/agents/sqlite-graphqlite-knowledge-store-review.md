# SQLite GraphQLite Knowledge Store Review

This document records agent-facing implementation gates for the local-first
SQLite plus GraphQLite knowledge store migration.

## Planner Sequencing

Status: accepted.

The implementation sequence stays coherent when each layer proves one durable
contract before the next layer consumes it:

1. Versioned SQLite state is the canonical store for projects, sources, pages,
   typed evidence, wiki content, approvals, proposals, checkpoints, and events.
2. GraphQLite is loaded as the required primary graph store during workspace
   initialization.
3. Relational DB writes, GraphQLite mutations, and audit events commit under one
   transaction boundary.
4. Retrieval reads from FTS5, GraphQLite graph neighborhoods, and typed evidence
   only after integrity filters remove stale, unapproved, hash-mismatched, or
   failed rows.
5. MCP and desktop graph reads use DB or GraphQLite projections, while JSON
   files remain export, migration, debug, or compatibility outputs.

Deferred work is explicit in health output rather than hidden in prose:
checkpoint rollback API, vector search, and graph algorithms stay disabled until
their prerequisite read paths are stable enough to verify.

## Architect Boundary Review

Status: accepted.

SQLite owns durable proof and GraphQLite owns graph-native shape.

SQLite responsibilities:

1. Source identity, source hashes, source pages, typed evidence, evidence refs,
   provider route, locality, parse warnings, and audit events.
2. Wiki content, wiki revisions, proposal state, approval state, correction
   state, checkpoint metadata, and context-pack assembly records.
3. FTS5 tables for lexical retrieval over evidence, source pages, and wiki text.

GraphQLite responsibilities:

1. Current graph nodes for sources, pages, concepts, entities, claims, and wiki
   pages.
2. Current semantic relationships such as mentions, supports, cites, contradicts,
   derived-from, and source-of.
3. Graph-neighborhood retrieval and future graph-native algorithms once current
   graph data has enough stability evidence.

The migration architecture rejects split ownership. A graph mutation that cannot
commit must prevent the related relational graph-ready state and audit event from
committing. Citation-ready evidence can exist before graph-ready state, but graph
state cannot be treated as citation proof without relational evidence rows.

## Data Integrity Review

Status: accepted.

The integrity gate is satisfied only when schema, transaction, and citation rules
remain visible in both implementation and tests.

Required invariants:

1. Schema versions are reported for both the relational DB and GraphQLite graph
   store.
2. Health output reports GraphQLite as transactional and blocks release when the
   required primary graph store is unavailable.
3. Agent write proposals cannot commit with unknown evidence refs.
4. Agent write commits revalidate evidence refs at commit time so stale evidence
   cannot slip through after proposal creation.
5. Context Pack and page-evidence reads resolve citations from evidence rows and
   preserve source, page, quoted text, hash, provider route, locality, parse
   confidence, and evidence type.

Representative verification:

1. `brain_health_is_clean_for_empty_workspace`
2. `agent_session_write_rejects_unknown_evidence_ref`
3. `agent_session_write_revalidates_evidence_on_commit`
4. `mcp_server_exposes_read_and_agent_session_write_brain_tools`
