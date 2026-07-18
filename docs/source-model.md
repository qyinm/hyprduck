# Source Model

## Definition

A **Source** is a registered, provenance-bearing origin that Etyma can
independently identify, capture or re-fetch, version, process, and cite.

The source is the durable product object. Its bytes, remote payload, parsed
text, page images, transcript, and graph nodes are representations or
derivatives of that object.

Documents are the first implemented source type. They do not define the
boundary of the model.

## Qualification Test

An input qualifies as a Source only when Etyma can:

1. Assign it a stable workspace-scoped `sourceId`.
2. Describe its origin without relying on generated content.
3. Capture or resolve a specific revision of its content.
4. Detect content changes with a hash, revision ID, or equivalent version.
5. Produce evidence with locators that resolve within that revision.
6. Record the access, processing locality, provider route, and ingest warnings
   needed to audit its use.

An input that cannot meet these conditions may be a transient attachment or
query input, but it is not a durable Source.

## Object Boundaries

### Source

The logical origin registered in a workspace. It owns identity, kind, title,
origin metadata, access policy, and revision history.

Choose the narrowest boundary that can be independently added, refreshed,
permissioned, and removed while still having one coherent revision. A page,
chunk, OCR block, or transcript segment is normally evidence inside a Source,
not a separate Source.

Examples:

- One PDF or Word document
- One Markdown note
- One web page
- One repository
- One image
- One audio recording
- One email thread
- One page or record from a connected service

### Source Revision

An immutable captured state of a Source. A revision is identified by
`sourceId` plus a content hash, upstream revision ID, commit SHA, ETag, or
equivalent version marker.

Evidence always binds to a Source Revision, never only to a mutable live
origin. Refreshing a Source creates or selects a new revision; it does not
silently rewrite the evidence attached to an older revision.

The current document adapter effectively exposes one current revision through
the Source Pack `contentHash`. Future adapters must make revision identity
explicit.

### Source Container

A connector, folder, site, mailbox, or workspace that discovers Sources. A
container is not automatically a Source because it is usually too broad to
version and cite as one unit.

Examples:

- A Google Drive folder is a container; each imported file is a Source.
- A Notion workspace is a container; each imported page or database record is
  a Source.
- A website crawl is a container; each canonical page is normally a Source.

A repository is the deliberate exception: the repository revision is a useful
version boundary, while file paths and line ranges act as evidence locators.

## What Is Not a Source

- A connector or provider configuration
- A local file picker selection before registration
- A one-time attachment that the user chose not to add
- Generated markdown, page images, OCR text, or transcripts
- An evidence item or quotation
- A claim, entity, concept, relation, memory, or wiki page derived by Etyma
- A Context Pack assembled for one query
- A model answer

These objects may point to a Source or Source Revision, but they must not
replace its provenance.

## Source Kind And Format

`kind` describes the semantic origin and selects an ingest adapter. `format`
or `contentType` describes the technical representation handled by that
adapter.

| Source kind | Example formats | Typical evidence locator |
| --- | --- | --- |
| `document` | PDF, DOCX, DOC, Markdown, plain text | page, section, paragraph, span |
| `web_page` | HTML, rendered page | canonical URL, heading, DOM/text span |
| `repository` | Git repository | revision, file path, line range, symbol |
| `image` | PNG, JPEG, HEIC | region, bounding box, OCR span |
| `audio` | WAV, MP3, M4A | timestamp range, speaker, transcript span |
| `video` | MP4, MOV | timestamp range, frame, transcript span |
| `message_thread` | email or chat thread | message ID, sender, timestamp, span |
| `connected_record` | service page or database record | external record ID, field, block |

This taxonomy is a target product model, not a claim that every adapter is
implemented. The current local adapter accepts PDF, DOCX, and DOC. Existing
`image` and `markdown` parser enum values are internal capabilities, not proof
of a complete user-facing source adapter.

## Required Source Metadata

Every Source adapter must normalize its input into these conceptual fields:

