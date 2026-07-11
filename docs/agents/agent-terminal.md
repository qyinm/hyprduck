# Agent Terminal

Agent Terminal is the Etyma Desktop surface for opening the user's default shell or detected local coding agents with Etyma context attached. Etyma remains the evidence provider. Codex, Claude Code, Pi Agent, Hermes, or another supported external agent owns the reasoning and work.

## Supported agents

Etyma v1 detects these agent commands from `PATH`:

- Codex: `codex`
- Claude Code: `claude`
- Pi Agent: `pi-agent`
- Hermes: `hermes`

`New Terminal` opens only the user's default shell resolved inside Electron main. Renderer-provided custom commands, arbitrary shell paths, and custom executable fields stay rejected.

## Context handoff

Every Agent Terminal session must expose a context handoff before the first real prompt:

- MCP status
- workspace ID
- selected context or evidence scope
- copyable or launchable attach instructions
- explicit handoff state; terminal input is blocked until the backend reports the handoff attached or acknowledged

The handoff tells the selected agent to call Etyma MCP `get_context_pack` before answering and to use cited evidence refs, page refs, and source refs from the returned pack.

## Backend gates

Etyma Desktop now uses a real PTY backend by default when `node-pty` is available. The backend launches only detected agent commands from the allowlisted registry or the main-resolved default shell for `New Terminal`, streams process output to the renderer terminal, accepts keyboard input, handles resize, and keeps external Ghostty available as a fallback.

The Ghostty-native backend remains an optional spike path:

As of May 29, 2026, official Ghostty documentation still describes `libghostty` as not yet a stable standalone API. Etyma keeps the native Ghostty backend behind an explicit module gate until a maintained embeddable package passes the same packaged-build and Codex/Claude dogfood checks as the PTY backend.

To probe a native Ghostty backend:

```sh
ETYMA_AGENT_TERMINAL_BACKEND=ghostty-native \
ETYMA_GHOSTTY_BACKEND_MODULE=<module-name-or-path> \
bun run --cwd apps/desktop check:agent-terminal-backend
```

The native backend must implement:

- `createSession`
- `write`
- `resize`
- `kill`
- `subscribe`
- `snapshotStatus`

The Ghostty spike is not accepted until a packaged desktop build can run a real process-backed Codex session with full-screen TUI rendering, approval flow interaction, resize, copy/paste, and clean termination. Until then, the PTY backend is the embedded implementation path.

## External Ghostty fallback

When embedded PTY or native Ghostty embedding is unavailable, Etyma returns an explicit external Ghostty fallback plan. The fallback includes the selected agent command and the same context attach instructions used by the embedded flow.

This fallback is an official v1 path, not an error state. It preserves terminal fidelity while keeping Etyma focused on context/evidence attachment.

## Security boundary

Renderer code cannot pass arbitrary shell commands to Electron main. Agent session creation accepts known agent IDs from the detection registry or `kind: "shell"` for the main-resolved default shell. Command resolution happens in the main process, and `command`, `customCommand`, `shell`, `shellCommand`, and `executable` payload fields remain rejected.

The renderer remains sandboxed with `contextIsolation: true`, `nodeIntegration: false`, and `sandbox: true`.

## Verification

Run the focused checks:

```sh
cd apps/desktop
bun run check:agent-terminal-backend
bun run test:agent-detection
bun run test:agent-terminal-sessions
node --test tests/agent-terminal-pty.test.cjs
bun run test:agent-terminal-ghostty
bun run test:agent-terminal-fallbacks
bun run smoke:agent-terminal-pty:electron
bun run smoke:agent-terminal-agents
bun run smoke:agent-terminal-agents:electron
bun run frontend:typecheck
```

Before broad rollout, dogfood Codex and at least one second agent. Record evidence that the pre-prompt handoff shows MCP status, workspace ID, selected context/evidence scope, and attach instructions in both embedded and external Ghostty fallback paths.
