<p align="center">
  <img src="apps/site/favicon.svg" width="120" alt="DuckDocs">
</p>

<h1 align="center">DuckDocs</h1>

<p align="center">
  <strong>Parse documents. Generate markdown. Keep the linked page images.</strong>
</p>

<p align="center">
  <img src="https://img.shields.io/badge/status-active%20development-blue?style=flat-square" alt="Status">
  <img src="https://img.shields.io/badge/platform-macOS-lightgrey?style=flat-square" alt="Platform">
  <img src="https://img.shields.io/badge/runtime-Rust%20%2B%20Tauri-orange?style=flat-square" alt="Runtime">
  <img src="https://img.shields.io/badge/license-MIT-green?style=flat-square" alt="License">
</p>

---

## Overview

DuckDocs is a macOS app for turning existing files into markdown using AI.

The active desktop shell lives in `apps/desktop` as a Tauri app.

The current product surface is file parsing:

1. **Import a file**
   Choose a PDF, DOCX, or DOC file from disk.
2. **Convert pages**
   DuckDocs renders each page as an image so multimodal models can analyze layout and content.
3. **Generate markdown**
   The selected provider extracts text and structure, then saves markdown plus linked page images.

DuckDocs currently uses provider-based AI processing with OpenRouter, OpenAI, Anthropic, or Ollama.

---

## Features

### File Parsing
- Import PDF, DOCX, and DOC
- Convert each page to an image for analysis
- Retry failed pages and save partial results
- Save markdown together with linked page images

### AI Processing
- OpenRouter, OpenAI, Anthropic, and Ollama support
- Preset model lists plus custom model entry
- Prompt templates for general docs, tutorials, UI flows, code, and tables
- Local Ollama support for privacy-sensitive workflows

---

## How It Works

### Parse a File
1. Choose an AI provider and model.
2. Import a PDF or Word document.
3. DuckDocs converts pages into images.
4. Each page is analyzed and assembled into markdown.
5. Results are saved to `~/Documents/DuckDocs/`.

---

## AI Providers

### OpenRouter
- Good default for flexible model choice
- Supports a wide range of multimodal models through one API key

### OpenAI
- Direct access to GPT multimodal models

### Anthropic
- Direct access to Claude multimodal models

### Ollama
- Local or cloud-backed usage
- Useful for privacy-first or offline-adjacent workflows

---

## Requirements

### System
- macOS 12.3+
- Apple Silicon or Intel Mac

### Permissions
- **No special permissions are required for file parsing**

---

## Quick Start

### Parse a Document
1. Launch DuckDocs.
2. Configure the AI provider and model.
3. Import a PDF, DOCX, or DOC file.
4. Wait for conversion and analysis to complete.
5. Open the generated markdown from `~/Documents/DuckDocs/`.

---

## Build

Preferred desktop build:

```bash
pnpm --dir apps/desktop build
```

Rust workspace verification:

```bash
cargo test -p duckdocs-engine-types -p duckdocs-engine-client -p duckdocs-engine -p duckdocs-cli
```

Type-check the Tauri desktop shell:

```bash
cargo check -p duckdocs-desktop
```

Stage the static site artifact locally:

```bash
just site-stage
```

## Repository Layout

```text
.
├── apps
│   ├── cli
│   ├── desktop
│   └── site
├── packages
├── scripts
└── release
```
