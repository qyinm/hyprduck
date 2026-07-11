# CLI AND MCP AGENT NOTES

## OVERVIEW

`crates/etyma-cli` owns the human CLI, eval/TUI helpers, and the MCP server that external agents use for cited reads and controlled mutations.

## WHERE TO LOOK

| Task | Location | Notes |
| --- | --- | --- |
| CLI entry and subcommands | `src/main.rs`, `src/cli.rs`, `src/app.rs` | Keep shell behavior separate from engine runtime contracts. |
| MCP server tools | `src/mcp.rs` | Main surface for agent reads, imports, graph patches, and write flows. |
| MCP path policy | `src/mcp/policy.rs`, `src/mcp.rs` | Canonical roots, `rootDir`, and symlink escape protection. |
| Eval harness | `src/eval.rs`, `tests/eval_golden_corpus.rs` | Isolate eval output paths from user app-support data. |
| MCP tests | `tests/mcp_server.rs` | Contract tests for tools, resources, import lifecycle, and path safety. |
| MCP docs | `../../docs/mcp.md`, `../../docs/agents/mcp-client-setup.md` | Keep docs and tool behavior aligned. |

## CONVENTIONS

- MCP is a first-class workflow surface, not a read-only side channel.
- Mutating tools must be explicit, narrow, auditable, and evidence-aware: `import_source`, `graph_patch_apply`, `write_propose`, `write_commit`, corrections, and save-back flows.
- Do not accept arbitrary agent-provided production paths. Resolve workspace/import access through approved canonical roots and protected allowlists.
- Keep local paths redacted by default. Only expose them when the caller explicitly requests debugging disclosure.
- Keep `citationReady` separate from graph/wiki readiness. A citation-ready import can still be `graph_pending`, `graph_retry_waiting`, or `graph_skipped`.
- Use `sourceId` as the durable import-status recovery handle after MCP process restarts.
- Before changing import-status semantics, read `docs/solutions/integration-issues/mcp-import-graph-readiness-recovery.md`.

## ANTI-PATTERNS

- Generic filesystem or arbitrary command MCP tools.
- Treating `jobId` as the only recovery handle after restart.
- Collapsing graph generation failures into import failure when cited source reads are already ready.
- Returning unredacted local paths in agent-facing payloads by default.

## VERIFICATION

- Use the Rust commands documented in `../../docs/agents/commands.md`.
- Prefer the MCP-focused `etyma-cli` cargo filters for changes in `src/mcp.rs`, `src/mcp/`, or `tests/mcp_server.rs`.
- Run broader CLI tests when changing `src/cli.rs`, `src/app.rs`, eval, metrics, or TUI behavior.
