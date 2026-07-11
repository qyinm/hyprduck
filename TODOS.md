# TODOS

## Direction

Etyma is moving from a document parser into a local evidence compiler for AI agents.

The parser remains the wedge, but the next product milestone is not "more chat"
or a memory OS. The next milestone is proving the reusable cited context loop:
source pack, evidence index, Context Pack v1, MCP agent citation quality, and
installed-app distribution.

Current roadmap order:

```text
1. Close the installed-app shell-command/shim gap found during Codex proof
2. Dogfood Agent Terminal context handoff with Codex and one second agent
3. Extend distribution proof beyond Codex when the client is available
4. Keep benchmark proof current for document-to-agent-context quality
```

## Current Next Slice

**What:** Finish the installed-app distribution closeout after the successful
Codex proof.

**Why:** `docs/dogfood-reports/2026-06-02-codex-installed-app-distribution-proof.md`
records `pass_active_codex_exec`, but also records that `~/.local/bin/etyma`
can still point at an unmanaged repo-local development binary. The active Codex
registration used the app-bundled CLI, but the shell-command path still needs a
clean user-facing story.

**Acceptance:**

- [ ] Opening or installing the packaged app refreshes a managed `etyma`
  shell command without overwriting unrelated unmanaged commands.
- [ ] `etyma doctor` resolves the app-bundled engine runtime for an installed
  app.
- [ ] `etyma mcp install codex` registers the app-bundled CLI and forwards
  import-root/project-store environment values.
- [ ] The Codex installed-app proof can be rerun without the stale-shim warning.
- [ ] Agent Terminal dogfood records the context handoff state before broad
  rollout.

---

## P0 - Golden corpus and eval harness - Done

**What:** Add a small benchmark corpus and CLI/eval path for extraction, retrieval, and context-pack quality.

**Why:** Etyma is making a local-first promise. Without a corpus, we cannot tell whether graph/retrieval changes improve agent usefulness or only make the UI look richer.

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

**Why:** Users can pick a weak local model and get a graph that feels broken even when the app works. Etyma needs recommended defaults and clear fallbacks for local-first operation.

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

## P2 - MCP read surface and controlled writes - Done

**What:** Expose Etyma's local document context through MCP after extraction
and retrieval stabilize, then add narrow mutating tools for import, graph
patches, and evidence-backed save-back flows.

**Why:** Agents must retrieve source/page/evidence-backed context, and controlled
agent workflows need explicit, auditable, evidence-scoped mutation paths instead
of generic filesystem or command access.

**Core tools:**

- `import_source`
- `import_status`
- `import_cancel`
- `import_retry_graph`
- `search_documents`
- `search_brain`
- `get_context_pack`
- `read_context_pack`
- `read_source`
- `read_page_evidence`
- `read_wiki_page`
- `read_node`
- `read_recent_events`
- `read_graph_history`
- `read_graph_snapshot`
- `read_health`
- `graph_patch_apply`
- `write_propose`
- `write_commit`
- `write_commit_all`
- `write_list`
- `write_reject`

**Acceptance:**

- [x] MCP tools return the same evidence-backed IDs as the engine/CLI.
- [x] Mutating tools are explicit, narrow, auditable, and evidence-scoped.
- [x] Tool responses include source/evidence provenance.
- [x] The contract is documented with examples.

**Effort:** M
**Priority:** P2
**Depends on:** Retrieval baseline, context-pack hardening, stable context pack schema

## P3 - Context pack demo and distribution proof

**What:** Make a known fixture prove the full loop from import to cited agent
answer.

**Why:** Etyma needs proof that one local ingest can be reused by Claude Code,
Codex, Cursor, or another MCP-aware agent without re-uploading documents or
losing page evidence.

**Scope:**

- Define `etyma demo` target behavior.
- Keep `etyma context` as the query-time context-pack alias.
- Keep `etyma documents search` as the document-search alias.
- Publish setup guides for at least two MCP clients.
- Add prompt-injection and hosted-provider disclosure warnings to first-run docs.
- Defer shared/team workflows until repeated cited reuse is proven.

**Acceptance:**

