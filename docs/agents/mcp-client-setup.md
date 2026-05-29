# Agent MCP Client Setup

HyprDuck's launch agent path is Codex first. Claude Code uses the same local
MCP server but is an external validation target when the operator does not have
a Claude Code subscription. Cursor remains a later manual setup target.

## Codex Required Path

Register HyprDuck with Codex:

```bash
hyprduck mcp install codex
```

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
  `search_documents`, `read_source`, and `read_page_evidence`;
- if MCP import is part of the proof, `HYPRDUCK_MCP_ALLOWED_IMPORT_ROOTS` is set
  and `import_source` returns a `jobId` without leaking local paths; polling
  `import_status` reaches `citation_ready` with `citationReady: true`, returns
  a `sourceId`, and reports nonzero evidence count without leaking local paths;
- first answer includes at least one `sourceId`, page, and `evidenceRef`;
- `get_context_pack` returns `contextPack.schemaVersion` as
  `hyprduck.context_pack.v1`, includes `selectedEvidence[].evidenceType`, and
  includes `retrievalTrace.evidenceTypeTrace`;
- `contextPackV0` remains present for compatibility with older agent clients;
- second query reuses the same source set or performs a follow-up evidence read;
- failure class, if any.

Polling `import_status` should move through:

```text
imported -> parsing -> packaging -> citation_ready -> context_ready -> failed
```

`citation_ready` is the key HyprDuck import milestone: the source has evidence
refs an agent can use with citations. `context_ready` is later context/graph
readiness and should not block citation-backed source inspection.

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
