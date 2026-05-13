# Model Task Matrix

HyprDuck uses the parser as the ingest wedge, but the product needs a stable
model policy for an agent-maintained local brain. This matrix defines the
default model path for each task, the local fallback, and the latency budget that
should be tracked with the golden corpus.

## Benchmark Command

```bash
cargo run -p hyprduck-cli -- eval golden-corpus \
  --fixtures crates/hyprduck-engine/tests/fixtures/brain-corpus \
  --mode all
```

The report must keep quality and latency together:

- entity recall
- claim citation coverage
- relation evidence coverage
- evidence snippet coverage
- contradiction detection
- context-pack relevance
- average latency in milliseconds

## Task Matrix

| Task | Default path | Local fallback | Quality gate | Latency budget |
| --- | --- | --- | --- | --- |
| Page parse | OpenRouter vision model: `google/gemini-2.5-flash` or `z-ai/glm-5v-turbo` | Ollama vision model: `qwen3-vl:72b` for high quality, `qwen3-vl:8b` only for small/private docs | Markdown preserves page structure, tables, and image-backed citations | Hosted p95 under 20s/page, local p95 under 45s/page |
| Structured extraction | Heuristic extractor plus hosted verification when available | Heuristic extractor only, with golden-corpus regression checks | Supported claims and relations must carry evidence refs | p95 under 5s/source after parse |
| Entity merge | Local deterministic alias merge first, hosted review for risky merges | Deterministic merge only; keep uncertain aliases separate | No evidence-free merge becomes trusted graph state | p95 under 3s/workspace aggregate |
| Review/save-back | Human-reviewed proposal path | Same local proposal path | Every accepted write creates an event and preserves source truth | p95 under 1s/write, excluding user review time |
| Grounded answer | Retrieval/context pack over local brain; hosted answer optional | Local retrieval-only answer with citations and warnings | Answer includes evidence IDs or blocks | p95 under 2s for retrieval pack |
| Agent context pack | Local retrieval baseline | Same local path | Pack includes relevant memory, claims, entities, relations, sources, evidence, events | p95 under 2s for 8k budget |

## Local Model Limits

Local models protect privacy, but small local models are not equally good at
every task.

- `qwen3-vl:8b` is acceptable for small visual parse checks and private smoke
  tests, but it should not be treated as the default for high-recall extraction
  or merge decisions.
- OCR-specialized local models such as `glm-ocr:latest` and
  `deepseek-ocr:latest` can improve page text recovery, but they do not replace
  claim extraction, contradiction detection, or graph merge policy.
- If local output misses citations, HyprDuck should keep the artifact as
  `needs_review` instead of promoting it to trusted graph state.
- Company/project memory writes should stay on the review path until the local
  model is proven on the golden corpus.

## Settings Copy

The desktop settings surface should make the warning explicit:

- Hosted vision models are recommended for high-recall parsing and extraction.
- Ollama keeps data local, but small local models may miss tables, conflicts, or
  evidence links.
- The user should run the golden corpus before trusting a new local model for
  durable graph writes.

## Update Rule

When model defaults change, update this file and verify:

```bash
cargo run -p hyprduck-cli -- eval golden-corpus \
  --fixtures crates/hyprduck-engine/tests/fixtures/brain-corpus \
  --mode all
pnpm --dir apps/desktop build
```
