# HyprDuck Architecture

HyprDuck is a local-first document ingestion and agent-readable knowledge
workspace. The product starts with file parsing, but the architecture is built
around durable source records, materialized graph/wiki state, reviewable agent
updates, and local interfaces that other agents can consume.

This document describes the architecture that exists in the repository today.
It intentionally separates implemented behavior from product direction.

## System Goals

HyprDuck is designed around five constraints:

1. **Import must be local and permission-light.** PDF, DOCX, and DOC import must
   work without Screen Recording or Accessibility permissions.
2. **Original source truth must be preserved.** Imported source files and
   generated markdown/page artifacts are stored as durable local records.
3. **Derived knowledge must stay auditable.** Nodes, claims, relations, wiki
   pages, memories, and review items are materialized from explicit events and
   provider outputs, not hidden UI state.
4. **Agents should read structured context, not scrape the UI.** The Rust engine
   and MCP server expose search, context packs, graph snapshots, source reads,
   wiki reads, and policy-controlled proposals.
5. **Provider output is a candidate, not source truth.** Provider-generated graph
   state is parsed, normalized, validated, written with run artifacts, and
   replayed through the materialized brain repository.

## Repository Map

```text
HyprDuck/
├── apps/
│   ├── desktop/                         # Active Electron desktop shell
│   │   ├── main.cjs                     # Main process, IPC, engine runtime queue
│   │   ├── preload.cjs                  # Safe renderer bridge
│   │   ├── src/App.tsx                  # App shell, import, settings, history, graph workspace
│   │   └── src/features/workspace/      # Materialized graph UI adapter and canvas
│   └── site/                            # Public Astro site / web preview assets
├── crates/
│   ├── hyprduck-engine                  # Core Rust runtime and domain logic
│   ├── hyprduck-engine-client           # Subprocess client used by CLI/MCP-style callers
│   ├── hyprduck-engine-types            # Shared command, response, graph, brain schemas
│   ├── hyprduck-cli                     # CLI, TUI, MCP server, eval harness
│   └── hyprduck-knowledge               # Shared knowledge/brain record types
├── docs/                                # Architecture, MCP, model task matrix, graph docs
├── schemas/                             # JSON schema contracts for external consumers
└── scripts/                             # Engine sync/build helpers for Electron
```

## High-Level Architecture

```mermaid
flowchart TB
  User["User"]
  Agent["External agent / MCP client"]

  subgraph Desktop["apps/desktop Electron"]
    Renderer["React renderer\nApp.tsx + workspace UI"]
    Preload["preload.cjs\ncontext-isolated bridge"]
    Main["main.cjs\nIPC handlers + engine runtime"]
  end

  subgraph Rust["Rust crates"]
    EngineTypes["hyprduck-engine-types\nwire contracts"]
    EngineClient["hyprduck-engine-client\none-shot subprocess client"]
    CLI["hyprduck-cli\nCLI, TUI, MCP server, eval"]
    Engine["hyprduck-engine\nruntime + domain orchestration"]
    Knowledge["hyprduck-knowledge\nbrain record types"]
  end

  subgraph EngineDomains["hyprduck-engine domains"]
    Ingest["ingest\nparse + output package"]
    Provider["provider\nconfig + OpenAI-compatible calls"]
    Retrieval["retrieval\nsource index + import context"]
    KnowledgeDomain["knowledge\ncompile, aggregate, corrections, answers"]
    Brain["brain\nreader, writer, replay, review proposals"]
    AgentWorkflow["agent_workflow\nprovider graph materialization"]
  end

  subgraph LocalFiles["Local files"]
    AppSupport["~/Library/Application Support/HyprDuck\nworkspace outputs"]
    UserConfig["~/.hyprduck/engine-config.json\nprovider config"]
    SQLite["knowledge.sqlite3\nproject/source/correction index"]
    BrainRepo["workspace brain repo\nevents, graph, wiki, memory, runs"]
  end

  User --> Renderer
  Renderer --> Preload --> Main
  Main -->|"stdin/stdout runtime envelopes"| Engine
  CLI --> EngineClient -->|"one-shot stdin/stdout"| Engine
  Agent --> CLI
  CLI -->|"MCP stdio"| Agent

  Engine --> EngineTypes
  Engine --> Knowledge
  Engine --> Ingest
  Engine --> Provider
  Engine --> Retrieval
  Engine --> KnowledgeDomain
  Engine --> Brain
  Engine --> AgentWorkflow

  Ingest --> AppSupport
  KnowledgeDomain --> SQLite
  KnowledgeDomain --> BrainRepo
  Brain --> BrainRepo
  AgentWorkflow --> BrainRepo
  Provider --> UserConfig
```

