# Agent Terminal

Agent Terminal is the HyprDuck Desktop surface for opening detected local coding agents with HyprDuck context attached. HyprDuck remains the evidence provider. Codex, Claude Code, Pi Agent, Hermes, or another supported external agent owns the reasoning and work.

## Supported agents

HyprDuck v1 detects these agent commands from `PATH`:

- Codex: `codex`
- Claude Code: `claude`
- Pi Agent: `pi-agent`, then `pi`
- Hermes: `hermes`

Generic shell and custom command entries are not part of the default v1 picker. They must remain hidden or disabled unless a later Advanced mode is explicitly designed and reviewed.

## Context handoff

Every Agent Terminal session must expose a context handoff before the first real prompt:

- MCP status
- workspace ID
- selected context or evidence scope
- copyable or launchable attach instructions

The handoff tells the selected agent to call HyprDuck MCP `get_context_pack` before answering and to use cited evidence refs, page refs, and source refs from the returned pack.

## Backend gates

The default backend is intentionally disabled until the native terminal spike passes. It returns an `external_ghostty` fallback so the product can still validate the HyprDuck evidence-to-agent loop.

To probe a native Ghostty backend:

```sh
HYPRDUCK_AGENT_TERMINAL_BACKEND=ghostty-native \
HYPRDUCK_GHOSTTY_BACKEND_MODULE=<module-name-or-path> \
bun run --cwd apps/desktop check:agent-terminal-backend
```

The native backend must implement:

- `createSession`
- `write`
- `resize`
- `kill`
- `subscribe`
- `snapshotStatus`

The spike is not accepted until a packaged desktop build can run a real process-backed Codex session with full-screen TUI rendering, approval flow interaction, resize, copy/paste, and clean termination.

## External Ghostty fallback

When native embedding is unavailable, HyprDuck returns an explicit external Ghostty fallback plan. The fallback includes the selected agent command and the same context attach instructions used by the embedded flow.

This fallback is an official v1 path, not an error state. It preserves terminal fidelity while keeping HyprDuck focused on context/evidence attachment.

## Security boundary

Renderer code cannot pass arbitrary shell commands to Electron main. Agent session creation accepts only known agent IDs from the detection registry. Command resolution happens in the main process.

The renderer remains sandboxed with `contextIsolation: true`, `nodeIntegration: false`, and `sandbox: true`.

## Verification

Run the focused checks:

```sh
bun run --cwd apps/desktop check:agent-terminal-backend
bun run --cwd apps/desktop test:agent-detection
bun run --cwd apps/desktop test:agent-terminal-sessions
bun run --cwd apps/desktop test:agent-terminal-ghostty
bun run --cwd apps/desktop test:agent-terminal-fallbacks
bun run --cwd apps/desktop frontend:typecheck
```

Before Agent Terminal replaces direct ask by default, dogfood Codex and at least one second agent. Record evidence that the pre-prompt handoff shows MCP status, workspace ID, selected context/evidence scope, and attach instructions in both embedded and external Ghostty fallback paths.
