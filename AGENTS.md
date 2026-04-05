# DuckDocs - Project Knowledge Base

**Generated:** 2026-03-06
**Project:** DuckDocs - Capture and Document Import to Markdown
**Stack:** Swift, SwiftUI, macOS

---

## OVERVIEW

DuckDocs is a macOS app that captures screens or imports documents, then converts the resulting images into markdown using AI.

Current product direction:
- **Auto Capture**: full screen, region, or window capture with optional simulated next actions
- **Document Import**: PDF, DOCX, and DOC conversion into page images for markdown generation
- **AI Processing**: provider-based analysis via OpenRouter, OpenAI, Anthropic, or Ollama

Legacy recording/playback components still exist in the codebase, but the primary product surface is now capture plus import.

**Core Value Proposition:** turn visual workflows and documents into markdown quickly.

---

## PROJECT STRUCTURE

```text
DuckDocs/
├── apps/
│   ├── cli/
│   └── macos/
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
└── docs/
```

---

## WHERE TO LOOK

| Task | Location | Notes |
|------|----------|-------|
| App entry | `apps/macos/DuckDocs/App/DuckDocsApp.swift` | Root scene and commands |
| Main UI | `apps/macos/DuckDocs/Views/ContentView.swift` | Capture workflow UI |
| Document import UI | `apps/macos/DuckDocs/Views/DocumentImportSection.swift` | Import progress and actions |
| Capture workflow | `apps/macos/DuckDocs/Playback/AutoCaptureService.swift` | Capture, AI processing, save |
| Document import workflow | `apps/macos/DuckDocs/Playback/DocumentImportService.swift` | Convert, process, save |
| Document conversion | `apps/macos/DuckDocs/Playback/DocumentConverter.swift` | PDF and Word to images |
| Screen capture | `apps/macos/DuckDocs/Playback/ScreenCapture.swift` | ScreenCaptureKit wrappers |
| AI orchestration | `apps/macos/DuckDocs/AI/AIService.swift` | Provider selection and prompts |
| AI providers | `apps/macos/DuckDocs/AI/Providers/` | OpenRouter, OpenAI, Anthropic, Ollama |
| Prompt templates | `apps/macos/DuckDocs/AI/PromptTemplate.swift` | Shared capture/import prompts |
| Legacy record/playback | `apps/macos/DuckDocs/Recording/`, `apps/macos/DuckDocs/Playback/ActionPlayer.swift` | Secondary, not primary UI |

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

### Swift Style
- Prefer `async/await` for capture, conversion, and AI processing
- Use `@Observable` state containers for UI-facing services
- Keep AppKit usage isolated to capture, permissions, and image handling
- Use `NSImage` for screenshots and converted page images

### Permissions
- **Screen Recording** and **Accessibility** are capture-only requirements
- Document import should remain usable even when capture permissions are missing
- Guide users to System Settings when capture permissions are needed

### AI Architecture
- Treat AI settings as shared across capture and import
- Use provider abstractions instead of product-specific OCR services
- Keep user-facing error messages specific, especially for missing API keys or unavailable Ollama instances

### Output
- Save results under `~/Documents/DuckDocs/`
- Keep images on disk, not in long-term memory
- Generate markdown that references the saved images

---

## ANTI-PATTERNS (DO NOT)

- **DO NOT** reintroduce product messaging that describes DuckDocs as DeepSeek-only
- **DO NOT** block document import behind capture permissions
- **DO NOT** save markdown without corresponding image references
- **DO NOT** assume single-display setups when working on region capture
- **DO NOT** route main UI and menu actions through different service instances

---

## UNIQUE REQUIREMENTS

### Auto Capture
- Support full screen, region, and window capture modes
- Support simulated next actions between captures
- Show a countdown preview before hiding the app and starting capture

### Document Import
- Support PDF, DOCX, and DOC
- Convert documents into one image per page before AI processing
- Allow retrying failed pages and saving partial results

### AI Providers
- OpenRouter, OpenAI, Anthropic, and Ollama are first-class providers
- Ollama should support local usage without an API key
- Prompt templates should work across both capture and import flows

### Legacy Components
- `ActionRecorder`, `EventMonitor`, `ActionPlayer`, and `ActionSequence` remain in the repo
- Treat them as legacy or secondary unless the product direction explicitly changes back to record/playback

---

## COMMANDS

```bash
# Build
xcodebuild -project apps/macos/DuckDocs.xcodeproj -scheme DuckDocs

# Build without code signing for local verification
xcodebuild -project apps/macos/DuckDocs.xcodeproj -scheme DuckDocs CODE_SIGNING_ALLOWED=NO build

# Run tests
xcodebuild test -project apps/macos/DuckDocs.xcodeproj -scheme DuckDocs
```

---

## NOTES

### Current Scope
- Capture-driven markdown generation
- Document import to markdown
- Shared provider-based AI processing
- Local and cloud AI options via Ollama and hosted providers

### Near-Term Focus
- Output quality and markdown structure
- Permission flow cleanup
- Multi-display region capture correctness
- Unifying product messaging with the implemented app

### Testing Considerations
- Capture with and without permissions granted
- Import-only flow on a fresh machine
- Multi-display region capture
- Partial AI failure and retry flows
- Provider configuration errors, especially missing keys and local Ollama availability