There are two primary execution surfaces:

- **Electron runtime mode:** `apps/desktop/main.cjs` starts
  `hyprduck-engine serve`, keeps the subprocess alive, sends request envelopes
  with UUIDv7 IDs, and receives command responses plus parse progress events.
- **CLI one-shot mode:** `hyprduck-cli` and `hyprduck-engine-client` spawn the
  engine per command, send a single JSON request on stdin, and read one JSON
  response from stdout.

The engine itself owns the domain contracts. The UI should not package output,
persist provider settings, or mutate the graph directly.

## Desktop Runtime

The active desktop shell is Electron under `apps/desktop`.

```mermaid
sequenceDiagram
  autonumber
  participant User
  participant React as React renderer
  participant Preload as preload.cjs
  participant Main as main.cjs
  participant Engine as hyprduck-engine serve

  User->>React: Choose file / start import
  React->>Preload: window.hyprduck.invoke("start_parse")
  Preload->>Main: IPC hyprduck:invoke
  Main->>Main: Build ParseRequest with workspace default
  Main->>Engine: Runtime request {id, command: parse, payload}
  Engine-->>Main: stderr event envelope {id, type: event}
  Main-->>React: Snapshot progress update
  Engine-->>Main: stdout response envelope {id, ok, command, data}
  Main->>Engine: compile_project with skip_graph_generation=true
  Main-->>React: Source-backed workspace available
  Main->>Main: Enqueue non-blocking graph rebuild
  Main->>Engine: compile_project with graph generation enabled
  Engine-->>Main: graph materialization status
  Main-->>React: workspaceRevision increment / graph refresh
```

### Main Process Responsibilities

`apps/desktop/main.cjs` owns:

- window creation and Electron lifecycle
- IPC command routing through `hyprduck:invoke`
- file picker filters for PDF, DOCX, and DOC
- engine subprocess lifecycle in `EngineRuntime`
- parse progress snapshot publication
- local artifact opening/revealing
- provider config load/save/validation routing
- workspace graph snapshot reads
- review item resolution
- non-blocking workspace graph rebuild queue

The main process keeps a small in-memory snapshot for the renderer:

```text
activeJob
progressLog
lastResult
lastProjectId
lastWorkspaceId
lastSourceId
lastSourceManifestPath
workspaceRevision
```

This snapshot is UI state only. Durable product state lives in engine-managed
files and the project store.

### Renderer Responsibilities

`apps/desktop/src/App.tsx` owns:

- shell navigation between Knowledge and Settings
- import panel and markdown preview
- provider/model settings UI
- readiness and validation status display
- graph workspace loading and fallback display
- history/review popover
- proposing and resolving brain updates through IPC

`apps/desktop/src/features/workspace/` adapts the latest materialized graph
snapshot into the UI envelope used by `GraphWorkspace`. This keeps graph canvas
code separate from engine wire contracts.

## Rust Engine Runtime

The engine supports two process protocols.

```mermaid
flowchart LR
  OneShot["One-shot mode\nhyprduck-engine"]
  Runtime["Runtime mode\nhyprduck-engine serve"]

  OneShot -->|"stdin: EngineRequest or ParseRequest\nstdout: EngineSuccess/EngineFailure\nstderr: ParseEvent lines"| EngineCore["engine command dispatch"]
  Runtime -->|"stdin lines: EngineRuntimeRequest\nstdout lines: EngineRuntimeResponse\nstderr lines: EngineRuntimeEvent"| EngineCore

  EngineCore --> Parse["parse handled by runtime wrapper"]
  EngineCore --> Commands["commands::encode_success_response"]
```

