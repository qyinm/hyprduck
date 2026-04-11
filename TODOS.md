# TODOS

## Knowledge Graph

### Agent-facing local context API / MCP bridge

**What:** Expose the local knowledge project as an agent-readable context API or MCP surface after the in-app graph and memory loop are stable.

**Why:** This unlocks better downstream decisions from external agents by letting them consume the same grounded concepts, evidence, and user-corrected history DuckDocs uses internally.

**Context:** The CEO review deferred this on purpose. The current wedge is still the in-app explainable knowledge graph, not external agent infrastructure. Once concept pages, evidence inspection, correction memory, and grounded answers are proven inside DuckDocs, we can freeze a cleaner external contract instead of shipping an API too early and carrying it forever.

**Effort:** M
**Priority:** P2
**Depends on:** Proven in-app graph workspace, correction memory loop, stable project schema

### Model-task matrix + latency budget

**What:** Measure and document which hosted and local models are good enough for parse, concept extraction, merge, and answer workloads, plus acceptable latency budgets for each path.

**Why:** DuckDocs is making a real local-first promise. Without a model-task matrix, users can pick a slow or weak local model and get a graph that feels broken even when the code is fine.

**Context:** The engineering review flagged local-first feasibility as a real risk. The new plan adds provenance, hybrid retrieval, concept extraction, merge logic, and grounded answers, which means model quality and latency matter more than in the original parse-only app. This should be driven by a small golden corpus and benchmark scripts so we can recommend sane defaults and decide where fallbacks are needed.

**Effort:** M
**Priority:** P1
**Depends on:** Golden corpus eval, provenance layer, hybrid retrieval baseline

## Completed
