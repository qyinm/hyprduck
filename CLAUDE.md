# DuckDocs - Context for AI Assistants

**Project:** DuckDocs
**Type:** macOS Desktop App (Tauri + Rust core)
**Domain:** File Parsing + AI Markdown Generation

---

## What This Project Does

DuckDocs converts existing files into markdown packages:

1. **Import**: Choose a PDF, DOCX, or DOC file.
2. **Render**: Convert each page into an image snapshot for multimodal analysis.
3. **Generate**: Send page images to the configured AI provider and assemble markdown output.
4. **Save**: Write markdown plus linked page images to `~/Documents/DuckDocs/`.

The product surface is file parsing first. The active desktop shell is `apps/desktop`.

---

## When Working on This Project

### If Adding File Parsing Features
- Look at: `apps/desktop/src-tauri/src/` and `crates/duckdocs-engine/`
- Key files: `main.rs`, `duckdocs-engine`, `duckdocs-engine-client`
- Must handle: sidecar orchestration, config migration, output ownership

### If Adding AI Features
- Look at: `crates/duckdocs-engine/src/main.rs`
- Must handle: provider routing, model configuration, multimodal prompts

### If Working on UI
- Look at: `apps/desktop/src/`
- Key files: `main.js`, `styles.css`, `index.html`
- Pattern: Tauri windows with a shared backend store

## Common Tasks & Where to Start

| Task | Entry Point | Notes |
|------|-------------|-------|
| Change AI model | `crates/duckdocs-engine/src/main.rs` | Update provider/model defaults and config payloads |
| Change prompt behavior | `crates/duckdocs-engine/src/main.rs` | Keep prompt template IDs aligned with config options |
| Modify import logic | `apps/desktop/src-tauri/src/main.rs` | Sidecar launch, parse lifecycle, window sync |
| Add file format support | `crates/duckdocs-engine/src/main.rs` | Extend conversion/parsing pipeline |
| Change output format | `crates/duckdocs-engine/src/main.rs` | Output folder and markdown package assembly |
| Change main UI | `apps/desktop/src/main.js` | File parsing surface |

---

## Data Flow

```text
User selects file in Tauri shell
    ↓
apps/desktop backend starts duckdocs-engine sidecar
    ↓
duckdocs-engine converts/parses the document
    ↓
Progress events stream over stderr
    ↓
Final result returns over stdout
    ↓
The engine writes markdown + linked assets
```

---

## Key Files

| File | Purpose |
|------|---------|
| `apps/desktop/src-tauri/src/main.rs` | Tauri desktop shell, window management, legacy-config migration |
| `apps/desktop/src/main.js` | Main/settings/progress/result window UI |
| `crates/duckdocs-engine/src/main.rs` | Conversion, provider execution, output package assembly |
| `crates/duckdocs-engine-client/src/lib.rs` | Shared subprocess client contract |
| `crates/duckdocs-engine-types/src/lib.rs` | Engine request/response/event schema |

---

## Important Considerations

### Security & Privacy
- API keys are stored locally
- Imported page images may be sent to external AI providers
- Output is saved locally in `~/Documents/DuckDocs/`

### Performance
- Parallel page processing is the main latency lever
- Images are resized/compressed before provider upload
- Large documents should fail partially rather than losing all progress

### Error Scenarios to Handle
- API key missing or invalid
- Provider/network failure during processing
- Unsupported or unreadable file contents
- Empty documents
- Partial page conversion or analysis failure

---

## Testing Checklist

- [ ] Test Tauri main-window import
- [ ] Test PDF import
- [ ] Test DOCX import
- [ ] Test DOC import
- [ ] Test partial AI failure and retry flow
- [ ] Test without API key
- [ ] Test Ollama local configuration

---

## Resources

- [PDFKit Documentation](https://developer.apple.com/documentation/pdfkit)
- [OpenRouter API](https://openrouter.ai/docs)
- [OpenAI API](https://platform.openai.com/docs)
- [Anthropic API](https://docs.anthropic.com/)

---

*This file is optimized for AI assistants.*