Runtime mode requires request IDs to be UUIDv7 strings. Parse events are scoped
to the active request ID and emitted on stderr as runtime event envelopes. This
lets Electron multiplex progress logs without restarting the engine process.

Non-parse commands are dispatched through `crates/hyprduck-engine/src/commands/mod.rs`.
Parse commands are handled specially so debug request/result artifacts and parse
progress events can be written consistently.

## Engine Command Surface

The shared command schema lives in `crates/hyprduck-engine-types/src/lib.rs`.

```mermaid
flowchart TB
  EngineCommand["EngineCommand"]

  EngineCommand --> Import["Import and project workflow"]
  Import --> Parse["Parse"]
  Import --> CompileProject["CompileProject"]
  Import --> LoadProject["LoadProject"]
  Import --> ApplyCorrection["ApplyCorrection"]
  Import --> AnswerProject["AnswerProject"]

  EngineCommand --> BrainReads["Brain reads"]
  BrainReads --> SearchBrain["SearchBrain"]
  BrainReads --> ReadSource["ReadSource"]
  BrainReads --> ReadWikiPage["ReadWikiPage"]
  BrainReads --> ReadNode["ReadNode"]
  BrainReads --> ReadRecentEvents["ReadRecentEvents"]
  BrainReads --> ReadGraphHistory["ReadGraphHistory"]
  BrainReads --> ReadGraphSnapshot["ReadGraphSnapshot"]
  BrainReads --> GetContextPack["GetContextPack"]
  BrainReads --> GetBrainHealth["GetBrainHealth"]

  EngineCommand --> BrainWrites["Brain writes and replay"]
  BrainWrites --> ProposeBrainUpdate["ProposeBrainUpdate"]
  BrainWrites --> ListBrainReviewItems["ListBrainReviewItems"]
  BrainWrites --> ResolveBrainReviewItem["ResolveBrainReviewItem"]
  BrainWrites --> ReconstructBrain["ReconstructBrain"]

  EngineCommand --> ProviderRuntime["Provider and runtime"]
  ProviderRuntime --> LoadConfig["LoadConfig"]
  ProviderRuntime --> SaveConfig["SaveConfig"]
  ProviderRuntime --> ValidateProvider["ValidateProvider"]
  ProviderRuntime --> ListProviderModels["ListProviderModels"]
  ProviderRuntime --> CheckReadiness["CheckReadiness"]
```

The command surface intentionally has both product-facing project commands and
agent-facing brain commands. Project commands support the desktop workflow.
Brain commands support graph/wiki readers, MCP, reviewable proposals, and
replay/debug tooling.

## Ingest and Output Package Flow

```mermaid
flowchart TB
  File["Input file\nPDF / DOCX / DOC / image / markdown"]
  Detect["DocumentFormat"]
  Markitdown["markitdown-rs\ntext extraction"]
  Textutil["textutil\nDOC text extraction"]
  PdfToPng["pdftoppm\nPDF page images"]
  ProviderVision["provider vision parse\nimage -> markdown"]
  ProviderText["provider text parse\ntext -> markdown"]
  Fallback["fallback markdown\nwhen provider unavailable"]
  Package["output_package\nsource copy, markdown, images, manifest"]
  Compile["compile_project\nsource evidence + workspace index"]

  File --> Detect
  Detect -->|"PDF"| Markitdown
  Markitdown -->|"success"| Package
  Markitdown -->|"unsupported/empty/failure"| PdfToPng
  PdfToPng --> ProviderVision
  ProviderVision --> Package

  Detect -->|"DOCX"| Markitdown
  Markitdown -->|"DOCX fallback"| Textutil
  Textutil --> ProviderText
  ProviderText --> Package

  Detect -->|"DOC"| Textutil
  Detect -->|"Markdown"| ProviderText
  ProviderText --> Fallback
  ProviderVision --> Fallback
  Fallback --> Package
  Package --> Compile
```

