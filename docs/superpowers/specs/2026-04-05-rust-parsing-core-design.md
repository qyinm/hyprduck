# Rust Parsing Core Design

**Date:** 2026-04-05
**Status:** Draft approved in chat, awaiting file review
**Scope:** Unify document parsing behind a single Rust core shared by the macOS app and CLI

## Goal

Refactor DuckDocs so the macOS app and CLI act as thin interface layers while a single Rust parsing core owns document conversion, provider orchestration, parsing execution, output generation, and persistent configuration.

## Problem

The current macOS app already contains a working import-first parsing flow in Swift, but the longer-term product direction requires:

- one parsing implementation shared by every interface
- support for local AI and multiple hosted providers
- a clean contract boundary between UI and parsing logic
- a path to validate parsing behavior outside the app

If parsing remains embedded inside interface-specific code, provider support, reliability work, and future integrations will diverge quickly.

## Non-Goals

- building a long-lived daemon or service runtime
- keeping provider-specific logic in Swift or app-specific layers
- adding new UI features before the engine boundary is complete
- introducing separate parsing implementations per interface

## Product Decision Summary

- The macOS app and CLI are thin interfaces only.
- A single Rust core is the source of truth for parsing behavior.
- The Rust core owns provider interfaces and concrete implementations.
- The Rust core handles document input end-to-end: `PDF`, `DOCX`, and `DOC` to page images to parsed markdown.
- The macOS app and CLI call the Rust core as a subprocess.
- Primary transport is `stdin/stdout` JSON.
- The same executable also supports debug-oriented request/result file output.
- Persistent provider settings live in the Rust core and are stored in the user's home directory.

## Architecture

### Top-Level Shape

The system is composed of three layers:

1. Interface layer
   - `apps/macos`
   - `apps/cli`
   - Responsibilities: file picking, progress presentation, user commands, subprocess lifecycle, result rendering

2. Contract layer
   - shared schema crate defining requests, results, events, and errors
   - Responsibilities: stable JSON I/O contract for all interfaces and the core

3. Parsing core layer
   - Rust executable with internally separated modules
   - Responsibilities: configuration, document conversion, provider selection, page parsing, output assembly, structured progress and error reporting

### Recommended Repository Layout

```text
crates/
  duckdocs-schema/
    src/
      request.rs
      response.rs
      event.rs
      error.rs

  duckdocs-core/
    src/
      main.rs
      config/
      conversion/
      parsing/
      output/
      runtime/
      providers/

apps/
  cli/
  macos/
```

`duckdocs-providers` can start as an internal `duckdocs-core/providers` module unless it becomes large enough to justify extraction into its own crate. The important decision is the boundary, not the initial packaging.

## Interface Contracts

### Request Contract

Both interfaces send the same schema-shaped request to the core:

- input file path
- input format
- prompt template or parsing template identifier
- parse options
- output target name and directory hints
- optional execution flags such as debug output

The request contract must stay interface-neutral. It must not contain AppKit, SwiftUI, or CLI-only concepts.

### Result Contract

The core returns a single final JSON result over `stdout` that includes:

- aggregate markdown output
- per-page parsed output
- output asset metadata
- engine/provider metadata
- counts for succeeded and failed pages
- structured error details when applicable

Partial success is a first-class result state. The core must preserve successful pages even if some pages fail.

### Event Contract

Progress events are emitted as JSON lines over `stderr`.

Expected event families:

- `document_opened`
- `page_rendered`
- `page_parse_started`
- `page_parse_completed`
- `page_parse_failed`
- `package_saved`

This allows:

- the macOS app to drive progress UI and retry state
- the CLI to print readable progress logs

### Debug File Mode

The same executable supports an alternate debug path:

- optional request file capture
- optional result file capture

This is for inspection and reproducibility only. It does not replace the primary `stdin/stdout` contract.

## Parsing Core Responsibilities

The Rust core owns the full parsing pipeline:

1. load and validate request
2. load provider configuration
3. convert the input document into page images
4. select the configured provider and model
5. parse each page
6. assemble markdown and assets
7. save outputs
8. emit final result

The core must not depend on interface-layer persistence or UI logic.

## Provider Runtime

### Ownership

The Rust core owns:

- common provider trait/interface
- provider capability normalization
- concrete implementations for `OpenAI`, `OpenRouter`, `Anthropic`, and `Ollama`
- provider-specific request/response adaptation
- provider-specific error mapping

### Why This Boundary

This keeps provider expansion independent from interface work. Adding or fixing a provider becomes a core change rather than a macOS-specific or CLI-specific change.

### Local AI

`Ollama` is treated as a first-class provider, not a special-case fallback. The core must support local execution without requiring hosted-provider configuration.

## Document Conversion

The Rust core is responsible for end-to-end file handling for:

- `PDF`
- `DOCX`
- `DOC`

