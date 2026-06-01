# Agent MCP Client Setup

HyprDuck's launch agent path is Codex first. Claude Code uses the same local
MCP server but is an external validation target when the operator does not have
a Claude Code subscription. Cursor remains a later manual setup target.

## Codex Required Path

Register HyprDuck with Codex:

```bash
hyprduck mcp install codex
```

For a proof that uses MCP `import_source`, register the approved import root at
install time so Codex launches HyprDuck with the import allowlist:

```bash
HYPRDUCK_MCP_ALLOWED_IMPORT_ROOTS=/path/to/approved/imports \
  hyprduck mcp install codex
```

Development proofs that pass `rootDir` also need the development workspace root
allowlist:

```bash
HYPRDUCK_MCP_ALLOWED_IMPORT_ROOTS=/path/to/approved/imports \
HYPRDUCK_MCP_ALLOW_ROOT_DIR=1 \
HYPRDUCK_MCP_ALLOWED_ROOTS=/path/to/hyprduck/workspace \
  hyprduck mcp install codex
```

Rerun the install command after changing an import root; Codex stores the MCP
server environment in its MCP entry.

If the shell command is not on `PATH`, use the shim directly:

```bash
~/.local/bin/hyprduck mcp install codex
```

Verify registration:

```bash
command -v hyprduck
codex mcp list
```

The Codex entry should be named `hyprduck` and run:

```text
<HyprDuck binary> mcp serve
```

Then ask Codex to use HyprDuck:

```text
Use HyprDuck to answer from my local document context. Start with
get_context_pack, cite sourceId, page, and evidenceRef for every claim, then ask
a second question against the same source set.
```

The first successful proof must record:

- time from install command to first successful `get_context_pack`;
- `tools/list` includes `import_source`, `import_status`, `get_context_pack`,
  `search_documents`, `read_source`, `read_page_evidence`, and
  `graph_patch_apply`;
- if MCP import is part of the proof, `HYPRDUCK_MCP_ALLOWED_IMPORT_ROOTS` is set
  and `import_source` returns a `jobId` without leaking local paths; polling
  `import_status` reaches a citation-ready state with `citationReady: true`,
  returns a `sourceId`, and reports nonzero evidence count without leaking local
  paths;
- first answer includes at least one `sourceId`, page, and `evidenceRef`;
- `get_context_pack` returns `contextPack.schemaVersion` as
  `hyprduck.context_pack.v1`, includes `selectedEvidence[].evidenceType`, and
  includes `retrievalTrace.evidenceTypeTrace`;
- `contextPackV0` remains present for compatibility with older agent clients;
- second query reuses the same source set or performs a follow-up evidence read;
- failure class, if any.

For agent-generated graph proof, call `import_source` with
`skipGraphGeneration: true`, read the imported source/evidence, have the calling
agent construct a `hyprduck.graph_patch.v1` payload, then call
`graph_patch_apply`. A passing proof records that `graph_patch_apply` returns
`status: applied`, `graphReady: true`, `read_graph_snapshot` includes the new
node/relation, and `read_node` returns the cited evidence. This path keeps the
existing provider graph path intact: `import_retry_graph` still retries
OpenRouter/Ollama-backed graph/wiki materialization when provider graph
generation is desired.

Polling `import_status` should move through:

```text
imported -> parsing -> packaging -> citation_ready -> context_ready
```

`citation_ready` is the key HyprDuck import milestone: the source has evidence
refs an agent can use with citations. `context_ready` is later context/graph
readiness and should not block citation-backed source inspection.

Graph generation has separate terminal states:

- `context_ready`: citation evidence is usable and the GraphQLite-backed graph
  materialization committed.
- `citation_ready_graph_pending`: citation evidence is usable, but graph/wiki
  inspection is incomplete. Continue with `get_context_pack`,
  `search_documents`, `read_source`, or `read_page_evidence`; inspect
  `retryable`, `retryAttempt`, `maxRetryAttempts`, `nextRetryAt`, and
  `manualRetryAvailable` before retrying graph work.
- `graph_retry_waiting`: a bounded automatic graph retry is scheduled; keep
  polling `import_status`.
- `citation_ready_graph_skipped`: the caller requested `skipGraphGeneration`;
  citation reads are usable, but provider graph inspection was intentionally
  skipped. An agent may still build the graph by submitting an evidence-backed
  `graph_patch_apply` payload.
- `failed`: parsing, packaging, or citation evidence commit failed before the
  source became citation-ready.

If the MCP process has restarted and the original `jobId` is no longer in
memory, call `import_status` or `import_retry_graph` with the persisted
`sourceId`. `import_retry_graph` retries only graph/wiki materialization for a
citation-ready source; it must not be used to reparse a failed import.

## Failure Classes

Use one of these labels in setup verification logs:

| Class | Meaning | User-facing next action |
| --- | --- | --- |
| `mcp_registration` | Codex has no enabled `hyprduck` MCP entry or cannot launch it. | Run `hyprduck mcp install codex`, then restart Codex if needed. |
| `path` | The `hyprduck` shell command or registered binary path is missing. | Use `~/.local/bin/hyprduck` or reopen HyprDuck to refresh the shim. |
| `provider_config` | Hosted provider settings block import or extraction. | Open Settings and fix the provider validation issue, or use Ollama for local-only setup. |
| `import_allowlist` | MCP `import_source` was called with a path outside `HYPRDUCK_MCP_ALLOWED_IMPORT_ROOTS` or with no import roots configured. | Configure an approved import root or import through the desktop picker. |
| `parsing` | No usable source artifacts exist for the workspace. | Add a PDF, DOCX, DOC, Markdown, or image file and poll `import_status` until `citation_ready` with `citationReady: true`. |
| `citation` | `get_context_pack` succeeds but returns no source/page/evidence refs. | Re-import the source or inspect `read_health` before using the answer. |
| `graph_pending` | `import_status` is `citation_ready_graph_pending` or `read_health` reports `citation_ready_graph_pending`. | Use citation-backed reads immediately; wait for retry or call `import_retry_graph` when `manualRetryAvailable` is true. |
| `unknown` | The error does not match the classes above. | Capture the command, workspace ID, and raw error for review. |

## Claude Code External Path

Register HyprDuck with Claude Code:

```bash
hyprduck mcp install claude-code
```

Local Claude Code validation is not a required release gate when the operator
does not have a Claude Code subscription. Record the exclusion as:

```text
사용자가 Claude Code를 구독하지 않아 로컬 테스트 불가
```

Claude Code proof cannot replace the required Codex proof.