The ingest domain is implemented in:

- `crates/hyprduck-engine/src/domains/ingest/parse.rs`
- `crates/hyprduck-engine/src/domains/ingest/output_package.rs`
- `crates/hyprduck-engine/src/domains/ingest/markdown_queue.rs`

### Output Package Contents

A successful parse writes a source-backed package under the workspace output
root. In Electron, the root is `~/Library/Application Support/HyprDuck`.

```text
<output-root>/
└── default/
    ├── sources/
    │   └── <source-id>/
    │       └── original file copy
    ├── artifacts/
    │   └── <source-id>/
    │       ├── source-manifest.json
    │       ├── output.md
    │       ├── pages/
    │       ├── images/
    │       ├── provider-graph-context.json
    │       └── provider-graph-materialization.json
    ├── source-index/
    │   ├── source-chunks.jsonl
    │   └── source-chunks-manifest.json
    ├── graph/
    ├── memory/
    ├── wiki/
    ├── events/
    ├── reviews/
    ├── runs/
    └── state/
```

The manifest is the bridge between raw parsing and knowledge materialization. It
records workspace ID, source ID, original path, copied source path, markdown
path, artifact root, page artifacts, status, and source metadata.

## Provider Architecture

Provider code lives in `crates/hyprduck-engine/src/domains/provider`.

```mermaid
flowchart TB
  Config["EngineConfig\nprovider, model, api key, base URL, prompt template"]
  Store["EngineConfigStore\n~/.hyprduck/engine-config.json"]
  Catalog["model_options_for\nprovider model list"]
  Readiness["readiness checks\nruntime dependencies + provider config"]
  ParseProvider["parse_provider.rs\nimage/text parse"]
  OpenAICompat["openai_compatible.rs\nasync-openai chat completion"]
  External["OpenRouter or Ollama\nOpenAI-compatible endpoint"]

  Store --> Config
  Config --> Catalog
  Config --> Readiness
  Config --> ParseProvider
  ParseProvider -->|"text/image prompt"| OpenAICompat
  OpenAICompat --> External
```

The implemented provider enum currently includes:

- `OpenRouter`
- `Ollama`

Both use an OpenAI-compatible chat completion request shape through
`async-openai`. Ollama is intentionally allowed without an API key. OpenRouter
requires an API key.

Provider output is used in two places:

- parse-time markdown generation for image/text input
- graph materialization through JSON schema response formats

When a provider is unavailable for parse-time work, the engine returns fallback
markdown rather than blocking local import. Graph generation, however, records a
skipped or failed provider graph materialization report.

## Knowledge Compile Flow

After parsing, HyprDuck compiles the markdown package into a source-backed
workspace project.

```mermaid
flowchart TB
  Markdown["Saved markdown output"]
  Manifest["SourceArtifactManifest"]
  Compiler["compile_knowledge_project"]
  SourceNode["Source graph node"]
  Evidence["Evidence refs from page sections"]
  ProjectStore["knowledge.sqlite3\nproject/source rows"]
  SourceIndex["source-index/source-chunks.jsonl"]
  Materialized["materialized brain repo"]

  Markdown --> Compiler
  Manifest --> Compiler
  Compiler --> SourceNode
  Compiler --> Evidence
  Compiler --> ProjectStore
  Compiler --> SourceIndex
  Compiler --> Materialized
```

The deterministic compiler intentionally keeps derived knowledge minimal. It
creates source nodes, evidence refs, source-backed project snapshots, and
answers that tell the user the source evidence is ready. Durable concepts,
claims, relations, wiki pages, and cross-source links are delegated to the
provider graph workflow or reviewable proposals.

This boundary matters: deterministic ingest prepares source truth; agent/provider
work proposes or materializes derived knowledge.

## Source Index and Retrieval Context