| Field | Meaning |
| --- | --- |
| `sourceId` | Stable identity within a workspace |
| `workspaceId` | Security and retrieval boundary |
| `kind` | Semantic source family and adapter route |
| `title` | Human-readable label |
| `origin` | Canonical local or remote origin descriptor |
| `externalId` | Optional stable identity from an upstream system |
| `contentType` | Technical media or document type |
| `revision` | Upstream version, commit, ETag, or equivalent |
| `contentHash` | Integrity key for the captured content |
| `status` | Ingest lifecycle state |
| `providerRoute` | Processing route used for derived artifacts |
| `localOnly` | Whether source processing stayed local |
| `createdAt` / `updatedAt` | Registration and refresh timestamps |

Local paths are implementation metadata. They are redacted from agent-facing
output unless the user explicitly enables path disclosure.

## Evidence Contract

Every adapter must produce addressable evidence that contains:

- `sourceId`
- Source revision identity or `contentHash`
- A type-appropriate locator
- The quoted or represented content
- Parse or extraction confidence when applicable
- References to derived artifacts needed for verification

The locator is adapter-specific, but it must be deterministic within the bound
revision. Page numbers are valid for documents; they are not the universal
evidence model.

## Identity And Deduplication

- Equal content hashes do not automatically mean equal Sources. Two distinct
  origins can contain identical bytes and retain separate provenance.
- A refreshed origin normally keeps its `sourceId` and creates a new Source
  Revision.
- Importing the same immutable origin twice should be detected and offered as a
  reuse or refresh operation.
- Moving or renaming a local file must not change evidence identity when Etyma
  can prove it is the same registered Source.
- Merging Sources is an explicit, auditable user decision. It must not happen
  solely because titles, URLs, or content are similar.

## Lifecycle And Readiness

The current ingest lifecycle is:

```text
added -> rendering -> ingesting -> ingested
                              \-> partial
                              \-> failed
ingested | partial -----------> stale -> ingesting
```

Lifecycle status and readiness are separate:

- `citationReady` means agent-facing evidence can be retrieved and cited.
- `graphReady` means graph and wiki materialization has completed.
- A Source may be citation-ready while graph materialization is pending or has
  failed.

Future adapters may replace `rendering` with adapter-specific internal stages,
but the user-facing contract should remain registration, processing, usable,
partial, failed, and stale.

## Privacy And Security

- Registration must resolve through approved canonical roots or an authorized
  connector.
- Production tools must not accept arbitrary agent-provided filesystem paths.
- Symlink and path escapes must remain blocked.
- Remote refresh must use the Source's stored connector identity and access
  policy, not an arbitrary URL supplied by an agent.
- Hosted processing must be disclosed before source content is sent.
- Removing connector access must prevent refresh without erasing already
  captured revisions unless the user requests deletion.
- Deletion, retention, and connector revocation must be auditable.

## Current Implementation Mapping

| Product concept | Current implementation |
| --- | --- |
| Source identity | `SourceId` / `source_id` |
| Source kind | Local `DocumentFormat`; cloud spike `kind` |
| Origin | `original_path`, `source_path`; cloud `external_id` |
| Revision integrity | Source Pack and Evidence Index `contentHash` |
| Evidence locator | `page`, `region`, optional `span`; cloud `locator` |
| Lifecycle | `IngestStatus` |
| Citation readiness | `citation_ready` |
| Graph readiness | `graph_ready` |
| Processing disclosure | `providerRoute`, `localOnly` |

The current local structs are document-shaped. New adapters should extend the
general Source and Source Revision contracts instead of adding more
document-specific fields to the shared model.

## Product Language

Use these terms consistently:

- **Add Source** for durable registration and ingest
- **Attach** for transient input to one Ask interaction
- **Refresh** for resolving a newer revision of the same Source
- **Reprocess** for rebuilding derivatives from the same Source Revision
- **Remove** for taking a Source out of the workspace
- **Delete captured data** when original bytes and revisions are also erased

Use a specific noun such as Document, Web Page, Repository, or Recording only
when the source kind changes the action or evidence locator.
