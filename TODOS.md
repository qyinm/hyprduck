# TODOS

## Direction

HyprDuck is moving from a document parser into a local evidence compiler for AI agents.

The parser remains the wedge, but the next product milestone is not "more chat"
or a memory OS. The next milestone is proving the reusable cited context loop:
source pack, evidence index, Context Pack v0, and MCP agent citation quality.

Current roadmap order:

```text
1. Context pack demo and agent citation proof
2. Distribution proof through Claude Code, Codex, and Cursor
3. Benchmark proof for document-to-agent-context quality
```

---

## P0 - Golden corpus and eval harness - Done

**What:** Add a small benchmark corpus and CLI/eval path for extraction, retrieval, and context-pack quality.

**Why:** HyprDuck is making a local-first promise. Without a corpus, we cannot tell whether graph/retrieval changes improve agent usefulness or only make the UI look richer.

**Fixture cases:**

- clean single-source document
- duplicate/alias entity case
- multi-source contradiction case
- stale source case
- table-heavy document
- low-quality OCR/parse case

**Metrics:**

- entity precision/recall proxy
- claim citation coverage
- relation evidence coverage
- contradiction detection
- context-pack relevance
- local/hosted model latency

**Acceptance:**

- [x] Fixtures live under a stable test path.
- [x] Expected entities, claims, relations, and evidence refs are checked in.
- [x] A CLI or test command can run the corpus locally.
- [x] The result reports quality and latency in a human-readable format.
- [x] The corpus can compare heuristic, hosted-model, and local-model extraction paths.

**Effort:** M
**Priority:** P0
**Depends on:** Structured extraction artifact

---

## P0 - Local retrieval baseline and context-pack hardening - Done

**What:** Replace simple substring-style brain search with a local retrieval baseline, then harden `GetContextPack` around that retrieval.

**Why:** Agent value depends on whether agents can receive the right sources,
claims, relations, and evidence at the right time. Opening MCP over weak
retrieval would freeze a poor external contract too early.

**Scope:**

- Add local full-text/BM25-style search over sources, wiki pages, claims, entities, memories, evidence, and events.
- Expand results through graph neighbors and evidence refs.
- Include recency and scope filters.
- Make context packs include relevant memories, active claims, entities, relations, source snippets, recent events, warnings, and evidence refs.
- Keep output deterministic enough for regression tests.

**Acceptance:**

- [x] Search no longer depends only on naive substring matching.
- [x] Context packs include entities, claims, relations, memories, evidence, and recent events when relevant.
- [x] Results expose citation/evidence IDs an agent can quote back.
- [x] Golden corpus tests catch retrieval regressions.
- [x] CLI output reflects the same context-pack fields the engine returns.

**Effort:** M
**Priority:** P0
**Depends on:** Golden corpus, structured extraction artifact, current brain search API

---

## P1 - Save-back and correction persistence - Done

**What:** Implement durable save-back flows for answers, corrections, and graph edits.

**Why:** The UI already suggests that answers/corrections can become wiki pages, claims, notes, source metadata, or graph updates. Those writes must preserve source-backed provenance instead of mutating generated artifacts opaquely.

**Scope:**

- Implement the correction ledger and source snapshot write flow described in `docs/workspace-corrections.md`.
- Persist rename, merge, keep-separate, and source-note decisions.
- Save approved answers as wiki pages, claims, memories, or source notes.
- Keep original imported artifacts immutable.
- Replay corrections during re-materialization.

**Acceptance:**

- [x] Rename/merge/keep-separate actions survive re-aggregation.
- [x] Approved answers can be saved as wiki page, claim, memory, or source note.
- [x] Every save-back creates an event log entry.
- [x] Original source files and source-derived raw artifacts are never overwritten.
- [x] Re-materialization replays correction and save-back history.

**Effort:** M
**Priority:** P1
**Depends on:** Structured extraction artifact, evidence provenance, source-backed project snapshots

---

## P1 - Model-task matrix and latency budget - Done

**What:** Measure and document which hosted and local models are good enough for parse, extraction, merge, and answer workloads.

**Why:** Users can pick a weak local model and get a graph that feels broken even when the app works. HyprDuck needs recommended defaults and clear fallbacks for local-first operation.

**Scope:**

- Compare OpenRouter-hosted models and local Ollama models on the golden corpus.
- Track parse latency, extraction quality, merge quality, answer groundedness, and generated graph quality.
- Define acceptable latency budgets for local and hosted paths.
- Document recommended defaults in repo docs and settings copy.

**Acceptance:**

- [x] Each major task has a recommended default model path.
- [x] Local model limits are explicit.
- [x] The benchmark reports latency and quality together.
- [x] Settings copy can warn users when a model is likely too weak for a task.

**Effort:** M
**Priority:** P1
**Depends on:** Golden corpus eval, structured extraction artifact, retrieval baseline

---

## P2 - Read-only MCP v0 - Done

**What:** Expose HyprDuck's local document context through a read-only MCP
surface after extraction and retrieval stabilize.

**Why:** The first agent-facing contract should be read-only. Agents should be
able to retrieve source/page/evidence-backed context before any future write
surface is considered.