The retrieval domain builds a compact context for provider graph generation and
agent reads.

```mermaid
flowchart LR
  Markdown["source markdown"]
  Chunker["chunk_source_markdown"]
  Chunks["source-index/source-chunks.jsonl"]
  Snapshot["current materialized graph snapshot"]
  Retrieval["retrieval.rs"]
  ImportContext["ImportEvidenceContext"]

  Markdown --> Chunker --> Chunks
  Chunks --> Retrieval
  Snapshot --> Retrieval
  Retrieval --> ImportContext
```

The import context includes:

- new source chunks
- retrieved old source/wiki/memory evidence
- a compact workspace source outline
- allowed source and evidence refs
- graph context from the latest materialized snapshot

Provider graph proposals and materialized graph outputs are expected to cite
only context-backed source/evidence IDs.

## Brain Repository and Materialized State

The local brain repository is the durable read model for agents and the desktop
graph UI.

```mermaid
flowchart TB
  Events["events/brain_events.jsonl\nappend-only source of truth"]
  Projects["knowledge.sqlite3\nsource/project/correction rows"]
  Proposals["reviews/proposed_updates/*.json\npending review"]
  Existing["existing materialized records\nmemory + provider overlays"]
  Replayer["brain replay + effective state computation"]
  Snapshot["BrainRepoSnapshot"]
  GraphFiles["graph/nodes.json\ngraph/edges.json\ngraph/claims.json"]
  Wiki["wiki/index.md\nwiki/*.md"]
  Memory["memory/records.json"]
  Marker["state/latest-readable-snapshot.json"]
  Readers["UI, MCP, CLI brain reads"]

  Events --> Replayer
  Projects --> Replayer
  Proposals --> Replayer
  Existing --> Replayer
  Replayer --> Snapshot
  Snapshot --> GraphFiles
  Snapshot --> Wiki
  Snapshot --> Memory
  Snapshot --> Marker
  Marker --> Readers
  GraphFiles --> Readers
  Wiki --> Readers
  Memory --> Readers
```

The latest-readable marker is the loading contract. Readers trust the materialized
files only when `state/latest-readable-snapshot.json` resolves to a completed
graph materialization event for the requested workspace. This avoids half-written
graph/wiki state becoming the UI or MCP read surface.

## Provider Graph Materialization

Provider graph materialization is implemented under
`crates/hyprduck-engine/src/domains/agent_workflow`.

The current workflow is two-stage:

1. **Source-local graph build:** generate graph records grounded in the imported
   source and its evidence.
2. **Workspace linking:** add relation-only cross-source links between the new
   source graph and the existing workspace graph.

```mermaid
sequenceDiagram
  autonumber
  participant Compile as compile_project
  participant Context as ImportEvidenceContext
  participant Provider as OpenAI-compatible provider
  participant Validator as Normalize + validate
  participant Writer as materialized brain writer
  participant Runs as provider run artifacts

  Compile->>Context: Build source + workspace context
  Compile->>Provider: Source-local graph prompt + JSON schema
  Provider-->>Compile: materializedGraph JSON
  Compile->>Runs: provider-response.json
  Compile->>Validator: parse, normalize, validate source-local graph
  Validator-->>Compile: valid BrainRepoSnapshot
  Compile->>Writer: write source_graph_build event
  Writer-->>Runs: validation-report.json + graph-diff.json
  Compile->>Provider: Workspace linking prompt + relation-only JSON schema
  Provider-->>Compile: materializedGraph JSON with relations
  Compile->>Validator: parse, normalize, validate links
  Validator-->>Compile: valid linking snapshot
  Compile->>Writer: write workspace_linking event
  Writer-->>Runs: validation-report.json + graph-diff.json
```

### Provider Run Artifacts

Each graph generation stage writes inspectable run artifacts:

```text
runs/
└── provider-source-graph-<uuid>/
    ├── provider-response.json
    ├── validation-report.json
    └── graph-diff.json

runs/
└── provider-workspace-linking-<uuid>/
    ├── provider-response.json
    ├── validation-report.json
    └── graph-diff.json
```

