# HyprDuck - Context for AI Assistants

**Project:** HyprDuck
**Type:** macOS Desktop App (Electron + Rust core)
**Domain:** File Parsing + AI Markdown Generation

---

## What This Project Does

HyprDuck imports existing files into a local agent-ready brain:

1. **Import**: Choose a PDF, DOCX, or DOC file.
2. **Parse**: Extract markdown with the Rust engine, using fast local parsing where available.
3. **Structure**: Compile markdown into source-backed graph/wiki/claim artifacts.
4. **Save**: Write durable brain artifacts under the HyprDuck application data directory.

The product surface is file parsing first. The active desktop shell is `apps/desktop`.

---

## When Working on This Project

### If Adding File Parsing Features
- Look at: `apps/desktop/main.cjs`, `apps/desktop/preload.cjs`, and `crates/hyprduck-engine/`
- Key files: `main.cjs`, `preload.cjs`, `hyprduck-engine`, `hyprduck-engine-client`
- Must handle: engine runtime orchestration, config migration, output ownership

### If Adding AI Features
- Look at: `crates/hyprduck-engine/src/main.rs`
- Must handle: provider routing, model configuration, multimodal prompts

### If Working on UI
- Look at: `apps/desktop/src/`
- Key files: `App.tsx`, `styles.css`, `index.html`
- Pattern: Electron window with a preload bridge and shared backend store

## Common Tasks & Where to Start

| Task | Entry Point | Notes |
|------|-------------|-------|
| Change AI model | `crates/hyprduck-engine/src/main.rs` | Update provider/model defaults and config payloads |
| Change prompt behavior | `crates/hyprduck-engine/src/main.rs` | Keep prompt template IDs aligned with config options |
| Modify import logic | `apps/desktop/main.cjs` | Engine launch, parse lifecycle, window sync |
| Add file format support | `crates/hyprduck-engine/src/main.rs` | Extend conversion/parsing pipeline |
| Change output format | `crates/hyprduck-engine/src/main.rs` | Output folder and markdown package assembly |
| Change main UI | `apps/desktop/src/App.tsx` | File parsing surface |

---

## Data Flow

```text
User selects file in Electron shell
    ↓
apps/desktop backend starts hyprduck-engine runtime
    ↓
hyprduck-engine converts/parses the document
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
| `apps/desktop/main.cjs` | Electron desktop shell, window management, legacy-config migration |
| `apps/desktop/preload.cjs` | Safe renderer bridge for Electron IPC |
| `apps/desktop/src/App.tsx` | Main/settings/progress/result window UI |
| `crates/hyprduck-engine/src/main.rs` | Conversion, provider execution, output package assembly |
| `crates/hyprduck-engine-client/src/lib.rs` | Shared subprocess client contract |
| `crates/hyprduck-engine-types/src/lib.rs` | Engine request/response/event schema |

---

## Important Considerations

### Security & Privacy
- API keys are stored locally
- Imported page images may be sent to external AI providers
- Output is saved locally in `~/Documents/HyprDuck/`

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

- [ ] Test Electron main-window import
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
