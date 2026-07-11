# ENGINE AGENT NOTES

## OVERVIEW

`crates/etyma-engine` owns parsing, provider execution, durable knowledge storage, graph/wiki materialization, context packs, and source-backed answer behavior.

## STRUCTURE

```text
src/
|-- main.rs                    # JSON stdin/stdout engine binary and dispatch
|-- lib.rs                     # Public module wiring
|-- application/               # Command handlers and orchestration services
|-- domains/                   # Ingest, provider, retrieval, context, knowledge, brain, agent workflow
|-- adapters/                  # Documents, persistence, process lookup, provider HTTP adapters
|-- graph_commit.rs            # Graph snapshot commit path
|-- graph_history.rs           # Graph history reads
`-- graph_patch_policy.rs      # Agent graph mutation policy
```

## WHERE TO LOOK

| Task | Location | Notes |
| --- | --- | --- |
| Engine command dispatch | `src/main.rs`, `src/application/commands/` | Keep command names and response shapes grep-friendly. |
| Import pipeline | `src/domains/ingest/`, `src/application/services/ingest_service.rs` | Preserve source/page/evidence refs. |
| Provider calls | `src/domains/provider/`, `src/adapters/providers/openai_compatible.rs` | OpenRouter and Ollama share OpenAI-compatible chat shape. |
| SQLite + GraphQLite persistence | `src/adapters/persistence/knowledge_store.rs` | Durable store hotspot with embedded tests. |
| Context packs | `src/domains/context_pack/`, `src/application/services/context_pack_service.rs` | Query-time agent artifact. |
| Graph/wiki materialization | `src/domains/agent_workflow/`, `src/domains/brain/`, `src/graph_commit.rs` | Validate evidence/source refs before writing. |
| Tests | `src/tests.rs`, `src/tests/`, `tests/fixture_roundtrip.rs` | Use focused cargo filters first. |

## CONVENTIONS

- Keep `main.rs` transport and dispatch thin; move behavior into application services, domains, or adapters.
- Keep provider-specific HTTP request/response shape in adapters; keep provider selection, config, and readiness in provider domain/application code.
- Treat SQLite + GraphQLite as authoritative durable state. JSON/readable files are read models or compatibility artifacts, not primary persistence.
- Validate source refs, page refs, evidence refs, relation endpoints, claim refs, and wiki refs before materializing graph/wiki changes.
- Preserve provider route, local/hosted disclosure, content hash, parse warnings, source refs, page refs, and evidence refs in agent-facing artifacts.
- Keep local-path redaction intact for MCP/agent outputs unless explicitly opted into debugging disclosure.

## ANTI-PATTERNS

- A generic command framework that obscures the existing `EngineCommand`/`EngineRequest` contract without shared middleware need.
- Provider plugin abstractions that imply direct OpenAI or Anthropic support before implementation and tests exist.
- Persistence adapters that absorb graph patch policy or provider validation logic.
- Tests that only validate invented helpers instead of the real import/provider/graph failure mode.

## VERIFICATION

- Use the Rust commands documented in `../../docs/agents/commands.md`.
- Prefer a focused `etyma-engine` cargo filter for local domain changes before running the broader core test alias.
- If an engine change alters MCP-visible behavior, also verify the relevant `etyma-cli` MCP tests.
