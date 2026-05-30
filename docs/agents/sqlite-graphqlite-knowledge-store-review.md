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
