---
title: MCP import graph readiness recovery
date: 2026-06-01
category: integration-issues
module: MCP import status and graph materialization
problem_type: integration_issue
component: assistant
symptoms:
  - "MCP imports could become citation-ready while graph/wiki materialization was still pending, skipped, or failed."
  - "`skipGraphGeneration` updated the live job but did not durably persist graph-skipped status for restart recovery."
  - "Graph-pending persistence failures were hidden, so `sourceId` recovery looked available even when SQLite had not saved the state."
  - "Graph error messages could expose local paths through agent-facing import status payloads."
root_cause: logic_error
resolution_type: code_fix
severity: high
related_components:
  - tooling
  - database
  - documentation
tags:
  - mcp
  - import-status
  - citation-ready
  - graph-ready
  - sourceid-recovery
  - graphqlite
  - path-redaction
  - agent-workflow
---

# MCP Import Graph Readiness Recovery

## Problem

MCP import status has to separate the moment an agent can cite source evidence from the later moment when graph/wiki materialization has committed. When those states blur together, an import that already has usable citations can look blocked, or a restarted MCP process can lose the graph failure/skipped state needed for `sourceId` recovery.

## Symptoms

- `import_status` reached a citation-ready import milestone, but graph/wiki work could stay pending or skipped.
- `skipGraphGeneration` set the live job to `citation_ready_graph_skipped`, while the persisted import job row could remain unaware of the skipped graph terminal state.
- If graph status persistence returned `updated: false` or failed, the live job had no warning and later recovery by `sourceId` was misleading.
- Error strings from graph materialization could include local paths if they were stored directly in `graphGenerationErrorMessage`.
- A previous readiness shortcut treated anything "not failed" as graph-ready, which made skipped graph generation look ready (session history).

## What Didn't Work

- Treating `context_ready` as the only useful terminal state blocked the product's core agent workflow. The useful boundary is `citation_ready`: evidence refs exist and an agent can cite them, even if graph inspection is still catching up.
- Keeping import status only in the in-memory MCP registry was too fragile. `jobId` is process-local, while `sourceId` is the durable handle agents need after an MCP restart.
- Deriving graph readiness from broad negative checks such as "not failed" was too loose. Skipped, pending, retry-waiting, and failed graph states all need to keep `graphReady: false`.
- Short polling windows in tests made the async import loop look broken even when it needed a realistic deadline (session history).

## Solution

Keep citation readiness and graph readiness as separate state machines. The agent-facing MCP contract documents that `citation_ready` means citation-backed reads are usable, while `context_ready` means the GraphQLite-backed graph materialization also committed. `docs/agents/mcp-client-setup.md` now describes the normal flow and the graph-specific states, including `citation_ready_graph_pending`, `graph_retry_waiting`, and `citation_ready_graph_skipped`.

Use an explicit readiness allowlist:

```rust
fn graph_status_is_ready(status: Option<&str>) -> bool {
    matches!(status, Some("rebuilt" | "partially_applied" | "ready"))
}
```

When `skipGraphGeneration` is requested, persist the skipped graph status after citation evidence is available instead of only updating the live job:

```rust
if request.skip_graph_generation {
    job.status = ImportJobStatus::CitationReadyGraphSkipped;
    job.phase = ImportJobPhase::GraphSkipped;
    job.graph_ready = false;
    job.graph_status = Some("skipped".into());
    job.graph_generation_skipped_reason = Some("skipGraphGeneration requested".into());
    job.manual_retry_available = true;
}
```

Then call the same persistence path used by pending graph failures:

```rust
if request.skip_graph_generation {
    persist_import_job_graph_status(registry, &client, request);
}
```

For graph materialization failures, persist the redacted pending status through `update_import_job_graph_status` and record a warning if persistence does not actually update a durable row:

```rust
let result = client.update_import_job_graph_status(UpdateImportJobGraphStatusRequest {
    scope: request.scope.clone(),
    source_id: source_id.to_string(),
    status: job.status.as_str().into(),
    graph_status: job.graph_status.clone().unwrap_or_else(|| "pending".into()),
    graph_error_category: job.graph_error_category.clone(),
    graph_error_message_redacted: job.graph_generation_error_message.clone(),
    graph_retryable: job.retryable,
    graph_retry_attempt: job.retry_attempt,
    graph_max_retry_attempts: job.max_retry_attempts,
    graph_next_retry_at: job.next_retry_at,
    manual_retry_available: job.manual_retry_available,
});
record_graph_status_persist_result(registry, &request.job_id, result);
```

The engine side reads and writes import job graph status by `sourceId`, falling back from the workspace-root store to the default project store when needed. That keeps restart recovery working for the same durable import record even when the initial lookup path does not contain it.

Finally, sanitize graph errors before they enter job state:

```rust
fn sanitize_graph_error_message(message: &str) -> String {
    redact_local_path_text(message.lines().next().unwrap_or(message).trim())
}
```

The final MCP behavior is:

- `citationReady: true` means agents can use `get_context_pack`, `search_documents`, `read_source`, or `read_page_evidence`.
- `graphReady: true` only appears for explicitly ready graph statuses.
- `import_status` and `import_retry_graph` accept `sourceId` when the live `jobId` registry is gone.
- Graph persistence failures surface as redacted warnings instead of silent state loss.

## Why This Works

`sourceId` is the durable import identity; `jobId` is only a live process handle. Persisting graph status by `sourceId` makes the MCP protocol resilient to restarts without reparsing a source that already has citation artifacts.

The separation also matches Etyma's product boundary. Citation-backed evidence reuse is useful before graph/wiki materialization finishes. Graph state remains visible and retryable, but it no longer blocks the core agent workflow.

The explicit graph-ready allowlist prevents false positives: skipped and pending graph work remain inspectable states, not successful graph completion. Redacting error strings before persistence keeps agent-facing payloads aligned with the local-path disclosure rule.

## Prevention

- Test import status across an MCP restart by polling with the persisted `sourceId`, not only the live `jobId`.
- Include a skipped-graph path in integration tests so `citation_ready_graph_skipped` stays durable.
- Assert `graphReady: false` for `skipped`, `pending`, and retry-waiting states.
- Treat graph status persistence returning `updated: false` as a warning-worthy failure, not a no-op.
- Sanitize any provider, parser, or graph-materialization error before storing it in agent-facing import status.
- Use realistic async polling deadlines in MCP import tests instead of assuming graph work completes inside a very short loop (session history).

Useful verification targets:

```bash
cargo test -p etyma-cli mcp::tests::
cargo test -p etyma-engine import_job
cargo test -p etyma-cli --test mcp_server
```

## Related Issues

- [docs/agents/mcp-client-setup.md](../../agents/mcp-client-setup.md) defines the MCP polling contract, graph terminal states, `sourceId` restart recovery, and failure classes.
- [docs/agents/sqlite-graphqlite-knowledge-store-review.md](../../agents/sqlite-graphqlite-knowledge-store-review.md) gives broader context for GraphQLite-backed graph materialization and store boundaries.
- Session history showed the same design pressure earlier: a real upload had citation evidence ready while graph materialization lagged, so `citation_ready` had to be a usable product state rather than an intermediate internal detail (session history).
