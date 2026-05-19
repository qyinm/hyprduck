# HyprDuck Agent Rules

HyprDuck compiles private documents into reusable, cited local context packs for AI agents.

## Product Direction

- Keep the product focused on private document evidence reuse for coding agents.
- Keep desktop first-run focused on `Add Docs -> Connect Agent -> Ask With Citations -> Verify Evidence -> Reuse`.
- Treat graph, wiki, claims, memory, and event history as internal retrieval and inspection infrastructure, not first-run product promises.
- Do not reframe HyprDuck as DeepSeek-only, generic PDF chat, graph-first, brain-first, generic memory OS, or a trust console.

## Architecture Boundaries

- Keep the active desktop shell in `apps/desktop`.
- Keep Electron main/preload as shell, window, and IPC glue.
- Keep parsing, artifact generation, provider execution, and output packaging in the Rust engine.
- Do not reintroduce frontend-owned output packaging or provider persistence.
- Do not block document import behind Screen Recording or Accessibility permissions; those apply only to optional legacy capture code.

## Artifact Contract

- Preserve provider route, local/hosted status, content hash, parse warnings, source refs, page refs, and evidence refs in agent-facing artifacts.
- Treat Source Pack as the import-time per-source artifact.
- Treat Evidence Index as the materialized map from evidence IDs to source/page/span/region data.
- Treat Context Pack as the query-time artifact external agents consume.
- Generate markdown with durable image and page references.

## Provider Rules

- Treat OpenRouter and Ollama as the current launch provider paths.
- Keep Ollama usable without an API key.
- Do not claim direct OpenAI or Anthropic support until it is implemented, configured, tested, and documented.
- Keep user-facing provider errors specific, especially for missing keys, hosted/local disclosure, and unavailable Ollama instances.

## MCP And Workspace Security

- Keep MCP read-only by default.
- Do not expose mutation, write, proposal, or approval tools in the default MCP surface.
- Do not let production MCP reads accept arbitrary agent-provided paths.
- Resolve production workspace access through approved canonical roots.
- Keep development `rootDir` overrides gated, allowlisted, and protected against symlink or path escapes.
- Redact local paths by default in agent-facing output.

## Workflow Rules

- State assumptions before coding when the request has multiple valid interpretations.
- Prefer the smallest implementation that satisfies the requested behavior.
- Touch only files needed for the task; do not refactor adjacent code unless required.
- Define the verification target before implementation and loop until it passes or a real blocker is found.
- Use Bun for JavaScript work; do not introduce new pnpm commands.
- Keep commands and operational references under `docs/agents/`, not in this file.
- Use the narrowest relevant verification command for the files changed.
- Keep public PR text focused on shipped code and verified behavior.
- Follow `.github/PULL_REQUEST_TEMPLATE.md` when opening pull requests.
- Never copy `docs/private/` paths or private planning content into public docs, commit messages, or PR bodies.
