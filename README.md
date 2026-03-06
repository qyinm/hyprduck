<p align="center">
  <img src="docs/site/favicon.svg" width="120" alt="DuckDocs">
</p>

<h1 align="center">DuckDocs</h1>

<p align="center">
  <strong>Capture screens. Import documents. Generate markdown.</strong>
</p>

<p align="center">
  <img src="https://img.shields.io/badge/status-active%20development-blue?style=flat-square" alt="Status">
  <img src="https://img.shields.io/badge/platform-macOS-lightgrey?style=flat-square" alt="Platform">
  <img src="https://img.shields.io/badge/language-Swift-orange?style=flat-square" alt="Language">
  <img src="https://img.shields.io/badge/license-MIT-green?style=flat-square" alt="License">
</p>

---

## Overview

DuckDocs is a macOS app for turning screenshots and imported documents into markdown using AI.

It supports two primary workflows:

1. **Auto Capture**
   Configure a target, let DuckDocs step through the screen, and convert the captured images into markdown.
2. **Document Import**
   Import a PDF or Word document, convert each page to images, and generate markdown from the page content.

DuckDocs currently uses provider-based AI processing with OpenRouter, OpenAI, Anthropic, or Ollama.

---

## Features

### Capture Workflow
- Full screen, region, or window capture
- Configurable next action between captures
- Countdown preview before capture starts
- Parallel AI processing for captured images
- Markdown output with saved images

### Document Import
- Import PDF, DOCX, and DOC
- Convert each page to an image for analysis
- Retry failed pages and save partial results
- Shared AI settings with the capture workflow

### AI Processing
- OpenRouter, OpenAI, Anthropic, and Ollama support
- Preset model lists plus custom model entry
- Prompt templates for general docs, tutorials, UI flows, code, and tables
- Local Ollama support for privacy-sensitive workflows

---

## How It Works

### Capture
1. Choose an AI provider and model.
2. Pick a capture target: full screen, region, or window.
3. Set the next action, capture count, and output name.
4. Start capture and switch to the target app during the countdown.
5. DuckDocs captures images, runs AI analysis, and saves markdown plus images to `~/Documents/DuckDocs/`.

### Import
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
- **Screen Recording**: required for screen capture
- **Accessibility**: required for simulated keyboard and mouse actions during capture
- **No special permissions are required for document import**

---

## Quick Start

### Auto Capture
1. Launch DuckDocs.
2. Configure the AI provider, model, and prompt template.
3. Choose `Full Screen`, `Region`, or `Window`.
4. Set the next action and capture count.
5. Start capture and review the generated markdown in `~/Documents/DuckDocs/`.

### Document Import
1. Launch DuckDocs.
2. Configure the AI provider and model.
3. Import a PDF, DOCX, or DOC file.
4. Wait for conversion and analysis to complete.
5. Open the generated markdown from `~/Documents/DuckDocs/`.

---

## Build

```bash
xcodebuild -project DuckDocs.xcodeproj -scheme DuckDocs
```

For local verification without code signing:

```bash
xcodebuild -project DuckDocs.xcodeproj -scheme DuckDocs CODE_SIGNING_ALLOWED=NO build
```
