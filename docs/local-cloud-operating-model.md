# Local And Cloud Operating Model

**Status:** Accepted product contract. Local Workspace is the current desktop
path. Cloud Workspace is a supported architectural mode, but its complete
desktop UX and movement workflow are not shipped yet.

## Purpose

Etyma must remain genuinely local-first while supporting an optional cloud
workspace for users who need server-side access, organization membership, or
always-available MCP access.

This document defines storage authority, processing locality, data ownership,
workspace movement, and the implementation sequence. It does not introduce an
external connector or a synchronization protocol.

The Source and Source Revision definitions in [`source-model.md`](source-model.md)
are normative for this document.

## Product Guarantees

- A Local Workspace works without an account or network connection.
- Cloud storage is explicit and opt-in. Local Source content is never uploaded
  implicitly.
- Hosted model processing and cloud workspace storage are separate decisions.
- Local model processing does not change workspace authority.
- Every Source Revision records the processing locality and provider route used
  to create its derived artifacts.
- Local paths are redacted from agent-facing output by default.
- Canonical Source and approved mutation records have a portable export form
  that does not require the Etyma UI to interpret them.
- Graph, wiki, search, and Context Pack outputs are rebuildable projections of
  canonical records and captured Source Revisions.
- A workspace has exactly one primary authority at a time.