The conversion stage produces one image per page before parsing. Interface layers should never need to know how conversion happens internally.

This requirement matters because the app and CLI must share identical document preparation behavior. If conversion remains outside the core, shared parsing behavior breaks immediately.

## Configuration Model

### Source of Truth

The Rust core is the owner of persistent parsing configuration.

The configuration store includes:

- selected provider
- selected model
- API keys or local endpoint configuration
- parsing defaults
- optional provider-specific settings

### Storage Location

Configuration is stored in the user's home directory as application-owned persistent state.

The initial design assumes:

- one user-level configuration
- no project-local overrides in the first version

This keeps behavior predictable across the macOS app and CLI.

### Security Note

The first version optimizes for a single source of truth. If platform-specific secret storage becomes necessary later, it can be added as an adapter layer without changing the interface contract.

## Error Model

The core returns structured error codes rather than only free-form messages.

Examples:

- `invalid_api_key`
- `provider_unreachable`
- `unsupported_format`
- `conversion_failed`
- `partial_page_failures`
- `configuration_missing`

Why this matters:

- the macOS app can present user-friendly UI messages
- the CLI can map them to logs and exit behavior
- tests can assert stable behavior

## Cancellation Model

For the first version, cancellation is subprocess termination by the interface layer.

This keeps the contract simple:

- the macOS app cancels by killing the child process
- the CLI cancels by process interruption

More granular cooperative cancellation can be added later if needed, but it is not required to establish the correct architecture.

## Migration Plan

### Phase 1: Shared Schema

Create the Rust schema contract for request, result, event, and error types.

Outcome:

- one canonical wire contract
- stable substrate for CLI and macOS integration

### Phase 2: Core Execution Path

Build `duckdocs-core` so it can:

- read a request
- convert a document
- run provider parsing
- save outputs
- emit final structured results

Outcome:

- the parser can be verified without any app integration

### Phase 3: CLI First Integration

Integrate the CLI against the core before the macOS app.

Why:

- the CLI is the fastest validation surface
- subprocess behavior and schema drift are easier to debug there first

### Phase 4: macOS Adapter Swap

Replace the current `AIServiceParsingEngine` Swift-backed parse path with a Rust subprocess adapter.

Outcome:

- the macOS app remains the same product surface
- parsing ownership moves out of Swift

### Phase 5: Legacy Isolation and Cleanup

After the Rust path is verified:

- remove or isolate the legacy Swift document conversion and provider execution path
- keep only the interface concerns in the app

## Testing Strategy

### Core Unit Tests

- request validation
- configuration loading
- provider selection
- structured error mapping

### Core Integration Tests

- `PDF` end-to-end conversion and parse result creation
- `DOCX` end-to-end conversion and parse result creation
- `DOC` end-to-end conversion and parse result creation
- partial page failure handling

### Contract Tests

- JSON request/response round-trip stability
- event emission format stability

### CLI End-to-End Tests

- subprocess invocation
- progress event handling
- final result consumption

### macOS Verification

- file selection triggers core request creation
- progress UI reflects subprocess events
- cancel terminates the subprocess cleanly
- partial success still allows save and open flows

## Risks

### Word Conversion Portability

`DOC` and `DOCX` handling may be more brittle than `PDF` depending on the conversion strategy chosen in Rust.

Mitigation:

- prove `PDF` and one Word path quickly
- keep conversion isolated behind a clear module boundary

### Provider Response Normalization

Hosted APIs and local AI runtimes will differ in response shape, limits, and error semantics.

Mitigation:

- centralize provider adaptation
- normalize into one parsed-page contract before the rest of the pipeline sees the result

### Settings Migration

The current macOS app already contains Swift-side AI settings and provider knowledge.

Mitigation:

- move slowly
- preserve existing UX while changing only the execution backend first

## Alternatives Considered

### Keep Parsing in Swift and Mirror It in CLI

Rejected because it creates duplicated behavior and makes provider growth harder.

### Use a Rust Library Directly from Swift

Rejected for the first version because FFI complexity is not justified before the core contract is proven.

### Run a Long-Lived Parsing Daemon

Rejected because it adds operational complexity before the shared parser boundary is even stabilized.

## Success Criteria

The design is successful when all of the following are true:

- one Rust executable performs document parsing end-to-end
- the CLI and macOS app both use the same core and same JSON contract
- provider support is implemented only once in the Rust core
- parsing behavior can be tested independently from the UI
- the macOS app remains an import-first experience but no longer owns parsing behavior

## Open Decisions Closed in This Spec

- Rust core is the single parsing source of truth
- macOS and CLI are thin interfaces
- provider implementations live in Rust
- `PDF`, `DOCX`, and `DOC` conversion are handled by Rust
- subprocess integration is the first delivery model
- transport is `stdin/stdout` JSON with optional debug file output
- configuration lives in the user's home directory under Rust-core ownership