**Initial tools:**

- `search_documents`
- `search_brain`
- `get_context_pack`
- `read_source`
- `read_page_evidence`
- `read_wiki_page`
- `read_node`
- `read_recent_events`
- `read_graph_history`
- `read_graph_snapshot`
- `read_health`

**Acceptance:**

- [x] MCP tools return the same evidence-backed IDs as the engine/CLI.
- [x] Tools cannot mutate document context artifacts.
- [x] Tool responses include source/evidence provenance.
- [x] The contract is documented with examples.

**Effort:** M
**Priority:** P2
**Depends on:** Retrieval baseline, context-pack hardening, stable context pack schema

## P3 - Context pack demo and distribution proof

**What:** Make a known fixture prove the full loop from import to cited agent
answer.

**Why:** HyprDuck needs proof that one local ingest can be reused by Claude Code,
Codex, Cursor, or another MCP-aware agent without re-uploading documents or
losing page evidence.

**Scope:**

- Define `hyprduck demo` target behavior.
- Keep `hyprduck context` as the query-time context-pack alias.
- Keep `hyprduck documents search` as the document-search alias.
- Publish setup guides for at least two MCP clients.
- Add prompt-injection and hosted-provider disclosure warnings to first-run docs.
- Defer shared/team workflows until repeated cited reuse is proven.

**Acceptance:**

- [ ] A fixture demo completes under 60 seconds.
- [ ] A sample query writes schema-valid `context_pack.json`.
- [ ] One MCP client can answer with source/page/evidence citations.
- [ ] A Claude Code, Codex, Cursor, or comparable MCP client dry run proves cited context-pack reuse.
- [ ] Default docs and tool lists contain no retired trust-layer UX language.
- [ ] Missing provider and partial import failures produce specific warnings.

**Effort:** L
**Priority:** P3
**Depends on:** Read-only MCP, Source Pack/Evidence Index, Context Pack v0

---

## Completed / Existing Substrate

- [x] Local document ingestion wedge
- [x] Internal graph/wiki materialization substrate
- [x] Source metadata persistence from composer intent
- [x] Read-only document context engine API
- [x] Context readiness lint path
- [x] Structured extraction artifact
  - Added `StructuredExtractionArtifact` and nested entity/topic/claim/relation/page-ref contracts.
  - Re-exported the contract through `hyprduck-engine-types`.
  - Persisted per-source `artifacts/<source_id>/extraction.json`.
  - Marked the current extractor as `extractor=heuristic`.
  - Prevented evidence-free claims/relations from becoming generated graph context.
  - Covered artifact shape and source/evidence round trip in unit tests.
- [x] Golden corpus and eval harness
  - Added six checked-in benchmark fixtures under `crates/hyprduck-engine/tests/fixtures/brain-corpus`.
  - Added `hyprduck eval golden-corpus` with `--fixtures` and `--mode heuristic|hosted|local|all`.
  - Reports entity recall, claim citation coverage, relation evidence coverage, evidence snippet coverage, contradiction detection, context-pack relevance, and latency.
  - Covered the CLI eval path with an integration test.
- [x] Local retrieval baseline and context-pack hardening
  - Replaced substring-only score counting with tokenized local scoring and suffix/plural normalization.
  - Expanded context packs through matched sources, evidence, entities, claims, relations, memories, and events.
  - Added evidence IDs to search snippets for agent-readable citations.
  - Added CLI context-pack counts for memories, entities, claims, and relations.
  - Covered plural-token retrieval and graph/evidence expansion with a regression test.
- [x] Context Pack v0, Source Pack, and Evidence Index artifacts
  - Added `schemas/context-pack.schema.json`, `schemas/source-pack.schema.json`, and `schemas/evidence-index.schema.json`.
  - Writes `source_pack.json` and `evidence_index.json` during output packaging.
  - Persists query-time `context_pack.json` and history files when requested.
  - Covers schema and artifact round trips in tests.
- [x] MCP document-context naming and root hardening
  - Added `search_documents` and `read_page_evidence`.
  - Kept `search_brain` and `hyprduck://brain/...` as compatibility surfaces.
  - Gated development `rootDir` behind `HYPRDUCK_MCP_ALLOW_ROOT_DIR=1` and a canonical `HYPRDUCK_MCP_ALLOWED_ROOTS` allowlist.
  - Rejects workspace path, prefix, and symlink escapes.
  - Keeps `read_health` read-only and adds per-source readiness fields for status, failed pages, content-hash state, and provider route.
- [x] CLI context aliases
  - Added `hyprduck context` / `hyprduck context-pack`.
  - Added `hyprduck documents search` / `hyprduck docs search`.
- [x] Save-back and correction persistence
  - Existing workspace rename, merge, and keep-separate corrections remain ledger-backed and replayable.
  - Claim updates now write durable `graph/claims.json` records.
  - Link updates now write durable `graph/edges.json` records.
  - Wiki save-back writes durable `wiki/save-back/*.md` pages.
  - Save-back records are replayed during internal graph/wiki materialization.
