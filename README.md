<p align="center">
  <img src="docs/assets/etyma-disco.svg" width="120" alt="Etyma">
</p>

<h1 align="center">Etyma</h1>

<p align="center">
  <strong>Private sources compiled into agent-ready knowledge.</strong>
</p>

<p align="center">
  <img src="https://img.shields.io/badge/status-active%20development-blue?style=flat-square" alt="Status">
  <img src="https://img.shields.io/badge/platform-macOS-lightgrey?style=flat-square" alt="Platform">
  <img src="https://img.shields.io/badge/runtime-Rust%20%2B%20Electron-orange?style=flat-square" alt="Runtime">
  <img src="https://img.shields.io/badge/license-MIT-green?style=flat-square" alt="License">
</p>

---

## Overview

Etyma turns private sources into reusable, cited local context packs for AI
agents. A source is a provenance-bearing original input whose identity and
evidence trail remain intact throughout ingestion and retrieval.
The canonical definition and object boundaries are documented in
[`docs/source-model.md`](docs/source-model.md).

Documents are the first implemented source type. The current ingestion adapter
accepts PDF, DOCX, and DOC files, preserves the original source, derives
markdown and page artifacts, then compiles source packs, evidence indexes, and
query-time context packs that agents can reuse without losing page evidence.
The source model is designed to support additional input types without changing
the provenance contract.

The primary product loop is:

```text
private source
  -> source pack + evidence index
  -> query-time context pack
  -> MCP agent reads cited source/evidence refs
  -> same local context is reused in the next task
```

Etyma is not a generic file chatbot, a memory OS, or an approval
dashboard. The product is built around durable local artifacts, visible
provenance, and agent-readable context.

---

## What Etyma Builds

### Source Packs And Evidence Indexes
- Keeps imported originals as immutable source records
- Stores generated page artifacts and markdown output
- Writes `source_pack.json` and `evidence_index.json` artifacts for imported sources
- Preserves source IDs, page refs, content hashes, provider route, and parse warnings

### Context Pack v1
- Builds query-shaped context packs with selected sources and typed evidence
- Requires findings to point back to selected evidence through `derivedFrom`
- Emits warnings for partial imports, visual loss, missing evidence, and budget truncation
- Publishes the primary schema at [`schemas/context-pack-v1.schema.json`](schemas/context-pack-v1.schema.json)
- Keeps [`schemas/context-pack.schema.json`](schemas/context-pack.schema.json) as the v0 compatibility projection

### Materialized Retrieval Model
- Represents sources, concepts, entities, claims, memory, and wiki pages as graph records
- Tracks evidence refs so nodes and claims stay tied to source material
- Keeps internal graph/wiki files as retrieval infrastructure, not the product surface

### Agent Context Surface
- Provides source retrieval and context-pack contracts through the Rust engine
- Exposes an MCP stdio server for external agents: [`docs/mcp.md`](docs/mcp.md)
- Preserves provenance so agents can cite source, page, and evidence IDs

### Local-First Parsing
- Imports PDF, DOCX, and DOC files
- Converts document pages into artifacts suitable for multimodal parsing
- Uses provider-based analysis through OpenRouter or local Ollama
- Avoids Screen Recording or Accessibility permissions for the primary import flow

---

## Current Status

Etyma is in active development. The parser wedge, local workspace layout,
Source Pack/Evidence Index artifacts, Context Pack v1 with v0 compatibility,
controlled MCP import/read/write surface, and the first document ingestion
adapter are implemented.

The current demo and dogfood path proves the loop end to end: import a known
fixture, generate a schema-valid context pack, read it over MCP, and produce a
Codex answer plus follow-up with the same source/page/evidence citation. The
next major work is installed-app shell-command closeout, Agent Terminal handoff
dogfood, and a second-client proof beyond Codex.

---

## How It Works

1. Import a supported local source. The current adapter accepts PDF, DOCX, and DOC.
2. Etyma saves source metadata and derived artifacts.
3. The engine parses the document into markdown and page-level evidence.
4. Etyma writes a source pack and evidence index.
5. A query generates a schema-valid Context Pack v1.
6. Agents consume context packs through CLI or MCP and cite source/page/evidence refs.

---

## Workspace Artifacts

An Etyma workspace is designed around durable local source context:

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

Etyma currently supports shared provider settings across ingestion and
source-context workflows:

- **OpenRouter** for flexible hosted model access
- **Ollama** for local-first and privacy-sensitive workflows

Ollama does not require an API key. Source Pack and Evidence Index artifacts
record provider-route metadata; Context Pack v1 preserves the metadata field
and falls back to `unknown` when the source artifact does not expose an effective
route. Task-specific model guidance and latency budgets live in
[`docs/model-task-matrix.md`](docs/model-task-matrix.md).

---

## Requirements

- macOS 12.3+
- Apple Silicon or Intel Mac
- No special macOS permissions are required for the current document import adapter

---

## Build

Build the Electron desktop shell:

```bash
bun --cwd apps/desktop run build
```

Run the Rust workspace verification:

```bash
cargo test -p etyma-engine-types -p etyma-engine-client -p etyma-engine -p etyma-cli
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
│   ├── etyma-cli
│   ├── etyma-engine
│   ├── etyma-engine-client
│   ├── etyma-engine-types
│   └── etyma-knowledge
├── packages
├── scripts
└── release
```
