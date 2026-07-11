# Agent Chat Rig Workflow

This document describes the current Rig-backed Agent chat workflow implemented
in `crates/etyma-engine/src/application/services/agent_chat_service.rs`.

## Runtime Shape

```mermaid
flowchart TD
  UI["Desktop AgentChatWorkspace"] --> IPC["Electron IPC: agent_chat_start"]
  IPC --> Runtime["EngineRuntime: agent_chat_ask"]
  Runtime --> Service["execute_agent_chat"]
  Service --> State["AgentChatRunState"]

  State --> Retrieval["retrieve_context_pack_for_agent"]
  Retrieval --> ContextPack["Context Pack service"]
  ContextPack --> Store["SQLite + GraphQLite + FTS5 retrieval"]

  State --> PlannerDecision{"Need more retrieval queries?"}
  PlannerDecision -->|yes| QueryPlanner["Rig agent: context query planner"]
  QueryPlanner --> PlannedQueries["JSON queries[]"]
  PlannedQueries --> Retrieval

  State --> PromptBuilder["build_agent_turn_prompt"]
  PromptBuilder --> Generator["Rig agent: answer generation"]
  Generator --> Provider{"Provider"}
  Provider --> OpenRouter["OpenRouter"]
  Provider --> Ollama["Ollama"]

  Generator --> CitationGuard["validate_model_citations"]
  CitationGuard --> RepairDecision{"Repair needed?"}
  RepairDecision -->|yes, non-streaming| RepairPrompt["build_citation_repair_user_prompt"]
  RepairPrompt --> Generator
  RepairDecision -->|no| Response["AgentChatAskResponseData"]
```

## State Machine

```mermaid
stateDiagram-v2
  [*] --> ResolvingScope
  ResolvingScope --> ClassifyQuestion

  ClassifyQuestion --> RetrieveContext: evidence-capable turn
  ClassifyQuestion --> AssessContext: general turn

  RetrieveContext --> AssessContext

  AssessContext --> RetrieveContext: blocked and next query candidate exists
  AssessContext --> PlanContextQueries: blocked and planner not attempted
  AssessContext --> Block: blocked and no more query candidates
  AssessContext --> ConnectProvider: evidence or general mode

  PlanContextQueries --> RetrieveContext: Rig planner adds candidate
  PlanContextQueries --> Block: planner fails or adds no candidate

  ConnectProvider --> Generate
  Generate --> ValidateCitations

  ValidateCitations --> RepairCitations: evidence mode and no valid citations and non-streaming retry remains
  RepairCitations --> Generate

  ValidateCitations --> Finalize: citations valid or no repair path
  Block --> Finalize
  Finalize --> [*]
```

## Query Planning Loop

```mermaid
sequenceDiagram
  participant SM as AgentChat state machine
  participant Candidates as Context query candidates
  participant Context as Context Pack service
  participant Search as SQLite GraphQLite FTS5 search
  participant Planner as Rig query planner

  SM->>Candidates: build_context_query_candidates(request)

  loop up to MAX_CONTEXT_RETRIEVAL_ATTEMPTS
    SM->>Context: retrieve_context_pack_for_agent(query)
    Context->>Search: query source text, evidence, metadata, graph trails
    Search-->>Context: ContextPackV1
    Context-->>SM: selectedEvidence + warnings

    alt citation-ready context exists
      SM->>SM: answerMode = Evidence
    else next deterministic candidate exists
      SM->>Candidates: advance_context_query()
    else Rig planner not attempted
      SM->>Planner: run_rig_agent(CONTEXT_QUERY_PLANNER_PREAMBLE)
      Planner-->>SM: JSON {"queries":[...]}
      SM->>Candidates: sanitize and append planned queries
    else no citation-ready context
      SM->>SM: answerMode = Blocked
    end
  end
```

## Rig Calls

```mermaid
flowchart TD
  RigEntry["run_rig_agent / run_rig_agent_stream"] --> ProviderSwitch{"EngineConfig.provider"}

  ProviderSwitch -->|OpenRouter| OpenRouterClient["openrouter::Client"]
  ProviderSwitch -->|Ollama| OllamaClient["ollama::Client"]
  ProviderSwitch -->|Unknown| ProviderError["provider_config error"]

  OpenRouterClient --> RigBuild["client.agent(model).preamble().context().temperature(0.2).max_tokens(1200).build()"]
  OllamaClient --> RigBuild

  RigBuild --> NonStreaming{"Streaming request?"}
  NonStreaming -->|no| Prompt["agent.prompt(prompt).max_turns(2).with_tool_concurrency(2)"]
  NonStreaming -->|yes| StreamPrompt["agent.stream_prompt(prompt).multi_turn(2)"]

  Prompt --> Text["model text"]
  StreamPrompt --> Delta["AgentChatStreamEvent::Delta"]
  Delta --> Text
```

## Answer Generation And Citation Guard

```mermaid
sequenceDiagram
  participant SM as AgentChat state machine
  participant Prompt as Prompt builder
  participant Rig as Rig generator
  participant Guard as Citation validator
  participant UI as Desktop stream

  SM->>Prompt: build_agent_turn_prompt(request, run)

  alt answerMode == Evidence
    Prompt-->>SM: EVIDENCE_AGENT_PREAMBLE + context document + evidence prompt
  else answerMode == General
    Prompt-->>SM: GENERAL_AGENT_PREAMBLE + no-context marker + general prompt
  else answerMode == Blocked
    SM->>SM: synthesize blocked response
  end

  SM->>Rig: run_rig_agent or run_rig_agent_stream

  alt streaming
    Rig-->>UI: delta events
  end

  Rig-->>SM: model text
  SM->>Guard: validate_model_citations(model text, context pack)

  alt valid citations found
    Guard-->>SM: cited ContextPackEvidenceV1 items
  else evidence mode and non-streaming retry remains
    SM->>Prompt: build_citation_repair_user_prompt
    SM->>Rig: regenerate with valid evidenceRefs listed
  else no valid citation path
    SM->>SM: attach fallback citations if needed
  end

  SM-->>UI: final result
```

## Scope Rules

```mermaid
flowchart TD
  Request["AgentChatAskRequest"] --> Mode{"mode"}

  Mode -->|auto| WorkspaceSearch["Search indexed workspace evidence"]
  Mode -->|all_docs| WorkspaceSearch
  Mode -->|selected_source| SourceFilter["Search then filter selected sourceIds"]
  Mode -->|graph_context| GraphScoped["Use selectedNodeId as retrieval selection"]

  WorkspaceSearch --> ContextPack["ContextPackV1"]
  SourceFilter --> ContextPack
  GraphScoped --> ContextPack

  DesktopRule["Desktop Agent page"] --> AutoOnly["Sends mode auto and selectedNodeId null"]
  AutoOnly --> WorkspaceSearch
```

## Design Notes

- The Rust loop is the deterministic controller. Rig is called only for query
  planning and answer generation.
- Query planning is dynamic: it runs only after deterministic query candidates
  fail to produce citation-ready context.
- Evidence prompts include recent user messages only. Prior assistant messages
  are not treated as evidence.
- Citation validation is post-generation and local. Invalid evidenceRefs are
  dropped, and non-streaming evidence answers may be regenerated once with a
  repair prompt.
- The desktop Agent page does not inherit graph selection state. Graph selection
  is inspection state unless an explicit graph scoped request is sent.