The source artifact root also records:

```text
provider-graph-context.json
provider-graph-materialization.json
```

The materialization report stores status, provider/model, input fingerprint,
node/relation/claim/memory counts, run IDs, skipped reason, and error message.

### Reuse and Fingerprints

Provider graph reports are reusable only when the previous report is linked and
the input fingerprint matches:

- workspace ID
- source ID
- manifest update timestamp
- markdown hash
- provider slug
- model ID
- schema versions
- prompt version
- baseline snapshot marker

This prevents stale provider output from being reused after source or workspace
state changes.

## Reviewable Brain Writes

HyprDuck distinguishes safe memory-style writes from risky graph truth writes.

```mermaid
flowchart TB
  Actor["User or agent"]
  Proposal["ProposeBrainUpdate"]
  Policy{"Proposal kind"}
  Safe["memory / observation / source note\nsafe auto-apply path"]
  Risky["claim / link / wiki page\npending review path"]
  Events["brain events"]
  Reviews["reviews/proposed_updates"]
  History["History popover / MCP health"]
  Resolve["ResolveBrainReviewItem"]
  Materialize["BrainWorkspaceWriter\nrefresh materialized state"]

  Actor --> Proposal --> Policy
  Policy --> Safe --> Events --> Materialize
  Policy --> Risky --> Reviews --> History
  History --> Resolve --> Events --> Materialize
```

Desktop users can resolve pending review items from the History popover.
External agents can propose changes through MCP, but they cannot accept risky
review items or overwrite source truth.

## MCP Brain Server

`hyprduck-cli` exposes an MCP stdio server through:

```bash
hyprduck mcp serve
```

```mermaid
flowchart LR
  MCPClient["MCP client / coding agent"]
  Server["hyprduck-cli mcp serve"]
  EngineClient["SubprocessEngineClient"]
  Engine["hyprduck-engine"]
  BrainRepo["materialized brain repo"]

  MCPClient -->|"initialize, tools/list, tools/call, resources/read"| Server
  Server --> EngineClient
  EngineClient -->|"EngineRequest JSON"| Engine
  Engine --> BrainRepo
  BrainRepo --> Engine
  Engine --> Server
  Server -->|"JSON text content / resources"| MCPClient
```

MCP read tools include:

- `search_brain`
- `get_context_pack`
- `read_source`
- `read_wiki_page`
- `read_node`
- `read_recent_events`
- `read_graph_snapshot`
- `read_health`

MCP proposed-write tools include:

- `propose_memory`
- `propose_claim`
- `propose_link`
- `append_observation`
- `add_source_note`
- `request_consolidation`

MCP resources expose graph snapshots and wiki pages through `hyprduck://brain/*`
URIs.

## Storage Contracts

HyprDuck uses three storage layers.

```mermaid
flowchart TB
  subgraph Config["Provider config"]
    ConfigPath["~/.hyprduck/engine-config.json"]
  end

  subgraph ProjectIndex["Project index"]
    SQLite["knowledge.sqlite3"]
    ProjectRows["projects"]
    SourceRows["sources"]
    CorrectionRows["workspace_corrections"]
  end

  subgraph WorkspaceRepo["Workspace brain repo"]
    Sources["sources/"]
    Artifacts["artifacts/"]
    SourceIndex["source-index/"]
    Events["events/brain_events.jsonl"]
    Graph["graph/*.json"]
    Wiki["wiki/*.md"]
    Memory["memory/records.json"]
    Reviews["reviews/"]
    Runs["runs/"]
    State["state/latest-readable-snapshot.json"]
  end

  ConfigPath --> Config
  SQLite --> ProjectRows
  SQLite --> SourceRows
  SQLite --> CorrectionRows
  ProjectRows --> WorkspaceRepo
  SourceRows --> WorkspaceRepo
  CorrectionRows --> WorkspaceRepo
```

### Environment Overrides

The engine supports environment overrides for tests and local workflows:

