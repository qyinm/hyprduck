# DuckDocs - Context for AI Assistants

**Project:** DuckDocs
**Type:** macOS Native App (Swift/SwiftUI)
**Domain:** File Parsing + AI Markdown Generation

---

## What This Project Does

DuckDocs converts existing files into markdown packages:

1. **Import**: Choose a PDF, DOCX, or DOC file.
2. **Render**: Convert each page into an image snapshot for multimodal analysis.
3. **Generate**: Send page images to the configured AI provider and assemble markdown output.
4. **Save**: Write markdown plus linked page images to `~/Documents/DuckDocs/`.

The product surface is file parsing first. Legacy capture code still exists in the repository, but it is not the primary app flow.

---

## When Working on This Project

### If Adding File Parsing Features
- Look at: `apps/macos/DuckDocs/Playback/`
- Key files: `DocumentImportService.swift`, `DocumentConverter.swift`, `DocumentationOutputBuilder.swift`
- Must handle: per-page conversion, partial failures, markdown assembly

### If Adding AI Features
- Look at: `apps/macos/DuckDocs/AI/`
- Key files: `AIService.swift`, `PromptTemplate.swift`, `Providers/`
- Must handle: provider routing, model configuration, multimodal prompts

### If Working on UI
- Look at: `apps/macos/DuckDocs/Views/`
- Key files: `ContentView.swift`, `DocumentImportSection.swift`
- Pattern: SwiftUI with `@Observable`

### If Touching Legacy Capture Code
- Look at: `apps/macos/DuckDocs/Playback/AutoCaptureService.swift`
- Related files: `ScreenCapture.swift`, `RegionSelectorWindow.swift`, `WindowPickerView.swift`
- Treat this as secondary unless the product direction changes again

---

## Common Tasks & Where to Start

| Task | Entry Point | Notes |
|------|-------------|-------|
| Change AI model | `apps/macos/DuckDocs/AI/AIService.swift` | Update provider/model defaults |
| Change prompt behavior | `apps/macos/DuckDocs/AI/PromptTemplate.swift` | Adjust parsing-oriented prompt text |
| Modify import logic | `apps/macos/DuckDocs/Playback/DocumentImportService.swift` | Conversion, analysis, retry flow |
| Add file format support | `apps/macos/DuckDocs/Playback/DocumentConverter.swift` | Extend conversion pipeline |
| Change output format | `apps/macos/DuckDocs/Playback/DocumentationOutputBuilder.swift` | Output folder and markdown assembly |
| Change main UI | `apps/macos/DuckDocs/Views/ContentView.swift` | File parsing surface |

---

## Data Flow

```text
User selects file
    ↓
DocumentImportJob
    ↓
DocumentConverter renders page images
    ↓
AIService sends page images to provider
    ↓
Results are collected in page order
    ↓
DocumentationOutputBuilder writes markdown + images
```

---

## Key Files

| File | Purpose |
|------|---------|
| `apps/macos/DuckDocs/Playback/DocumentImportService.swift` | Main file parsing workflow orchestrator |
| `apps/macos/DuckDocs/Playback/DocumentConverter.swift` | PDF and Word conversion into page images |
| `apps/macos/DuckDocs/Playback/DocumentationOutputBuilder.swift` | Markdown package assembly |
| `apps/macos/DuckDocs/Models/DocumentImportJob.swift` | File parsing job configuration |
| `apps/macos/DuckDocs/AI/AIService.swift` | AI orchestration and provider routing |
| `apps/macos/DuckDocs/Views/ContentView.swift` | Main import-first UI |
| `apps/macos/DuckDocs/Views/DocumentImportSection.swift` | Import state and controls |

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
