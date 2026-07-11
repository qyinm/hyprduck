# DESKTOP AGENT NOTES

## OVERVIEW

`apps/desktop` is the active Electron desktop shell and React inspection surface for imported document evidence, source history, and graph workspace state.

## WHERE TO LOOK

| Task | Location | Notes |
| --- | --- | --- |
| Electron window lifecycle | `main.cjs` | Keep as shell glue: window, app ready, wire domain modules. |
| IPC switch, import, preview, chat stream, engine RPC | `main/*.cjs` | Domain modules under `main/`; do not re-inflate `main.cjs`. |
| Renderer bridge | `preload.cjs` | Expose only narrow `window.etyma.invoke/listen` APIs. |
| Agent detection and terminal backends | `main/agent-terminal-*.cjs` | Keep terminal behavior in backend modules, not buried in `main.cjs`. |
| Import/settings/history/workspace state | `src/App.tsx` | Large app shell; avoid adding engine behavior here. |
| Graph workspace UI | `src/features/workspace/` | Required inspection surface for source/document/concept relationships. |
| Source hydration path | `src/workspaceSourceHydration.ts` | Preserve redaction and source/evidence identifiers. |
| Desktop tests | `tests/`, `scripts/check-*.cjs` | IA tests read source files directly. |

## CONVENTIONS

- Use Bun commands from `package.json`; do not add new `pnpm` commands.
- Keep parsing, provider calls, output packaging, and persistence in Rust. Desktop calls engine commands and renders state.
- Keep `main.cjs` and `preload.cjs` as Electron shell and bridge layers. Put reusable backend logic in `main/*.cjs`.
- Treat the graph canvas as a real inspection surface, not decorative UI.
- Keep local paths redacted in agent-facing or shareable output unless the user explicitly opts into debugging disclosure.
- When changing visible IA/copy or graph-workspace states, expect `tests/ia-ui.test.mjs` to enforce source-level contracts.

## ANTI-PATTERNS

- Reimplementing engine command behavior in React or Electron main.
- Expanding `preload.cjs` into app logic instead of a minimal bridge.
- Hiding import usability behind Screen Recording or Accessibility permissions.
- Treating `dist/`, `release/`, or bundled binaries as the source of truth for code changes.

## VERIFICATION

- Use the desktop commands documented in `../../docs/agents/commands.md`.
- Match the narrow script in `package.json` to the files changed: typecheck/build for renderer changes, IA test for visible workspace/copy changes, desktop contract check for engine command surface changes, and agent-terminal tests for `main/agent-terminal-*`.