- `HYPRDUCK_OUTPUT_DIR`: output/workspace root
- `HYPRDUCK_PROJECT_STORE`: SQLite project store path
- `HYPRDUCK_CONFIG_DIR`: provider config directory
- `HYPRDUCK_DISABLE_PROVIDER_GRAPH`: disables provider graph generation

These are process-wide settings. Rust tests that mutate them must be serialized
with a shared lock because Rust test execution is parallel by default.

## Read Model Consistency

Materialized graph/wiki state is written through `write_materialized_brain_repo`.
That function computes an effective snapshot before persistence:

```mermaid
flowchart TB
  Input["Incoming BrainRepoSnapshot"]
  ExistingEvents["Existing events JSONL"]
  ExistingMemory["Existing memory records"]
  Origins["materialized-record-origins.json"]
  ProviderOverlays["replayable provider graph events"]
  AcceptedProposals["accepted review proposals"]
  Effective["EffectiveBrainState"]
  Persist["Persist graph, wiki, memory, events, origins, marker"]

  Input --> Effective
  ExistingEvents --> Effective
  ExistingMemory --> Effective
  Origins --> Effective
  ProviderOverlays --> Effective
  AcceptedProposals --> Effective
  Effective --> Persist
```

This is why writes must be careful: the final disk state can include preserved
provider overlays, accepted proposals, existing memory records, and newly
computed deterministic source state.

## Failure and Fallback Boundaries

HyprDuck is designed to keep import useful even when advanced graph generation
fails.

| Boundary | Failure behavior |
| --- | --- |
| Provider unavailable during parse | Return fallback markdown with extracted text or image placeholder. |
| Provider unavailable during graph generation | Write provider graph materialization report with skipped reason. |
| Source graph generation fails | Report graph generation failure; source markdown/package remains available. |
| Workspace linking fails after source graph materialization | Keep source-local graph and report partial status. |
| Materialized marker missing/stale | Readers fall back to latest completed materialized event when possible. |
| Engine runtime subprocess exits | Electron fails active/queued runtime requests and can restart on next command. |
| Reviewable risky write | Store pending proposal; do not mutate trusted graph until accepted. |

## Testing Strategy

The Rust verification command used for the core workspace is:

```bash
cargo test -p hyprduck-engine-types -p hyprduck-engine-client -p hyprduck-engine -p hyprduck-cli
```

The Electron IA/contract tests are:

```bash
bun run --cwd apps/desktop test:ia
```

Important test categories:

- engine type serialization round trips
- runtime request/response envelopes
- fixture round trips for PDF/DOCX/DOC
- output package fallback behavior
- brain materialization and replay invariants
- provider graph schema/validation/materialization
- workspace deletion and correction replay
- MCP tool/resource protocol behavior
- desktop IA source-of-truth assertions

## Current Implementation Notes

These details are current as of this document:

- The active desktop app is Electron, not Tauri.
- The engine provider enum currently implements OpenRouter and Ollama.
- OpenRouter and Ollama both use the OpenAI-compatible client path.
- The deterministic compiler creates source/evidence state; derived knowledge is
  agent/provider-maintained.
- The desktop import path compiles source evidence first and queues provider
  graph generation asynchronously so the UI can show imported source state before
  graph materialization finishes.
- MCP exposes read tools and proposed-write tools. It does not expose review
  acceptance for external agents.
- `events/brain_events.jsonl` remains the audit/source-of-truth path for graph
  and wiki changes; `graph/*.json`, `wiki/*.md`, and `memory/records.json` are
  materialized read models.

## Design Implications

HyprDuck should continue to keep these boundaries strict:

- UI commands should route through one engine-backed service path.
- File import should not depend on capture permissions.
- Provider settings should be shared across parsing and graph workflows.
- Output markdown should retain references to saved image/page artifacts when
  visual parsing is used.
- Provider graph output should remain schema-constrained and validated.
- Reviewable writes should stay auditable and source-backed.
- Agent-facing interfaces should prefer structured context packs and graph reads
  over UI scraping.