- [x] A fixture demo completes under 60 seconds.
- [x] A sample query writes schema-valid `context_pack.json`.
- [x] One MCP client can answer with source/page/evidence citations.
- [x] Active Codex proof reuses the same source and evidence ref through MCP.
- [x] Default docs and tool lists contain no retired trust-layer UX language.
- [x] Missing provider and partial import failures produce specific warnings.

**Effort:** L
**Priority:** P3
**Depends on:** MCP read/write surface, Source Pack/Evidence Index, Context Pack v1

---

## Completed / Existing Substrate

- [x] Local document ingestion wedge
- [x] Internal graph/wiki materialization substrate
- [x] Source metadata persistence from composer intent
- [x] Read-only document context engine API
- [x] Context readiness lint path
- [x] Structured extraction artifact
  - Added `StructuredExtractionArtifact` and nested entity/topic/claim/relation/page-ref contracts.
  - Re-exported the contract through `etyma-engine-types`.
  - Persisted per-source `artifacts/<source_id>/extraction.json`.
  - Marked the current extractor as `extractor=heuristic`.
  - Prevented evidence-free claims/relations from becoming generated graph context.
  - Covered artifact shape and source/evidence round trip in unit tests.
- [x] Golden corpus and eval harness
  - Added six checked-in benchmark fixtures under `crates/etyma-engine/tests/fixtures/brain-corpus`.
  - Added `etyma eval golden-corpus` with `--fixtures` and `--mode heuristic|hosted|local|all`.
  - Reports entity recall, claim citation coverage, relation evidence coverage, evidence snippet coverage, contradiction detection, context-pack relevance, and latency.
  - Covered the CLI eval path with an integration test.
- [x] Local retrieval baseline and context-pack hardening
  - Replaced substring-only score counting with tokenized local scoring and suffix/plural normalization.
  - Expanded context packs through matched sources, evidence, entities, claims, relations, memories, and events.
  - Added evidence IDs to search snippets for agent-readable citations.
  - Added CLI context-pack counts for memories, entities, claims, and relations.
  - Covered plural-token retrieval and graph/evidence expansion with a regression test.
- [x] Context Pack v1, Context Pack v0 compatibility, Source Pack, and Evidence Index artifacts
  - Added `schemas/context-pack-v1.schema.json`, `schemas/context-pack.schema.json`, `schemas/source-pack.schema.json`, and `schemas/evidence-index.schema.json`.
  - Writes `source_pack.json` and `evidence_index.json` during output packaging.
  - Persists query-time Context Pack v1 as `context_pack.json` and history files when requested.
  - Returns `contextPackV0` for compatibility with older agent clients.
  - Covers schema and artifact round trips in tests.
- [x] MCP document-context naming and root hardening
  - Added `search_documents` and `read_page_evidence`.
  - Kept `search_brain` and `etyma://brain/...` as compatibility surfaces.
  - Gated development `rootDir` behind `ETYMA_MCP_ALLOW_ROOT_DIR=1` and a canonical `ETYMA_MCP_ALLOWED_ROOTS` allowlist.
  - Rejects workspace path, prefix, and symlink escapes.
  - Keeps `read_health` read-only and adds per-source readiness fields for status, failed pages, content-hash state, and provider route.
- [x] CLI context aliases
  - Added `etyma context` / `etyma context-pack`.
  - Added `etyma documents search` / `etyma docs search`.
- [x] MCP client setup guides
  - Added Codex and Claude Code installer setup instructions.
  - Documented Cursor manual stdio configuration.
  - Added a verification prompt that requires source/page/evidence citations.
  - Recorded active Codex installed-app distribution proof in `docs/dogfood-reports/2026-06-02-codex-installed-app-distribution-proof.md`.
- [x] Save-back and correction persistence
  - Existing workspace rename, merge, and keep-separate corrections remain ledger-backed and replayable.
  - Claim updates now write durable `graph/claims.json` records.
  - Link updates now write durable `graph/edges.json` records.
  - Wiki save-back writes durable `wiki/save-back/*.md` pages.
  - Save-back records are replayed during internal graph/wiki materialization.
