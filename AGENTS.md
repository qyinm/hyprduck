# PROJECT KNOWLEDGE BASE

**Generated:** 2026-06-02 06:20 KST
**Commit:** 59135b4
**Branch:** main

## OVERVIEW

HyprDuck compiles private documents into reusable, cited local context packs for coding agents. The active stack is an Electron desktop shell in `apps/desktop`, a Rust engine/CLI workspace in `crates/`, local SQLite + GraphQLite knowledge storage, and MCP as the agent workflow surface.

## STRUCTURE

```text
HyprDuck/
|-- apps/desktop/                 # Active Electron shell and graph workspace UI
|-- apps/site/                    # Public Astro site
|-- crates/hyprduck-engine/       # Parsing, providers, persistence, graph/wiki/materialization
|-- crates/hyprduck-cli/          # CLI, MCP server, eval and TUI surfaces
|-- crates/hyprduck-engine-types/ # Shared command, response, graph, artifact contracts
|-- crates/hyprduck-engine-client/# Engine subprocess client
|-- crates/hyprduck-knowledge/    # Shared knowledge/brain record types
|-- docs/agents/                  # Operational commands and agent setup docs
|-- docs/solutions/               # Reusable fixes and architecture patterns
|-- schemas/                      # External JSON schema contracts
`-- scripts/                      # Desktop engine sync/release helpers
```

## WHERE TO LOOK

| Task | Location | Notes |
| --- | --- | --- |
| Desktop import, settings, history, graph UI | `apps/desktop` | See its local `AGENTS.md`; keep it shell/UI only. |
| Electron IPC or agent terminal behavior | `apps/desktop/main.cjs`, `apps/desktop/main/` | Main/preload are glue, not parser/provider logic. |
| Parsing, artifacts, providers, graph/wiki state | `crates/hyprduck-engine` | See its local `AGENTS.md`; this owns runtime behavior. |
| MCP tools, import status, agent mutations | `crates/hyprduck-cli/src/mcp.rs` | See `crates/hyprduck-cli/AGENTS.md` and `docs/mcp.md`. |
| Engine command and artifact DTOs | `crates/hyprduck-engine-types/src/lib.rs`, `schemas/` | Keep Rust types and JSON schemas aligned. |
| Build/test commands | `docs/agents/commands.md`, `justfile` | Commands belong in `docs/agents/`, not root instruction files. |
| Prior fixes and reusable patterns | `docs/solutions/` | Check before re-solving integration or architecture issues. |
| Public PR wording | `.github/PULL_REQUEST_TEMPLATE.md` | Keep public text to shipped code and verified behavior. |

## PRODUCT DIRECTION

- Keep the product focused on private document evidence reuse for coding agents.
- Keep desktop first-run focused on `Add Docs -> Connect Agent -> Ask With Citations -> Verify Evidence -> Reuse`.
- Keep the desktop app's graph canvas as a required inspection surface for document/source/concept relationships.
- Treat graph, wiki, claims, memory, and event history as retrieval and inspection infrastructure, not first-run product promises or marketing claims.
- Do not reframe HyprDuck as DeepSeek-only, generic PDF chat, graph-first, brain-first, generic memory OS, or a trust console.

## ARCHITECTURE BOUNDARIES

- Keep the active desktop shell in `apps/desktop`.
- Keep Electron main/preload as shell, window, and IPC glue.
- Keep parsing, artifact generation, provider execution, and output packaging in the Rust engine.
- Treat SQLite + GraphQLite as the authoritative knowledge/graph store direction; do not revive file artifacts as primary persistence.
- Do not reintroduce frontend-owned output packaging or provider persistence.
- Do not block document import behind Screen Recording or Accessibility permissions; those apply only to optional legacy capture code.

## ARTIFACT CONTRACT

- Preserve provider route, local/hosted status, content hash, parse warnings, source refs, page refs, and evidence refs in agent-facing artifacts.
- Treat Source Pack as the import-time per-source artifact.
- Treat Evidence Index as the materialized map from evidence IDs to source/page/span/region data.
- Treat Context Pack as the query-time artifact external agents consume.
- Generate markdown with durable image and page references.
- Keep JSON schemas in `schemas/` synchronized with `crates/hyprduck-engine-types/src/lib.rs` tests.

## PROVIDER RULES

- Treat OpenRouter and Ollama as the current launch provider paths.
- Keep Ollama usable without an API key.
- Do not claim direct OpenAI or Anthropic support until it is implemented, configured, tested, and documented.
- Keep user-facing provider errors specific, especially for missing keys, hosted/local disclosure, and unavailable Ollama instances.

## MCP AND WORKSPACE SECURITY

- Treat MCP as a first-class agent workflow surface, including controlled mutation for import, proposal, approval, correction, and save-back flows.
- Do not require MCP to be read-only by default; mutating tools may appear in the normal MCP surface when they are explicit, well-scoped, and accurately annotated as mutating.
- Keep mutating MCP tools narrow, auditable, and evidence-aware; prefer purpose-built tools such as `import_source`, `write_propose`, `write_commit`, and correction/save-back operations over generic filesystem or arbitrary command tools.
- Do not let production MCP tools accept arbitrary agent-provided paths.
- Resolve production workspace and import access through approved canonical roots.
- Keep `rootDir` and import path overrides allowlisted and protected against symlink or path escapes.
- Redact local paths by default in agent-facing output unless the user explicitly opts into local-path disclosure for debugging.
- Keep `citationReady` separate from graph/wiki readiness; `sourceId` is the durable retry/recovery handle after MCP restarts.

## CONVENTIONS

- State assumptions before coding when the request has multiple valid interpretations.
- Use Bun for JavaScript work; do not introduce new `pnpm` commands.
- Keep commands and operational references under `docs/agents/`, not in this file.
- Check `docs/solutions/` for prior fixes and reusable patterns before re-solving recurring implementation, integration, or workflow problems.
- Prefer the smallest implementation that satisfies the requested behavior.
- Touch only files needed for the task; do not refactor adjacent code unless required.
- Define the verification target before implementation and loop until it passes or a real blocker is found.
- Use the narrowest relevant verification command for the files changed.
- Keep public PR text focused on shipped code and verified behavior.
- Follow `.github/PULL_REQUEST_TEMPLATE.md` when opening pull requests.

## ANTI-PATTERNS

- Frontend-owned output packaging or provider persistence.
- Generic filesystem or arbitrary command MCP tools in production.
- Public docs, commit messages, or PR bodies that copy `docs/private/` paths or private planning content.
- Provider messaging that implies direct OpenAI or Anthropic support before the repo actually ships it.
- Graph/wiki/claims/memory/event history marketed as the first-run promise instead of retrieval infrastructure.
- PR descriptions that claim broader verification than the current HEAD actually supports.

## NOTES

- `CLAUDE.md` is a thin entrypoint that points back here and to `docs/agents/commands.md`.
- Canonical build, test, run, and just aliases live in `docs/agents/commands.md`.
- The public site has a GitHub Pages workflow; desktop packaging is local-script driven.
- `docs/private/` may exist in local checkouts but must not leak into public-facing artifacts.
