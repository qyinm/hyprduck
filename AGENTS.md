# DuckDocs - Project Knowledge Base

**Generated:** 2026-03-06
**Project:** DuckDocs - File Parsing to Markdown
**Stack:** Rust, Tauri, JavaScript, macOS

---

## OVERVIEW

DuckDocs is a macOS app that imports documents, converts each page into an image, and turns the result into markdown using AI.

Current product direction:
- **File Parsing**: PDF, DOCX, and DOC conversion into page images for markdown generation
- **AI Processing**: provider-based analysis via OpenRouter, OpenAI, Anthropic, or Ollama

The active desktop shell is `apps/desktop` (Tauri). Legacy Swift capture and recording/playback components still exist in the codebase as migration/reference material, but the primary product surface is import-first file parsing through the Rust engine.

**Core Value Proposition:** turn existing files into markdown packages quickly.

---

## PROJECT STRUCTURE

```text
DuckDocs/
├── apps/
│   ├── cli/
│   ├── desktop/
│   │   ├── package.json
│   │   ├── src/
│   │   └── src-tauri/
│   ├── macos/
│       ├── DuckDocs.xcodeproj
│       └── DuckDocs/
│           ├── App/
│           │   ├── DuckDocsApp.swift
│           │   ├── AppState.swift
│           │   ├── KeyboardShortcutManager.swift
│           │   └── PermissionManager.swift
│           ├── AI/
│           │   ├── AIService.swift
│           │   ├── AIProvider.swift
│           │   ├── MarkdownGenerator.swift
│           │   ├── KeychainService.swift
│           │   ├── PromptTemplate.swift
│           │   ├── Models/
│           │   └── Providers/
│           ├── Models/
│           │   ├── CaptureJob.swift
│           │   ├── CaptureResult.swift
│           │   ├── DocumentImportJob.swift
│           │   └── ActionSequence.swift
│           ├── Playback/
│           │   ├── AutoCaptureService.swift
│           │   ├── DocumentImportService.swift
│           │   ├── DocumentConverter.swift
│           │   ├── ScreenCapture.swift
│           │   └── ActionPlayer.swift
│           ├── Recording/
│           │   ├── ActionRecorder.swift
│           │   └── EventMonitor.swift
│           └── Views/
│               ├── ContentView.swift
│               ├── DocumentImportSection.swift
│               ├── OnboardingView.swift
│               ├── QuickEntryWindow.swift
│               ├── RegionSelectorWindow.swift
│               ├── CapturePreviewWindow.swift
│               └── WindowPickerView.swift
│   └── site/
├── packages/
└── scripts/
```

---

## WHERE TO LOOK

| Task | Location | Notes |
|------|----------|-------|
| Desktop app entry | `apps/desktop/src-tauri/src/main.rs` | Active Tauri desktop shell |
| Desktop UI | `apps/desktop/src/main.js` | Main/settings/progress/result windows |
| Desktop styling | `apps/desktop/src/styles.css` | Active desktop visual surface |
| Engine contract | `crates/duckdocs-engine-types/src/lib.rs` | Shared request/response/event schema |
| Engine runtime | `crates/duckdocs-engine/src/main.rs` | Conversion, provider execution, output writing |
| Engine client | `crates/duckdocs-engine-client/src/lib.rs` | Shared subprocess bridge |
| Legacy Swift reference UI | `apps/macos/DuckDocs/Views/ContentView.swift` | Reference only during migration |
| Legacy Swift import flow | `apps/macos/DuckDocs/Playback/DocumentImportService.swift` | Reference only during migration |

---

## DATA MODELS

```swift
struct CaptureJob {
    var captureMode: CaptureMode
    var nextAction: NextAction
    var captureCount: Int
    var delayBetweenCaptures: TimeInterval
    var outputName: String
}

struct DocumentImportJob {
    let fileURL: URL
    let format: DocumentFormat
    var outputName: String
}

struct ImageProcessingResult {
    let id: Int
    let image: NSImage
    var status: Status
    var analysis: String?
    var errorMessage: String?
}
```

---

## CONVENTIONS

### Desktop Shell Style
- Keep the active desktop shell in `apps/desktop`
- Treat `apps/macos` as legacy reference unless the task explicitly says otherwise
- Keep shell logic in Tauri and parsing/output logic in the Rust engine
- Avoid reintroducing frontend-owned output packaging or provider persistence

### Permissions
- File parsing must remain usable without Screen Recording or Accessibility permissions
- Screen Recording and Accessibility apply only to optional legacy capture code
- Do not surface capture-permission friction in the primary import flow

### AI Architecture
- Treat AI settings as shared across file parsing flows
- Use provider abstractions instead of product-specific OCR services
- Keep user-facing error messages specific, especially for missing API keys or unavailable Ollama instances

### Output
- Save results under `~/Documents/DuckDocs/`
- Keep images on disk, not in long-term memory
- Generate markdown that references the saved images

---

## ANTI-PATTERNS (DO NOT)

- **DO NOT** reintroduce product messaging that describes DuckDocs as DeepSeek-only
- **DO NOT** block file parsing behind capture permissions
- **DO NOT** save markdown without corresponding image references
- **DO NOT** let legacy capture terminology leak into the primary app surface
- **DO NOT** route main UI and menu actions through different service instances

---

## UNIQUE REQUIREMENTS

### File Parsing
- Support PDF, DOCX, and DOC
- Convert documents into one image per page before AI processing
- Allow retrying failed pages and saving partial results

### AI Providers
- OpenRouter, OpenAI, Anthropic, and Ollama are first-class providers
- Ollama should support local usage without an API key
- Prompt templates should support general document parsing, tutorials, UI flows, code, and tables

### Legacy Components
- `AutoCaptureService`, `ScreenCapture`, `ActionRecorder`, `EventMonitor`, `ActionPlayer`, and `ActionSequence` remain in the repo
- Treat them as legacy or secondary unless the product direction explicitly changes back to capture/playback

---

## COMMANDS

```bash
# Build active desktop app
pnpm --dir apps/desktop build

# Verify desktop shell types
cargo check -p duckdocs-desktop

# Run core tests
cargo test -p duckdocs-engine-types -p duckdocs-engine-client -p duckdocs-engine -p duckdocs-cli
```

---

## NOTES

### Current Scope
- File parsing to markdown
- Shared provider-based AI processing
- Local and cloud AI options via Ollama and hosted providers

### Near-Term Focus
- Output quality and markdown structure
- Import UX clarity
- Retry and partial-save reliability
- Unifying product messaging with the implemented app

### Testing Considerations
- Import-only flow on a fresh machine
- Partial AI failure and retry flows
- Provider configuration errors, especially missing keys and local Ollama availability
