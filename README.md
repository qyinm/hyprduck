<p align="center">
  <img src="apps/site/favicon.svg" width="120" alt="HyprDuck">
</p>

<h1 align="center">HyprDuck</h1>

<p align="center">
  <strong>Local document parsing for agent-ready knowledge.</strong>
</p>

<p align="center">
  <img src="https://img.shields.io/badge/status-active%20development-blue?style=flat-square" alt="Status">
  <img src="https://img.shields.io/badge/platform-macOS-lightgrey?style=flat-square" alt="Platform">
  <img src="https://img.shields.io/badge/runtime-Rust%20%2B%20Electron-orange?style=flat-square" alt="Runtime">
  <img src="https://img.shields.io/badge/license-MIT-green?style=flat-square" alt="License">
</p>

---

## Overview

HyprDuck turns private PDFs and Word files into reusable, cited local context
packs for AI agents.

The current wedge is local document ingestion: import PDF, DOCX, or DOC files,
preserve the original source, derive markdown and page artifacts, then compile
source packs, evidence indexes, and query-time context packs that agents can
reuse without losing page evidence.

The primary product loop is:

```text
private document
  -> source pack + evidence index
  -> query-time context pack
  -> MCP agent reads cited source/page/evidence
  -> same local context is reused in the next task
```

HyprDuck is not a generic document chatbot, a memory OS, or a governance
dashboard. The product is built around durable local artifacts, visible
provenance, and agent-readable context.

---

## What HyprDuck Builds

### Source Packs And Evidence Indexes
- Keeps imported files as immutable source records
- Stores generated page artifacts and markdown output
- Writes `source_pack.json` and `evidence_index.json` artifacts for imported sources
- Preserves source IDs, page refs, content hashes, provider route, and parse warnings

### Context Pack v0
- Builds query-shaped context packs with selected sources and evidence
- Requires findings to point back to selected evidence through `derivedFrom`
- Emits warnings for partial imports, visual loss, missing evidence, and budget truncation
- Publishes the stable schema at [`schemas/context-pack.schema.json`](schemas/context-pack.schema.json)

### Materialized Retrieval Model
- Represents sources, concepts, entities, claims, memory, and wiki pages as graph records
- Tracks evidence refs so nodes and claims stay tied to source material
- Keeps internal graph/wiki files as retrieval infrastructure, not the product surface

### Agent Context Surface
- Provides document search and context-pack contracts through the Rust engine
- Exposes an MCP stdio server for external agents: [`docs/mcp.md`](docs/mcp.md)
- Preserves provenance so agents can cite source, page, and evidence IDs

### Local-First Parsing
- Imports PDF, DOCX, and DOC files
- Converts document pages into artifacts suitable for multimodal parsing
- Uses provider-based analysis through OpenRouter or local Ollama
- Avoids Screen Recording or Accessibility permissions for the primary import flow

---

## Current Status

HyprDuck is in active development. The parser wedge, local workspace layout,
Source Pack/Evidence Index artifacts, Context Pack v0, read-only MCP server, and
document-context CLI aliases are implemented.

The next major work is proving the first agent loop end to end: import a known
fixture, generate a schema-valid context pack, connect Claude Code/Codex/Cursor,
and produce an answer with source/page/evidence citations.

---

## How It Works

1. Import a local document.
2. HyprDuck saves source metadata and derived artifacts.
3. The engine parses the document into markdown and page-level evidence.
4. HyprDuck writes a source pack and evidence index.
5. A query generates a schema-valid Context Pack v0.
6. Agents consume context packs through CLI or MCP and cite source/page/evidence refs.

---

## Workspace Artifacts

A HyprDuck workspace is designed around durable local document context files:

```text
artifacts/<source-id>/
  source_pack.json
  evidence_index.json
context_pack.json
context_packs/
brain-manifest.json
events/
  brain_events.jsonl
graph/
  nodes.json
  edges.json
  evidence.json
memory/
  records.json
wiki/
  index.md
  log.md
```

Original sources remain separate from generated artifacts.

---

## AI Providers

HyprDuck currently supports shared provider settings across parsing and
document-context workflows:

- **OpenRouter** for flexible hosted model access
- **Ollama** for local-first and privacy-sensitive workflows

Ollama does not require an API key. Source Pack and Evidence Index artifacts
record provider-route metadata; Context Pack v0 currently preserves the metadata
field and falls back to `unknown` when the source artifact does not expose an
effective route. Task-specific model guidance and latency budgets live in
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