These are product guarantees for the operating model. The current
implementation status is listed in [Current Implementation](#current-implementation).

## Workspace Authority

| Mode | Primary authority | Account | Offline contract | Primary storage |
| --- | --- | --- | --- | --- |
| Local Workspace | User device | Not required | Import, retrieval, Ask, graph inspection, and local MCP remain usable offline | Local SQLite + GraphQLite and approved local Source storage |
| Cloud Workspace | Etyma Cloud | Required for workspace access | Cached reads may remain available, but full offline writes are not promised until an explicit sync protocol exists | Postgres control/knowledge/graph planes plus Blob storage |

The authority is a property of the workspace, not of the model provider. A
Local Workspace may use a hosted provider for a specific operation. A Cloud
Workspace may eventually use a local provider from an authorized client. Neither
case changes where the workspace is authoritative.

Local and cloud are not simultaneous primary authorities for one workspace.
The product must not silently turn a local database and a cloud database into
multi-master replicas.

## Storage And Processing Matrix

Storage and processing are independent axes:

| Workspace storage | Local model | Hosted model | Status |
| --- | --- | --- | --- |
| Local Workspace | Source and derived artifacts remain local | Source content may be sent for the opted-in operation; route and locality are recorded; no workspace upload follows automatically | Current local path |
| Cloud Workspace | Deferred until a client-side execution contract exists | Cloud service processes the cloud-owned Source Revision under workspace policy | Cloud target path |

The following terms must remain distinct:

- **Local storage:** Source Revisions, evidence, and knowledge state are
  authoritative on the user's device.
- **Cloud storage:** Source Revisions, evidence metadata, and workspace state
  are authoritative in the cloud service.
- **Local processing:** The model or parser runs on the user's device.
- **Hosted processing:** Source content is sent to a hosted provider or cloud
  worker for a specific operation.

Hosted processing must disclose the provider route, locality, and relevant
warnings before or at the point of use. A hosted model call is not permission
to persist the Source in an Etyma Cloud Workspace.

## Data Ownership

| Data | Canonical owner | Rebuildable or derived form | Notes |
| --- | --- | --- | --- |
| Workspace authority and membership | Local workspace manifest or Cloud control plane | UI session state | One authority per workspace |
| Source identity and metadata | Local knowledge store or Cloud knowledge plane | Source list projections | Follow `docs/source-model.md` |
| Source Revision payload | Approved local Source storage or Cloud Blob plane | Download cache | Content hash and revision identity are required |
| Evidence records and index | Durable citation projection owned by the workspace authority | Rebuilt from a Source Revision | Evidence IDs and locators must remain deterministic within a revision |
| User context and Source instructions | Workspace knowledge plane | UI form state | Must be included in portable export |
| Corrections and approved mutations | Append-only local event history or Cloud audit/mutation records | Graph and wiki projections | User intent is canonical; rendered graph/wiki files are not |
| Connector state | Reserved for a future connector service or local adapter | Cursor/status UI | No connector is implemented by this operating model |
| Graph nodes and relations | Mutation events plus Source/evidence references | GraphQLite or Postgres graph projection | Projection may be rebuilt |
| Wiki pages and claims | Approved mutation records and evidence references | Markdown/wiki read model | Must retain provenance when rebuilt |
| Search and FTS indexes | None | Local or Cloud retrieval index | Rebuildable cache |
| Provider run metadata | Workspace audit record | Run UI | Preserve provider route, local/hosted status, hash, warnings, and timestamps |
| Context Packs | None by default | Query-time artifact or optional history | Never the primary Source store |

Evidence is durable because agents need stable citations, but it is still
derived from a captured Source Revision. If an evidence index is lost, Etyma
must be able to rebuild it without changing the meaning of the Source Revision.

## Local Workspace

The Local Workspace is the default product path.

- No account or cloud service is required for core use.
- The Electron desktop shell and Rust engine use local SQLite and GraphQLite as
  the authoritative knowledge and graph store.
- Original Source access is limited to approved canonical roots and protected
  against path or symlink escapes.
- Local MCP reads use the same local Source and evidence state as the desktop.
- Hosted provider use is explicit and does not change the workspace authority.
- Provider failure must not delete or invalidate an already captured Source
  Revision. The UI must expose a specific retry or local-provider path when
  available.

Local Workspace data may be exported as a portable package containing Source
metadata, captured revisions or references to them, evidence, user context,
and approved mutation records. Absolute local paths are not portable identity.

## Cloud Workspace

The Cloud Workspace is the optional multi-tenant path.

- Account and workspace membership are required.
- Postgres owns control, knowledge metadata, import jobs, evidence metadata,
  and the relational graph projection.
- Blob storage owns captured Source Revision payloads. Postgres stores blob
  references, hashes, sizes, and content types rather than raw payloads.
- The desktop client is a client of the Cloud Workspace and may keep a
  disposable cache. The cache is never an authority.
- Cloud MCP access is scoped to workspace authorization and tokens.
- Cloud failures must preserve queued jobs and retryable state without
  reporting a Source as citation-ready when its evidence is unavailable.

The current `etyma-server` runtime provides Postgres-backed control,
knowledge, graph, and blob primitives. A complete desktop Cloud Workspace
experience, including workspace selection and movement, remains future work.

## Security Boundaries

### Local

- Do not require Screen Recording or Accessibility permissions for document
  import or normal Local Workspace use.
- Resolve local Source access through approved canonical roots.
- Reject arbitrary agent-provided paths and symlink escapes.
- Redact local paths from MCP and agent-facing output unless explicitly
  requested for debugging.
- Keep hosted provider disclosure separate from account or cloud-workspace
  state.
- Store provider credentials through the existing local configuration boundary;
  never persist them in Source content or Context Packs.

### Cloud

- Enforce organization and workspace membership before Source or evidence reads.
- Keep control, knowledge, graph, and Blob plane ownership explicit.
- Never expose raw Blob keys or private local paths as ordinary agent output.
- Treat Source Revision payloads as tenant data and apply retention and
  deletion policy to both metadata and Blob objects.
- Record provider route, processing locality, import status, and audit events.
- Do not treat a valid human session as workspace authorization; workspace
  APIs still require workspace-scoped access.

## Workspace Movement

Movement is an explicit snapshot operation, not transparent synchronization.

### Export Local Workspace

1. Freeze a consistent local snapshot.
2. Export Source identity, Source Revision metadata, captured payloads or
   declared payload references, evidence, user context, and approved mutation
   events.
3. Exclude absolute paths, transient UI state, provider secrets, and rebuildable
   indexes unless explicitly requested as diagnostics.
4. Include content hashes so the destination can verify every captured revision.

### Move Local Workspace To Cloud

The future move operation must create or select a Cloud Workspace, upload an
explicit snapshot, verify hashes and evidence counts, and only then offer a
choice of authority. The original Local Workspace remains a recoverable export
or archive; it is not silently converted into a live replica.

After cutover, the cloud copy is authoritative. Continuing to write to the old
local copy creates a separate workspace unless a future, explicitly designed
sync protocol is enabled.

### Export Cloud Workspace To Local

The future export operation downloads a verified snapshot of Source Revisions,
evidence, user context, and approved mutation events into a new Local
Workspace. It must not copy Postgres or GraphQLite database files directly.
Derived graph, wiki, and search state is rebuilt locally.

## Failure And Offline Behavior

| Condition | Local Workspace | Cloud Workspace |
| --- | --- | --- |
| No network | Core local workflows continue; hosted operations are unavailable | Cached reads may work; authoritative writes wait for the cloud |
| Hosted provider unavailable | Preserve Source Revision; offer retry or local route when available | Preserve cloud Source Revision and job state; retry through the cloud job system |
| Local engine unavailable | Desktop reports local readiness failure; cloud is unaffected | Client may report cached state; cloud API remains authoritative |
| Cloud unavailable | No impact on local authority | Do not claim fresh state or successful writes; retain retryable client intent only when explicitly supported |
| Partial ingest | Keep citation readiness and graph readiness separate | Keep citation readiness and graph readiness separate |

Offline support for Cloud Workspace is intentionally not a hidden promise.
Full offline cloud writes require a future conflict and sync contract and are
outside this operating model.

## Implementation Sequence

### Phase 0: Freeze the contract

- Keep this document and `docs/source-model.md` as the canonical product
  definitions.
- Reconcile the current cloud storage documentation with the Postgres-backed
  `etyma-server` runtime.
- Keep Slack, other external connectors, and synchronization out of scope.

### Phase 1: Harden the Local Workspace

- Preserve the existing PDF, DOCX, and DOC import flow.
- Make workspace authority and processing locality explicit in internal status
  and artifact metadata.
- Define a portable export envelope for Source Revisions, evidence, and
  approved mutations.
- Verify account-free, offline local import, retrieval, Ask, graph inspection,
  and MCP behavior.

### Phase 2: Complete Cloud Workspace Foundations

- Keep Postgres control, knowledge, and graph planes plus Blob storage as the
  cloud authority.
- Finish workspace-scoped access, job recovery, Source Revision handling, and
  evidence-aware cloud pack reads.
- Add a cloud workspace client contract without making local databases cloud
  replicas.

### Phase 3: Explicit Workspace Movement

- Implement verified Local-to-Cloud and Cloud-to-Local snapshot movement.
- Preserve Source IDs, revision hashes, evidence locators, and mutation event
  identity where the destination contract allows it.
- Make authority selection visible and auditable.

### Phase 4: Reconsider Synchronization Separately

Only after real demand for concurrent local/cloud writes should Etyma define a
sync protocol. That work would need explicit conflict semantics, event ordering,
deletion handling, connector ownership, and security review. It is not part of
the current product contract.

## Explicit Non-Goals

This operating model does not:

- Add Slack or any other external channel connector.
- Implement connector OAuth or background remote ingestion.
- Implement bidirectional local/cloud synchronization.
- Introduce CRDTs or generic conflict resolution.
- Require an account for Local Workspaces.
- Upload local Source content automatically.
- Replicate SQLite or GraphQLite database files to the cloud.
- Change the PDF, DOCX, or DOC user flow.
- Add billing, subscription, or team collaboration UI.
- Claim that the future Cloud Workspace or movement workflows are already
  implemented.

## Current Implementation

| Area | Current state | Contract interpretation |
| --- | --- | --- |
| Desktop and engine | Electron + Rust engine with local SQLite and GraphQLite | Local Workspace authority |
| Document import | PDF, DOCX, and DOC adapter | First Source adapter; preserve existing behavior |
| Provider routes | OpenRouter and Ollama paths | Processing locality is independent of workspace authority |
| Local MCP | Reads local Source, evidence, graph, and Context Pack state | Local Workspace surface |
| Cloud server | Postgres control/knowledge/graph plus Blob-backed Source payloads | Cloud Workspace backend foundation; not complete desktop cloud UX |
| Local/cloud movement | No verified snapshot movement yet | Phase 3 |
| External connectors | Not implemented | Explicitly deferred |
| Live synchronization | Not implemented | Explicitly deferred |

The current implementation must not be described as offering Cloud Workspace
desktop parity, live local/cloud sync, or external connector ingestion.
