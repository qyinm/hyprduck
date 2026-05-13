<p align="center">
  <img src="apps/site/favicon.svg" width="120" alt="HyprDuck">
</p>

<h1 align="center">HyprDuck</h1>

<p align="center">
  <strong>Local-first brain repo and trust console for AI agents.</strong>
</p>

<p align="center">
  <img src="https://img.shields.io/badge/status-active%20development-blue?style=flat-square" alt="Status">
  <img src="https://img.shields.io/badge/platform-macOS-lightgrey?style=flat-square" alt="Platform">
  <img src="https://img.shields.io/badge/runtime-Rust%20%2B%20Electron-orange?style=flat-square" alt="Runtime">
  <img src="https://img.shields.io/badge/license-MIT-green?style=flat-square" alt="License">
</p>

---

## Overview

HyprDuck turns local documents into a maintained, evidence-backed brain that AI
agents can read, extend, and audit.

The current wedge is local document ingestion: import PDF, DOCX, or DOC files,
preserve the original source, derive markdown and page artifacts, then compile
the result into a source-backed knowledge base with wiki pages, graph nodes,
claims, memory records, evidence refs, review items, and maintenance logs.

The longer-term product direction is an agent-maintained personal or company
brain:

```text
local sources
  -> immutable source records
  -> extracted entities, claims, topics, and typed links
  -> maintained wiki, graph, memory, and review queue
  -> context packs agents can use without losing provenance
```

HyprDuck is not a generic document chatbot. The product is built around durable
local artifacts, visible provenance, and human-operable trust surfaces for agent
memory.

---

## What HyprDuck Builds

### Source-Backed Brain Repo
- Keeps imported files as immutable source records
- Stores generated page artifacts and markdown output
- Materializes brain artifacts under a local workspace
- Writes append-only brain events for import, proposal, review, and maintenance

### Knowledge Graph
- Represents sources, concepts, entities, claims, memory, and wiki pages as graph records
- Tracks evidence refs so nodes and claims stay tied to source material
- Supports reviewable proposals for claims, links, observations, and source notes
- Detects stale, orphaned, missing-evidence, and conflicting brain artifacts

### Agent Context Surface
- Provides brain search and context-pack contracts through the Rust engine
- Exposes an MCP stdio server for external agents: [`docs/mcp.md`](docs/mcp.md)
- Keeps proposal writes reviewable instead of letting agents mutate source truth directly
- Separates safe memory updates from risky claims and links
- Preserves provenance so agents can cite the source of durable context

### Local-First Parsing
- Imports PDF, DOCX, and DOC files
- Converts document pages into artifacts suitable for multimodal parsing
- Uses provider-based analysis through OpenRouter or local Ollama
- Avoids Screen Recording or Accessibility permissions for the primary import flow

---

## Current Status

HyprDuck is in active development. The parser wedge, local workspace layout,
brain materialization, review queue, and maintenance lint loop are implemented
as early product infrastructure.

The next major work is improving structured extraction and retrieval quality:
entities, claims, typed relations, evidence coverage, golden-corpus evaluation,
and stronger context packs before opening a broader external agent interface.

---

## How It Works

1. Import a local document.
2. HyprDuck saves source metadata and derived artifacts.
3. The engine parses the document into markdown and page-level evidence.
4. The brain compiler materializes source, node, claim, memory, wiki, graph, and event records.
5. The Knowledge workspace lets you inspect the graph, evidence, health, and review queue.
6. Agents can eventually consume context packs while HyprDuck keeps writes auditable.

---

## Workspace Artifacts

A HyprDuck brain workspace is designed around durable local files:

```text
brain-manifest.json
events/
  brain_events.jsonl
graph/
  nodes.json
  edges.json
  evidence.json
memory/
  records.json
reviews/
  proposed_updates/
wiki/
  index.md
  log.md
```

Original sources remain separate from generated and reviewable artifacts.

---

## AI Providers

HyprDuck currently focuses on:

- **OpenRouter** for flexible hosted model access
- **Ollama** for local-first and privacy-sensitive workflows

Provider settings are shared across parsing and knowledge workflows. Ollama does
not require an API key. Task-specific model guidance and latency budgets live in
[`docs/model-task-matrix.md`](docs/model-task-matrix.md).

---

## Requirements

- macOS 12.3+
- Apple Silicon or Intel Mac
- No special macOS permissions are required for document import

---

## Build

Build the Electron desktop shell:

```bash
bun --cwd apps/desktop run build
```

Run the Rust workspace verification:

```bash
cargo test -p hyprduck-engine-types -p hyprduck-engine-client -p hyprduck-engine -p hyprduck-cli
```

Stage the static site artifact locally:

```bash
just site-stage
```

---

## Repository Layout

```text
.
├── apps
│   ├── cli
│   ├── desktop
│   └── site
├── crates
│   ├── hyprduck-cli
│   ├── hyprduck-engine
│   ├── hyprduck-engine-client
│   ├── hyprduck-engine-types
│   └── hyprduck-knowledge
├── packages
├── scripts
└── release
```
