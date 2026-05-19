@AGENTS.md
@docs/agents/commands.md

# Claude Code

- Treat this file as a thin Claude entrypoint. Shared project behavior lives in `AGENTS.md`.
- Keep commands and operational references in `docs/agents/`; do not duplicate them here.
- Add durable project behavior to `AGENTS.md`, reusable procedures to `docs/agents/`, and personal notes to `CLAUDE.local.md`.
- For broad or ambiguous changes, explore the codebase first, make a short plan, then edit.
- For narrow mechanical fixes, edit directly and keep the diff scoped.
- After code changes, run the narrowest relevant verification from `docs/agents/commands.md` and report exactly what ran or why it was skipped.
- If this file and `AGENTS.md` diverge, prefer `AGENTS.md` for shared project behavior.
