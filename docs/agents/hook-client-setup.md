# Agent Hook Client Setup

HyprDuck hooks make the existing MCP server discoverable during agent lifecycle
events. Hooks do not replace MCP tools, schemas, workspace policy, import-root
allowlists, evidence validation, or local-path redaction.

## Codex

Install MCP first:

```bash
hyprduck mcp install codex
```

Then install hooks:

```bash
hyprduck hooks install codex
```

The Codex installer writes HyprDuck-owned entries to Codex hook configuration
and adds per-tool approval defaults for the `hyprduck` MCP server. Known
non-destructive HyprDuck tools may run automatically. Destructive removal or
overwrite of HyprDuck knowledge, source records, graph/wiki records, or saved
memories remains approval-gated.

Review hook trust in Codex:

```text
/hooks
```

Codex skips non-managed command hooks until the user reviews and trusts the
current hook definition.

Check setup:

```bash
hyprduck hooks status codex
```

## Expected behavior

- Session and prompt hooks add HyprDuck MCP guidance without requiring the user
  to type `use HyprDuck MCP`.
- Prompt-time guidance tells Codex to call `get_context_pack` when local
  document evidence may matter.
- Permission hooks can allow known non-destructive HyprDuck MCP actions.
- Destructive or unknown HyprDuck actions stay prompt-gated or are denied with
  an actionable reason.

## Current limits

- Codex hooks are deterministic command handlers. HyprDuck uses them to inject
  context and configure MCP approval defaults rather than creating a second MCP
  client path.
- Multiple matching Codex command hooks may run concurrently, so HyprDuck hooks
  should not assume they are the only policy layer.
- Claude Code and Cursor hook adapters are deferred until their paths are
  implemented and verified.
