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
- `payloadJson`: event-specific payload, including proposal, materialized graph, diff, or rollback data.
- `causality`: proposal/source/snapshot lineage for audit and replay.
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

- `graph_materialized` with a `materializedGraph` payload replaces the materialized graph/wiki snapshot.
- `graph_materialized` with an accepted `proposal` payload applies the proposal to the current replay snapshot.
- `memory_accepted` upserts a memory record.
- `graph_materialized` with `operationType: graph_rollback` restores a prior materialized state by appending a new event.

Proposal, review, source, and maintenance events remain audit context unless
their payload is explicitly handled by the replay engine.

## Rollback And Snapshots

Rollback never rewrites historical events. The engine reconstructs the selected
state, snapshots the pre-rollback materialized files under `snapshots/`, writes
the selected materialized graph/wiki files, and appends a new
`graph_materialized` event with `operationType: graph_rollback`.

This makes rollback/replay behave like `git reset` over materialized files while
keeping the append-only event log reviewable.
