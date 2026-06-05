# Graph Event History

HyprDuck graph maintenance treats `events/brain_events.jsonl` as the source of
truth. Files such as `graph/nodes.json`, `graph/edges.json`,
`graph/claims.json`, `memory/records.json`, `wiki/index.md`, and topic pages are
materialized read models that can be rebuilt from the event history.

## Event Row

Each JSONL row must follow `schemas/graph-mutation-event.schema.json`.

Required replay fields:

- `eventId`: unique append-only event identifier.
- `schemaVersion`: event contract version.
- `workspaceId`: workspace partition for replay.
- `eventType`: high-level event kind.
- `operationType`: concrete graph/wiki mutation or audit operation.
- `actor`: system, user, or agent that produced the event.
- `sourceRefs` and `sourceMarkdownRefs`: source evidence touched by the event.
- `nodeRefs`, `relationRefs`, `claimRefs`, and `memoryRefs`: refs mentioned by the event.
- `targetNodeIds`, `targetEdgeIds`, `targetClaimIds`, and `targetMemoryIds`: durable materialized records changed by the event.
- `evidenceRefs`: source evidence supporting the mutation.
- `payloadJson`: event-specific payload, including materialized graph, diff, or rollback data.
- `causality`: source/snapshot lineage for audit and replay.
- `policyResult`: automatic policy outcome.
- `createdAt`: event creation timestamp.

## Replay Ordering

Replay is deterministic and append-order based:

1. Read `events/brain_events.jsonl` line by line.
2. Filter rows to the requested `workspaceId`.
3. Apply remaining rows in physical JSONL append order.
4. Do not sort by `createdAt`, `eventId`, or `causality.materializedVersion`.
5. `upToEventId` stops immediately after applying the matching row.
6. `upToTimestamp` and `upToMaterializedVersion` are cutoff selectors checked while scanning append order.

Only replayable mutation events change materialized state:

- `graph_materialized` with a `materializedGraph` payload merges graph records
  into the materialized graph/wiki snapshot.
- `memory_accepted` upserts a memory record.
- `graph_materialized` with `operationType: graph_rollback` restores a prior materialized state by appending a new event.

Source and maintenance events remain audit context unless
their payload is explicitly handled by the replay engine.

## Graph Record Lifecycle

Materialized graph nodes and relations are append-aware records. Each graph
record carries:

- `validFrom`: the materialized version that first made this record live.
- `validTo`: absent for the live projection, or set when a later materialized
  event invalidates the record.
- `supersededBy`: the event ID that invalidated the record when `validTo` is
  present.

Replay treats missing or changed graph records as superseded instead of deleting
them from the durable history. A later event that changes the content for the
same node or relation ID invalidates the prior live record and inserts the new
live version. A full workspace rebuild invalidates omitted live graph records.
Source-scoped graph builds invalidate omitted live records in the affected
source scope. Workspace-linking artifacts that depend on deleted source evidence
are dropped rather than retained as graph history, because they are derived
cross-source projections and must not keep stale source joins alive.

Agent and desktop read surfaces use the live projection: graph records with
`validTo` set are excluded from graph canvas reads, `read_node` relations,
graph search results, and GraphQLite-backed graph-neighborhood retrieval. The
durable event log and materialized history remain available for audit and replay.

## Rollback And Snapshots

Rollback never rewrites historical events. The engine reconstructs the selected
state, snapshots the pre-rollback materialized files under `snapshots/`, writes
the selected materialized graph/wiki files, and appends a new
`graph_materialized` event with `operationType: graph_rollback`.

This makes rollback/replay behave like `git reset` over materialized files while
keeping the append-only event log auditable.
