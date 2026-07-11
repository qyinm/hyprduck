# Agent Hook Client Setup

Etyma hooks make the existing MCP server discoverable during agent lifecycle
events. Hooks do not replace MCP tools, schemas, workspace policy, import-root
allowlists, evidence validation, or local-path redaction.

## Codex

Install MCP first:

```bash
etyma mcp install codex
```

Then install hooks:

```bash
etyma hooks install codex
```

The Codex installer writes Etyma-owned entries to Codex hook configuration
and adds per-tool approval defaults for the `etyma` MCP server. Known
non-destructive Etyma tools may run automatically. Destructive removal or
overwrite of Etyma knowledge, source records, graph/wiki records, or saved
memories remains approval-gated.

Review hook trust in Codex:

```text
/hooks
```

Codex skips non-managed command hooks until the user reviews and trusts the
current hook definition.

Check setup:

```bash
etyma hooks status codex
```

## Expected behavior

- Session and prompt hooks add Etyma MCP guidance without requiring the user
  to type `use Etyma MCP`.
- Prompt-time guidance tells Codex to call `get_context_pack` when local
  document evidence may matter.
- Permission hooks can allow known non-destructive Etyma MCP actions.
- Destructive or unknown Etyma actions stay prompt-gated or are denied with
  an actionable reason.

## Current limits

- Codex hooks are deterministic command handlers. Etyma uses them to inject
  context and configure MCP approval defaults rather than creating a second MCP
  client path.
- Multiple matching Codex command hooks may run concurrently, so Etyma hooks
  should not assume they are the only policy layer.
- Claude Code and Cursor hook adapters are deferred until their paths are
  implemented and verified.
