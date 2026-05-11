# Workspace Correction Persistence

Workspace graph corrections are not allowed to mutate only the aggregate graph. The
workspace graph is a read model compiled from source-backed project snapshots, so
rename and merge actions must be recorded in a correction ledger and then replayed
against the source snapshots that contributed evidence to the aggregate node.

## Contract

### Correction command

```json
{
  "id": "corr_01",
  "workspace_id": "default",
  "aggregate_node_id": "concept-shared-context-layer",
  "kind": "rename",
  "target_node_id": null,
  "value": "Shared Context Layer",
  "evidence_ids": ["ev-shared-context-layer-1"],
  "source_node_ids": ["source-a:concept-shared-context-layer"],
  "created_at": 1778515200
}
```

### Required fields

- `workspace_id`: workspace whose aggregate graph received the correction.
- `aggregate_node_id`: selected aggregate graph node.
- `kind`: `rename`, `merge`, or `keep_separate`.
- `target_node_id`: merge target for `merge`, otherwise null.
- `value`: new canonical label for `rename`, otherwise null.
- `evidence_ids`: evidence refs that justify the source mapping.
- `source_node_ids`: source project node ids affected by the correction.
- `created_at`: unix timestamp for deterministic replay order.

## Write Flow

1. Resolve the selected aggregate node to contributing source nodes using
   `source_id`, `source_path`, `page_index`, `markdown_path`, and `image_path`
   from its evidence refs.
2. Append a correction command to the workspace correction ledger.
3. Apply the command to every matching source project snapshot:
   - `rename`: update the source concept canonical label and keep the old label
     as an alias.
   - `merge`: move aliases and evidence refs onto the surviving source concept.
   - `keep_separate`: store a split guard so later aggregation does not collapse
     the same aliases again.
4. Rebuild the workspace aggregate from the corrected source snapshots.
5. Keep the original source artifacts immutable; corrections only update the
   knowledge snapshot and correction ledger.

## Storage Shape

The first implementation should add a `workspace_corrections` table next to the
existing project/source tables:

```sql
CREATE TABLE workspace_corrections (
  id TEXT PRIMARY KEY,
  workspace_id TEXT NOT NULL,
  aggregate_node_id TEXT NOT NULL,
  kind TEXT NOT NULL,
  target_node_id TEXT,
  value TEXT,
  evidence_ids_json TEXT NOT NULL,
  source_node_ids_json TEXT NOT NULL,
  created_at INTEGER NOT NULL
);
```

Source project snapshots should store the post-correction node state. The ledger
exists so the aggregate can explain user edits and so future schema migrations can
replay corrections when concept extraction improves.

## Open Questions

- Whether correction ids should be UUIDv7 like runtime request ids or a shorter
  local-only id.
- Whether failed partial source writes should roll back the ledger append or mark
  the command as `needs_replay`.
- How much UI should expose the affected source list before applying a merge.
