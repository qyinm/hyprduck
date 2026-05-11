# TODOS

## Direction

HyprDuck is moving from a document parser into a local-first brain repo and trust console for AI agents.

The parser remains the wedge, but the next product milestone is not "more chat" or "open MCP immediately." The next milestone is making the materialized brain measurable and retrievable: golden-corpus evaluation, local retrieval quality, context-pack hardening, and reviewable save-back.

Current roadmap order:

```text
1. Team/company brain governance
```

---

## P0 - Golden corpus and eval harness - Done

**What:** Add a small benchmark corpus and CLI/eval path for extraction, retrieval, and context-pack quality.

**Why:** HyprDuck is making a local-first promise and a trust-console promise. Without a corpus, we cannot tell whether graph/retrieval changes improve agent usefulness or only make the UI look richer.

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

**Why:** Agent-maintained brain value depends on whether agents can receive the right memories, sources, claims, relations, and evidence at the right time. Opening MCP over weak retrieval would freeze a poor external contract too early.

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

## P1 - Reviewable save-back and correction persistence - Done

**What:** Implement durable save-back flows for answers, corrections, and graph edits.

**Why:** The UI already suggests that answers/corrections can become wiki pages, claims, notes, source metadata, or graph updates. Those writes must go through source-backed provenance and reviewable events instead of mutating generated artifacts opaquely.

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

**What:** Measure and document which hosted and local models are good enough for parse, extraction, merge, review, and answer workloads.

**Why:** Users can pick a weak local model and get a graph that feels broken even when the app works. HyprDuck needs recommended defaults and clear fallbacks for local-first operation.

**Scope:**

- Compare OpenRouter-hosted models and local Ollama models on the golden corpus.
- Track parse latency, extraction quality, merge quality, answer groundedness, and memory/write proposal quality.
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

**What:** Expose HyprDuck's local brain through a read-only MCP surface after extraction and retrieval stabilize.

**Why:** The product direction is agent-maintained personal/company brain, but the first agent-facing contract should be read-only. Agents should be able to retrieve context before they can write durable memory.

**Initial tools:**

- `search_brain`
- `get_context_pack`
- `read_source`
- `read_wiki_page`
- `read_node`
- `read_recent_events`
- `read_health`

**Acceptance:**

- [x] MCP tools return the same evidence-backed IDs as the engine/CLI.
- [x] Tools cannot mutate brain artifacts.
- [x] Context packs are useful in Claude Code, Codex, Cursor, or another MCP client.
- [x] Tool responses include source/evidence provenance.
- [x] The contract is documented with examples.

**Effort:** M
**Priority:** P2
**Depends on:** Retrieval baseline, context-pack hardening, stable brain repo schema

---

## P2 - Proposed-write MCP and policy engine - Done

**What:** Let agents propose memories, claims, links, observations, and source notes through a policy-controlled write path.

**Why:** HyprDuck should become an agent-maintained brain, but agent writes must stay auditable and reversible. Safe memory writes can be auto-applied; risky claims, links, destructive rewrites, and company-scope updates should require review.

**Initial tools:**

- `propose_memory`
- `propose_claim`
- `propose_link`
- `append_observation`
- `add_source_note`
- `request_consolidation`

**Policy defaults:**

- Low-risk personal/project memory can auto-apply.
- Claims without evidence require review.
- Typed links that affect entity identity require review.
- Company-scope memory requires review by default.
- Destructive rewrites and merges require review.

**Acceptance:**

- [x] Every write proposal creates a brain event.
- [x] Auto-applied writes are limited to safe categories.
- [x] Risky writes appear in the Health/Review queue.
- [x] Review decisions are persisted and auditable.
- [x] Agents cannot overwrite source truth directly.

**Effort:** L
**Priority:** P2
**Depends on:** Read-only MCP v0, reviewable save-back, policy metadata

---

## P3 - Team/company brain governance

**What:** Turn personal/project brain scopes into team/company-ready governance.

**Why:** The desired direction is an agent-maintained personal/company brain. Team/company memory needs source visibility, review authority, audit, rollback, and scope boundaries before it can be trusted.

**Scope:**

- Clarify `personal`, `project`, `team`, and `company` scopes in storage and UI.
- Add source visibility and write policy per scope.
- Add audit and rollback paths for accepted agent writes.
- Add workspace health summaries for company-level memory.
- Keep local-first operation as the default.

**Acceptance:**

- [ ] Brain objects carry explicit scope.
- [ ] Team/company writes require stronger review policy than personal/project writes.
- [ ] Audit log can explain who/what wrote each memory, claim, or relation.
- [ ] Rollback can undo accepted proposals without touching original sources.
- [ ] UI copy makes scope and trust level visible.

**Effort:** L
**Priority:** P3
**Depends on:** Proposed-write MCP, review queue, event log, policy engine

---

## Completed / Existing Substrate

- [x] Local document ingestion wedge
- [x] Brain repo schema and materialization
- [x] Source metadata persistence from composer intent
- [x] Read-only brain engine API
- [x] Proposed brain update queue
- [x] Safe memory proposal auto-apply
- [x] Brain review health queue
- [x] Brain maintenance lint loop
- [x] README and repository metadata aligned with agent-brain positioning
- [x] Structured extraction artifact
  - Added `StructuredExtractionArtifact` and nested entity/topic/claim/relation/page-ref contracts.
  - Re-exported the contract through `duckdocs-engine-types`.
  - Persisted per-source `artifacts/<source_id>/extraction.json`.
  - Marked the current extractor as `extractor=heuristic`.
  - Prevented evidence-free claims/relations from becoming trusted brain context.
  - Covered artifact shape and source/evidence round trip in unit tests.
- [x] Golden corpus and eval harness
  - Added six checked-in benchmark fixtures under `crates/duckdocs-engine/tests/fixtures/brain-corpus`.
  - Added `duckdocs eval golden-corpus` with `--fixtures` and `--mode heuristic|hosted|local|all`.
  - Reports entity recall, claim citation coverage, relation evidence coverage, evidence snippet coverage, contradiction detection, context-pack relevance, and latency.
  - Covered the CLI eval path with an integration test.
- [x] Local retrieval baseline and context-pack hardening
  - Replaced substring-only score counting with tokenized local scoring and suffix/plural normalization.
  - Expanded context packs through matched sources, evidence, entities, claims, relations, memories, and events.
  - Added evidence IDs to search snippets for agent-readable citations.
  - Added CLI context-pack counts for memories, entities, claims, and relations.
  - Covered plural-token retrieval and graph/evidence expansion with a regression test.
- [x] Reviewable save-back and correction persistence
  - Existing workspace rename, merge, and keep-separate corrections remain ledger-backed and replayable.
  - Accepted claim proposals now write durable `graph/claims.json` records.
  - Accepted link proposals now write durable `graph/edges.json` records.
  - Accepted wiki-page proposals now write durable `wiki/save-back/*.md` pages.
  - Accepted save-back proposals are replayed during brain repo materialization.
