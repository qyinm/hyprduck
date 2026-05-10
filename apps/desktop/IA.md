# HyprDuck Desktop IA

목적: HyprDuck의 UI를 "파일 파싱 앱"이 아니라 "파일에서 지속적으로 자라는 개인/팀 지식 구조화 도구"로 설계하기 위한 source of truth.

참고한 방향:
- GBrain: agent-maintained brain, typed links, self-wiring knowledge graph, hybrid search, timeline/log, entity enrichment.
- Karpathy LLM Wiki: raw sources -> LLM-maintained wiki -> schema, ingest/query/lint operations, index.md/log.md, persistent compounding artifact.

핵심 전환:

```text
Before
  file -> page images -> markdown output

After
  files -> immutable sources -> extracted claims/entities/topics -> linked knowledge base
        -> wiki pages / graph / contradictions / questions / answers / updates
```

제품 문장:

```text
HyprDuck turns local documents into a maintained, evidence-backed knowledge base.
```

비범위:
- 단순 OCR/markdown 변환기를 최종 제품으로 포지셔닝하지 않는다.
- DeepSeek-only, capture-first, screen-recording-required 메시지를 다시 넣지 않는다.
- 원본 source를 LLM이 임의 수정하게 하지 않는다.

---

## 1. Product Model

HyprDuck은 세 계층을 UI에 드러낸다.

```text
+--------------------------------------------------------------------------------+
| Layer 1. Sources                                                               |
| Immutable originals: PDF, DOCX, DOC, generated page images, raw markdown        |
+--------------------------------------------------------------------------------+
                                      |
                                      v
+--------------------------------------------------------------------------------+
| Layer 2. Automatic Ingest                                                       |
| Extract entities, topics, claims, evidence refs, contradictions, typed links    |
+--------------------------------------------------------------------------------+
                                      |
                                      v
+--------------------------------------------------------------------------------+
| Layer 3. Knowledge Base                                                        |
| Maintained wiki pages, graph, index, log, answerable project memory             |
+--------------------------------------------------------------------------------+
```

UI는 사용자가 아래 질문에 답할 수 있게 해야 한다.

```text
1. 어떤 source가 들어왔나?
2. 어떤 지식 단위로 쪼개졌나?
3. 어떤 entity/topic/claim이 생겼나?
4. 기존 지식과 어디에서 연결/충돌/업데이트됐나?
5. 어떤 근거 페이지/문장/이미지에서 나온 주장인가?
6. 지금 knowledge base가 건강한가? stale/orphan/conflict가 있는가?
```

---

## 2. Top-level IA

```text
HyprDuck Desktop
|
+-- App Shell
|   |
|   +-- Native Titlebar / Drag Region
|   |   +-- Health Bell / Maintenance Notifications
|   |   +-- Right Inspector Toggle
|   +-- Sidebar
|   +-- Main Content
|
+-- Knowledge
|   |
|   +-- Empty State / Add Files
|   +-- Graph Canvas
|   |   +-- default and primary workspace
|   |   +-- source nodes
|   |   +-- concept/entity/topic/claim nodes
|   |   +-- evidence-backed links
|   +-- Right Inspector
|   |   +-- selected node/source/claim/link
|   |   +-- evidence
|   |   +-- source provenance
|   |   +-- open uploaded file
|   |   +-- reveal in finder
|   +-- Revealed Surfaces
|       +-- Source Library
|       +-- Wiki
|       +-- Claims
|       +-- Conflicts
|       +-- Ask / Add Files Composer
|       +-- ask selected graph context
|       +-- ask whole knowledge base
|       +-- attach files
|       +-- add file description
|       +-- add to knowledge base or ask temporarily
|       +-- save answer as page/claim/note
|
+-- Maintenance Agent
|   |
|   +-- Background Lint
|   +-- Auto Repair / Reprocess
|   +-- Conflict Queue
|   +-- Health Bell Notifications
|   +-- Maintenance Log
|
+-- Settings
    |
    +-- General
    +-- AI Providers
    +-- Knowledge Schema
    +-- Storage
```

---

## 3. Navigation Model

기존 `Import / Graph / Settings`를 다음처럼 제품 언어로 바꾼다. `Sources`는 독립 navigation destination이 아니라 `Knowledge` 안의 source library/surface로 흡수한다.

```text
Sidebar
|
+-- Knowledge     source file, wiki, graph, entity, topic, claim을 탐색하고 질문한다
|
+-- Settings      provider/schema/storage/maintenance 정책 설정

Window Bar
|
+-- Left: Sidebar Toggle   항상 좌측에 위치한다
+-- Right: Health Bell     자동 점검/수정 결과와 사용자 개입이 필요한 충돌만 알린다
+-- Far Right: Inspector Toggle   우측 inspector rail을 열고 닫는다

Settings는 window bar에 중복 노출하지 않는다. Settings 진입점은 sidebar 하단에만 둔다.
Health Bell은 inspector 내부에 들어가지 않는다. 우측 inspector가 열려 있어도 bell은 window bar의 독립 status surface로 남고, inspector toggle만 우측 rail을 제어한다.
```

첫 MVP에서 너무 많은 탭을 만들기 부담스럽다면 최소형은 아래다.

```text
MVP Sidebar
|
+-- Knowledge
|
+-- Settings
```

사용자가 파일을 넣으면 별도 Compile 화면 없이 HyprDuck이 자동으로 ingest하고 safe update는 자동 승인한다. 진행 상태는 Knowledge 안의 source library/graph inspector에 노출하고, 실패/충돌만 Health Bell에 올린다. Ask도 별도 navigation page가 아니라 Knowledge/Graph에서 필요할 때 여는 composer에서 수행한다. Source와 Ask는 destination이 아니라 Knowledge workspace 안의 interaction surface다. `Health`도 sidebar page가 아니라 우측 상단 window bar의 bell/notification surface로 둔다.

---

## 4. App Shell ASCII

```text
+--------------------------------------------------------------------------------+
| [sidebar] macOS drag region                         [health bell] [inspector]  |
+----------------------+-------------------------------------------+-------------+
| HyprDuck             | Current Screen                            | Right rail   |
|                      |                                           | when open    |
|  > Knowledge         |                                           |             |
|                      |                                           |             |
|  Settings            |                                           |             |
+----------------------+-------------------------------------------+-------------+
```

Collapsed sidebar:

```text
+--------------------------------------------------------------------------------+
| [open sidebar] macOS drag region                  [health bell] [inspector]    |
+--------------------------------------------------------------+-----------------+
| Current Screen                                               | Right rail      |
|                                                              | when open       |
+--------------------------------------------------------------+-----------------+
```

---

## 5. Knowledge Empty State IA

목표: Sidebar에서 Source/Ask를 제거해도 첫 사용자가 즉시 할 일을 이해하게 한다. 빈 Knowledge 화면도 graph workspace이며, 기본 행동은 파일을 추가해 evidence-backed graph를 만드는 것이다.

```text
+--------------------------------------------------------------------------------+
| Knowledge Base                                                                 |
|                                                                                |
|                         Your knowledge base is empty                            |
|                                                                                |
|             Drop PDF, DOCX, or DOC files here.                                  |
|             HyprDuck will turn them into a source-backed graph,                 |
|             wiki pages, claims, and evidence.                                   |
|                                                                                |
|                                  [Choose files]                                 |
+--------------------------------------------------------------------------------+
```

Rules:

```text
+-- Empty state shows Add Files as the primary action.
+-- Dropping files anywhere in the empty graph starts automatic ingest.
+-- Ask/Add Files composer is opened by command/action, not permanently shown by default.
+-- Asking without files searches existing knowledge; if empty, prompt suggests adding files first.
+-- Attached files can be added permanently or used temporarily for a one-time answer.
```

---

## 6. Source Library Surface IA

목표: `Sources`를 별도 navigation page로 두지 않고, Knowledge graph 안의 first-class source node로 둔다. 원본 파일은 immutable source로 탐색/관리한다. source file은 graph node가 canonical UI object이며, source index/search row/evidence provenance가 모두 같은 객체를 가리키는 projection이다.

기본 규칙:

- Source Library는 default surface가 아니다. 필요하면 `Source Index` / `All sources`로 command/search/inspector/Health에서 여는 projection이다.
- 모든 `SourceSummary`는 graph에 source node로 나타나야 한다. ingest 실패, stale, 아직 link가 없는 source도 source-only graph node로 남는다.
- source node 클릭 시 Source Detail은 오른쪽 inspector 안에서 열린다. graph context를 떠나지 않는다.
- Source Index가 생기더라도 left nav destination이 아니라 graph/search/bulk workflow 보조 surface다.

```text
Knowledge / Source Library
|
+-- Header
|   +-- Title: Source Library
|   +-- Subtitle: Immutable documents that HyprDuck automatically turns into knowledge.
|   +-- CTA: Add sources
|
+-- Source Intake
|   +-- Dropzone / Choose files
|   +-- Supported: PDF, DOCX, DOC
|   +-- Destination: ~/Library/Application Support/HyprDuck/<workspace>/sources
|
+-- Source Library
|   +-- Source rows
|       +-- filename
|       +-- type
|       +-- status: added | rendering | ingesting | ingested | needs review | failed | stale
|       +-- page count
|       +-- extracted entities/topics/claims count
|       +-- last ingested time
|
+-- Source Detail
    +-- metadata
    +-- original uploaded file preview
    +-- page thumbnails
    +-- raw extracted markdown
    +-- evidence ids
    +-- linked graph nodes / claims
    +-- auto-ingest status
    +-- reprocess action
```

Wireframe:

```text
+--------------------------------------------------------------------------------+
| Knowledge / Source Library                                       [Add sources] |
| Immutable documents that HyprDuck turns into structured knowledge automatically. |
|                                                                                |
| +----------------------------------------------------------------------------+ |
| | Drop PDF, DOCX, or DOC files here                                          | |
| | Sources are preserved. HyprDuck writes derived markdown/wiki pages beside   | |
| | them, never over the originals.                                             | |
| |                                                        [Choose files]       | |
| +----------------------------------------------------------------------------+ |
|                                                                                |
| Source Library                                                                  |
| +----------------------+----------+-----------+-------+----------+------------+ |
| | Source               | Type     | Status    | Pages | Knowledge| Updated    | |
| +----------------------+----------+-----------+-------+----------+------------+ |
| | yc-memo.pdf          | PDF      | ingested  | 18    | 42 items | 10:42      | |
| | notes.docx           | DOCX     | stale     | 7     | 13 items | yesterday  | |
| | contract.doc         | DOC      | failed    | 3     | 0 items  | -          | |
| +----------------------+----------+-----------+-------+----------+------------+ |
|                                                                                |
| Selected source: yc-memo.pdf                                                    |
| +----------------------+-----------------------------------------------------+ |
| | Original file        | Source-derived artifacts                            | |
| | yc-memo.pdf          | raw.md, pages/*.png, evidence.json                  | |
| | [Open file]          | [Open raw markdown] [Reprocess]                    | |
| | [Reveal in Finder]   | Linked graph nodes: Fundraising, Demo Day, SAFE     | |
| |                      | Evidence: p4, p9, p12                              | |
| +----------------------+-----------------------------------------------------+ |
+--------------------------------------------------------------------------------+
```

---

## 7. Automatic Ingest IA

목표: 별도 Compile 페이지나 수동 승인 단계를 만들지 않는다. 사용자가 source를 추가하면 HyprDuck이 자동으로 render -> extract -> link -> write wiki/graph까지 진행한다. safe update는 자동 승인하고, 사용자 판단이 필요한 충돌만 Health Bell로 올린다.

Automatic ingest는 Karpathy LLM Wiki의 `ingest`와 GBrain의 self-wiring graph를 제품 내부 작업으로 만든 것이다.

```text
Automatic Ingest
|
+-- Trigger
|   +-- user adds PDF/DOCX/DOC sources
|   +-- user clicks Reprocess on a source
|
+-- Pipeline
|   +-- render pages
|   +-- parse page markdown
|   +-- extract entities
|   +-- extract topics
|   +-- extract claims
|   +-- attach evidence
|   +-- update wiki pages
|   +-- create typed links
|   +-- detect contradictions
|   +-- append log.md
|
+-- Auto-approval policy
|   +-- safe page/entity/topic/link updates are written automatically
|   +-- every write keeps evidence refs and maintenance log entries
|   +-- low confidence or conflicting updates are not silently merged
|
+-- User-visible surfaces
    +-- Knowledge source-library row status: ingesting | ingested | needs review | failed
    +-- Knowledge recent updates
    +-- Health Bell for conflicts/failures/risky merges
```

Knowledge source-library ingest state wireframe:

```text
+--------------------------------------------------------------------------------+
| Knowledge / Source Library                                       [Add sources] |
| Added files automatically become wiki pages, graph links, and claims.            |
|                                                                                |
| Source Library                                                                  |
| +----------------------+--------------+-------+----------+--------------------+ |
| | Source               | Status       | Pages | Knowledge| Last activity      | |
| +----------------------+--------------+-------+----------+--------------------+ |
| | yc-memo.pdf          | ingested     | 18    | 42 items | 10:42              | |
| | notes.docx           | ingesting 46%| 7     | 13 items | extracting claims  | |
| | contract.doc         | needs review | 3     | 8 items  | conflict detected  | |
| +----------------------+--------------+-------+----------+--------------------+ |
|                                                                                |
| Selected source: notes.docx                                                     |
| +----------------------+-----------------------------------------------------+ |
| | Pipeline             | Derived artifacts                                   | |
| | render pages     done| raw.md, pages/*.png, evidence.json                  | |
| | extract claims   now | wiki updates are applied automatically when safe    | |
| | link graph       next| risky changes appear in the Health Bell            | |
| +----------------------+-----------------------------------------------------+ |
+--------------------------------------------------------------------------------+
```

Conflict review is not a page. It opens from the Health Bell only when needed:

```text
+--------------------------------------------------------------------------------+
| Review maintenance item                                                         |
|                                                                                |
| New source created a conflicting claim.                                         |
|                                                                                |
| Current: topics/pricing.md says pricing is fixed.                               |
| New:     contract.doc page 7 says pricing varies by customer tier.              |
|                                                                                |
| [Keep both with context] [Mark new claim active] [Reject new claim]             |
+--------------------------------------------------------------------------------+
```

---

## 8. Knowledge Workspace IA

목표: knowledge base를 graph 중심 workspace로 탐색한다. Graph는 예쁜 보조 시각화가 아니라 구조화된 지식의 primary surface다. 기본 Knowledge 화면에는 mode switcher, counts header, page-title chrome을 두지 않는다. Wiki/Sources/Claims/Conflicts는 graph를 떠나는 상단 탭이 아니라 graph에서 필요할 때 열리는 contextual surface로 시작한다. Entities/Topics는 별도 top-level mode가 아니라 graph filter 또는 facet으로 시작한다.

```text
Knowledge
|
+-- Graph Canvas
|   +-- default screen after app chrome
|   +-- no rounded outer wrapper
|   +-- no persistent mode tabs
|   +-- no persistent counts/title header
|
+-- Facets / Filters
|   +-- Entities
|   +-- Topics
|   +-- Link types
|   +-- Source types
|
+-- Right Inspector Rail
|   +-- selected page/node/link/claim/source
|   +-- evidence refs
|   +-- backlinks
|   +-- outgoing links
|   +-- source provenance
|   +-- edit/reprocess actions
|
+-- Revealed Surfaces
    +-- Source Library
    +-- Wiki Page
    +-- Claims Review
    +-- Conflict Review
    +-- Ask / Add Files Composer

Ask / Add Files Composer
    +-- prompt input
    +-- ask selected graph context or whole knowledge base
    +-- attach PDF/DOCX/DOC files
    +-- optional file description/instruction prompt
    +-- submit as Ask or Add Source + Ingest
```

Default Knowledge graph wireframe:

```text
+--------------------------------------------------------------------------------+
| [sidebar] macOS drag region                         [health bell] [inspector]  |
+----------------------+------------------------------------------+--------------+
| Sidebar              | Graph canvas                             | Right rail   |
|                      |                                          |              |
|  > Knowledge         |       [yc-memo.pdf]                      | Node detail  |
|                      |            | source_of                    | Evidence     |
|  Settings            |      [Fundraising]---related_to---[...]  | Provenance   |
|                      |            | mentions                     | Actions      |
|                      |      [Paul Graham]                       |              |
+----------------------+------------------------------------------+--------------+
+--------------------------------------------------------------------------------+
```

Collapsed right inspector:

```text
+--------------------------------------------------------------------------------+
| [sidebar] macOS drag region                         [health bell] [inspector]  |
+----------------------+---------------------------------------------------------+
| Sidebar              | Graph canvas                                            |
|                      |                                                         |
|  > Knowledge         |       [yc-memo.pdf]                                     |
|                      |            | source_of                                   |
|  Settings            |      [Fundraising]---related_to---[Demo Day]            |
|                      |            | mentions                                    |
|                      |      [Paul Graham]                                      |
+----------------------+---------------------------------------------------------+
+--------------------------------------------------------------------------------+
```

Graph canvas rules:

```text
+-- The canvas is the workspace, not a card inside the workspace.
+-- Do not wrap the graph view in a large rounded bordered container.
+-- Default graph view should show only the graph and minimal app chrome.
+-- Title, mode tabs, counts, and status summaries are hidden from the default canvas.
+-- Inspector opens as a right rail with a clear border-left, matching the left sidebar separation.
+-- Health Bell stays in the window bar outside the right inspector rail.
+-- Source Library, Wiki, Claims, and Conflicts are opened from graph selection, inspector actions, search, command palette, or health review.
```

Graph source behavior:

```text
+-- Source file nodes are first-class graph nodes.
+-- Clicking a source node opens the Source Detail surface in the inspector.
+-- Source Detail can also be expanded into Knowledge / Source Library with the same source selected.
+-- The uploaded file remains the immutable object of record.
+-- Derived artifacts stay adjacent: page images, raw markdown, evidence refs, linked claims.
+-- Non-source nodes still show provenance links back to their source files/pages.
```

Graph prompt composer behavior:

```text
+-- The prompt input is hidden by default and opens from command/action.
+-- It is the primary Ask surface once opened; no separate Ask navigation is required for MVP.
+-- Default scope follows current context: selected node/edge/source, visible graph cluster, or whole knowledge base.
+-- The [+] attachment action accepts PDF/DOCX/DOC files.
+-- Attached files require an intent: Add to knowledge base or Ask only this time.
+-- Default intent is Add to knowledge base for supported document files.
+-- Ask only this time uses the file as temporary context and does not create source/wiki/graph artifacts.
+-- If the user attaches a file with a prompt, the prompt is saved as source metadata when the intent is Add to knowledge base.
+-- Save metadata fields: source.description, source.user_context, source.ingest_instruction.
+-- Example: attach `contract.docx` with "This is the 2024 vendor agreement; focus on payment terms."
+-- Attached files with Add to knowledge base enter the same automatic ingest pipeline and become source-file graph nodes.
+-- The response can be saved back as a wiki page, claim, note, or source description.
```

Claims and conflicts surface:

```text
+--------------------------------------------------------------------------------+
| Claims / Conflicts Surface                                                      |
|                                                                                |
| +----------+----------------------------------+------------+---------+---------+ |
| | Status   | Claim                            | Topic      | Source  | Updated | |
| +----------+----------------------------------+------------+---------+---------+ |
| | active   | Demo day moved earlier           | demo-day   | p4      | today   | |
| | active   | SAFE terms changed for batch     | legal      | p9      | today   | |
| | conflict | Pricing is fixed vs negotiable   | pricing    | p2/p7   | today   | |
| | stale    | Old onboarding deadline           | ops        | old doc | 2w ago  | |
| +----------+----------------------------------+------------+---------+---------+ |
|                                                                                |
| Selected claim                                                                  |
| +-- Text                                                                        |
| +-- Evidence                                                                    |
| +-- Linked entities/topics                                                      |
| +-- Superseded by / contradicts                                                  |
+--------------------------------------------------------------------------------+
```

---

## 9. Embedded Graph Prompt Composer IA

목표: Ask를 별도 navigation page/screen으로 두지 않고 Knowledge/Graph에서 필요할 때 열리는 composer로 통합한다. raw chunks에 매번 RAG하는 느낌이 아니라, 선택된 graph context와 유지되는 wiki/graph를 기반으로 질문하고 답변을 다시 knowledge base에 축적한다. 기본 graph view는 Obsidian graph처럼 canvas가 중심이고, composer는 상시 점유하지 않는다.

```text
Knowledge / Graph Prompt Composer
|
+-- Prompt Input
|   +-- scope: selected graph context | visible cluster | all knowledge
|   +-- answer type: quick answer | wiki page | comparison table | briefing
|   +-- attachment intent: add to knowledge base | ask only this time
|
+-- Attachment Intake
|   +-- file picker / drag files into composer
|   +-- intent selector: add to knowledge base | ask only this time
|   +-- optional file description/instruction prompt
|   +-- source metadata preview: description / user_context / ingest_instruction
|   +-- automatic ingest starts after submit only for Add to knowledge base
|
+-- Retrieval Trace
|   +-- read index
|   +-- matched wiki pages
|   +-- walked graph links
|   +-- cited source evidence
|
+-- Answer
|   +-- response
|   +-- citations
|   +-- confidence / gaps
|   +-- suggested follow-up questions
|
+-- Save Back
    +-- save as new wiki page
    +-- append to existing page
    +-- create task: find source
```

Wireframe:

```text
+--------------------------------------------------------------------------------+
| [sidebar] macOS drag region                         [health bell] [inspector]  |
+----------------------+---------------------------------------------------------+
| Sidebar              | Graph canvas                                            |
|                      |                                                         |
|  > Knowledge         |       [selected node cluster]                           |
|                      |                                                         |
|                      | +---------------------------------------------------+   |
|                      | | Scope: Selected graph  Attach: [+ files]          |   |
|                      | | Compare this node with the attached contract.     |   |
|                      | | [Add to knowledge base] [Ask only this time] [Ask]|   |
|                      | +---------------------------------------------------+   |
+----------------------+---------------------------------------------------------+
+--------------------------------------------------------------------------------+
| Composer result / retrieval trace opens as an overlay, dock, or inspector-linked |
| answer surface after submit.                                                     |
|                                                                                |
| Retrieval trace                                                                 |
| +-- index.md -> topics/fundraising.md, entities/yc.md                           |
| +-- graph walk -> demo-day, safe-terms, paul-graham                             |
| +-- evidence -> yc-memo.pdf p4, notes.docx p2, contract.doc p7                  |
|                                                                                |
| Answer                                                                          |
| +----------------------------------------------------------------------------+ |
| | The latest docs changed three things...                                     | |
| |                                                                            | |
| | Citations: [yc-memo p4] [notes p2] [contract p7]                            | |
| | Gaps: no source confirms final pricing deadline.                            | |
| +----------------------------------------------------------------------------+ |
|                                                                                |
| [Save as wiki page] [Append to fundraising] [Create follow-up question]          |
+--------------------------------------------------------------------------------+
```

---

## 10. Health Bell & Maintenance Agent IA

목표: Health를 사용자가 매번 들어가야 하는 페이지로 만들지 않는다. 지식베이스 품질 관리는 HyprDuck이 백그라운드에서 자동으로 수행하고, 사용자 판단이 필요한 경우에만 우측 상단 custom window bar의 bell로 알린다.

원칙:

```text
1. Silent by default
   정상 lint, 자동 link repair, index/log 갱신은 조용히 처리한다.

2. Auto-fix when safe
   orphan backlink 추가, index.md 갱신, log.md append, 낮은 위험의 duplicate alias 정리,
   stale derived artifact 재생성은 자동 수행한다.

3. Notify only when judgment is needed
   의미 충돌, 근거 부족 claim, 위험한 entity merge, source 재요청이 필요한 경우만 알린다.

4. Every fix is reversible/auditable
   자동 수정은 maintenance log와 diff/provenance를 남긴다.
```

Health는 navigation destination이 아니라 app shell의 persistent status surface다.

```text
Custom Window Bar
|
+-- Left: Sidebar Toggle
|   +-- expanded: collapse sidebar icon
|   +-- collapsed: open sidebar icon
|
+-- Right: Health Bell
+-- Far Right: Inspector Toggle

Health Bell
|
+-- status: clean | working | attention_needed | failed
+-- badge count: user action required count
+-- popover
    |
    +-- Auto-fixed summary
    +-- Needs review
    +-- Failed maintenance jobs
    +-- Open maintenance log
```

Health Bell은 right inspector rail 안에 배치하지 않는다. Inspector가 열려 있을 때도 Health Bell은 window bar의 독립 버튼이고, 필요하면 inspector rail 너비만큼 좌측으로 밀려 rail 밖에 남는다.

Bell states:

```text
clean
+-- no badge
+-- tooltip: Knowledge base healthy

working
+-- subtle spinner/dot
+-- tooltip: Maintaining knowledge base...

attention_needed
+-- badge with count
+-- tooltip: 3 items need review

failed
+-- warning badge
+-- tooltip: Maintenance failed; click for details
```

Popover wireframe:

```text
+--------------------------------------------------------------------------------+
| [sidebar] macOS drag region                         [bell 3] [inspector]        |
+--------------------------------------------------------------------------------+
                                           |
                                           v
                            +-----------------------------------+
                            | Knowledge maintenance             |
                            |                                   |
                            | Auto-fixed                        |
                            | + index.md refreshed              |
                            | + 6 backlinks added               |
                            | + 2 stale artifacts reprocessed   |
                            |                                   |
                            | Needs review                      |
                            | ! Conflict: pricing fixed/varies  |
                            | ! Merge? Paul G. / Paul Graham    |
                            | ! Claim missing citation          |
                            |                                   |
                            | [Review now] [Dismiss]            |
                            | [Open maintenance log]            |
                            +-----------------------------------+
```

Review drawer/modal:

```text
+--------------------------------------------------------------------------------+
| Review maintenance item                                                         |
|                                                                                |
| Conflict: pricing fixed vs pricing variable                                     |
|                                                                                |
| +--------------------------------------+---------------------------------------+ |
| | Current knowledge                    | New evidence                          | |
| | topics/pricing.md says fixed         | contract.doc page 7 says variable     | |
| | source: sales-note.pdf page 2        | confidence: medium                    | |
| +--------------------------------------+---------------------------------------+ |
|                                                                                |
| Suggested resolution                                                            |
| +-- mark newer claim active                                                                            |
| +-- keep both and label context-specific                                                               |
| +-- request better source                                                                               |
|                                                                                |
| [Accept suggestion] [Keep both] [Reject new claim] [Ask me later]               |
+--------------------------------------------------------------------------------+
```

Background maintenance jobs:

```text
Safe auto-fix
+-- rebuild index.md from wiki pages
+-- append log.md maintenance entry
+-- add missing backlinks when source/target are unambiguous
+-- re-render stale page artifacts
+-- re-run failed low-risk extraction
+-- normalize aliases when exact match is obvious
+
Needs user review
+-- conflicting claims with comparable evidence
+-- duplicate entities that could be different people/companies
+-- missing citation for an important answer/claim
+-- destructive rewrite of a wiki page
+-- deleting or superseding a claim
+-- importing external/web sources not chosen by user
```

---

## 11. Settings IA

Settings는 단순 provider 설정이 아니라 knowledge automation의 규칙을 포함해야 한다.

```text
Settings
|
+-- General
|   +-- workspace name
|   +-- output folder
|   +-- default source destination
|
+-- AI Providers
|   +-- provider
|   +-- model
|   +-- API key / base URL
|   +-- prompt template
|   +-- validation
|
+-- Knowledge Schema
|   +-- page types: overview, source, entity, topic, claim, question
|   +-- entity types: person, company, project, product, place, event, concept
|   +-- typed links: mentions, source_of, related_to, founded, works_at, advises,
|       invested_in, contradicts, supersedes, supports
|   +-- citation format
|   +-- acceptance policy: auto-accept high confidence or require review
|
+-- Maintenance Policy
|   +-- safe auto-fix enabled
|   +-- notify only on judgment-required items
|   +-- maintenance log retention
|   +-- require review before destructive rewrites
|
+-- Storage
    +-- raw sources path
    +-- page images path
    +-- wiki path
    +-- index/log path
```

Wireframe:

```text
+--------------------------------------------------------------------------------+
| Settings                                                                       |
|                                                                                |
| +----------------------+-----------------------------------------------------+ |
| | General              | Knowledge Schema                                    | |
| | AI Providers         |                                                     | |
| | > Knowledge Schema   | Page types                                          | |
| | Maintenance Policy   | [overview, source, entity, topic, claim, question] | |
| | Storage              |                                                     | |
| |                      |                                                     | |
| |                      | Entity types                                        | |
| |                      | [person, company, project, product, event, concept]| |
| |                      |                                                     | |
| |                      | Typed links                                         | |
| |                      | mentions, source_of, related_to, contradicts, ...  | |
| |                      |                                                     | |
| |                      | Acceptance                                          | |
| |                      | ( ) auto-accept safe updates                       | |
| |                      | (x) auto-accept safe updates                        | |
| +----------------------+-----------------------------------------------------+ |
+--------------------------------------------------------------------------------+
```

---

## 12. Data Artifacts Exposed by UI

HyprDuck는 아래 산출물을 명시적으로 다룬다.

```text
~/Library/Application Support/HyprDuck/<workspace>/
|
+-- sources/
|   +-- original files
|
+-- artifacts/
|   +-- page images
|   +-- raw page markdown
|   +-- extracted source JSON
|
+-- wiki/
|   +-- index.md
|   +-- log.md
|   +-- overview.md
|   +-- sources/*.md
|   +-- entities/*.md
|   +-- topics/*.md
|   +-- claims/*.md
|   +-- questions/*.md
|
+-- graph/
|   +-- nodes.json
|   +-- edges.json
|   +-- evidence.json
|
+-- reviews/
    +-- proposed-updates/*.json
    +-- lint-reports/*.md
```

UI에서 반드시 보이는 artifact:

```text
Source
+-- original file path
+-- rendered page images
+-- raw markdown

Knowledge
+-- generated wiki page path
+-- source evidence refs
+-- graph node/edge ids

Maintenance
+-- health bell status
+-- maintenance log recent entries
+-- proposed updates
+-- auto-fixed summary
+-- user-review queue
```

---

## 13. Core User Flows

### 13.1 First automatic ingest

```text
Open app
  |
  v
Knowledge empty state
  |
  v
Add PDF/DOCX/DOC
  |
  v
Source rendered into pages + raw markdown
  |
  v
HyprDuck automatically extracts entities/topics/claims/evidence
  |
  v
Safe wiki/graph updates are written automatically
  |
  v
Only conflicts/failures appear in Health Bell
  |
  v
Knowledge base now has wiki pages + graph + evidence
  |
  v
Command-opened graph composer can answer from built knowledge
```

### 13.2 Add a new source to an existing knowledge base

```text
Add source
  |
  v
HyprDuck automatically ingests against existing index/wiki/graph
  |
  +-- create new pages when safe
  +-- update existing pages when safe
  +-- add typed links
  +-- flag contradictions
  +-- mark stale claims
  |
  +-- safe changes -> write automatically and append log.md
  |
  +-- judgment needed -> Health Bell review item
```

### 13.3 Ask and save answer

```text
Open the graph composer from command/action
  |
  v
HyprDuck reads index/wiki/graph/evidence
  |
  v
Answer with citations and gaps
  |
  +-- user discards answer
  +-- user saves answer as questions/*.md
  +-- user appends answer to existing topic/entity page
  |
  v
Saved answer becomes part of future knowledge
```

### 13.4 Background maintenance and health notifications

```text
Knowledge base changes
  |
  v
HyprDuck runs background maintenance
  |
  +-- rebuild index/log if needed
  +-- repair obvious backlinks
  +-- reprocess stale artifacts
  +-- detect conflicts / missing evidence / risky merges
  |
  +-- safe fix possible -> apply automatically -> log diff -> no user interruption
  |
  +-- judgment needed -> health bell badge -> user reviews only that item
  |
  v
Knowledge base stays coherent without becoming a sidebar chore
```

---

## 14. State Inventory

```text
Global
|
+-- startup_error
+-- sidebar_collapsed
+-- active_destination: knowledge | settings
+-- health_bell: clean | working | attention_needed | failed
+-- workspace_status: empty | ready | ingesting | degraded | stale
+
Source Library
|
+-- no_sources
+-- source_selected
+-- source_rendering
+-- source_rendered
+-- source_ingest_pending
+-- source_ingested
+-- source_failed
+
Ingest
|
+-- ingest_idle
+-- ingest_running
+-- source_ingested
+-- auto_updates_written
+-- review_needed
+-- ingest_partial_failure
+-- ingest_failed
+
Knowledge
|
+-- no_knowledge
+-- overview_ready
+-- wiki_page_selected
+-- graph_node_selected
+-- graph_edge_selected
+-- claim_selected
+-- conflict_selected
+-- stale_after_new_source
+
Embedded Ask Composer
|
+-- prompt_empty
+-- prompt_has_context
+-- files_attached
+-- attachment_intent_add_to_knowledge
+-- attachment_intent_ask_only
+-- file_description_prompt_added
+-- answer_pending
+-- answer_grounded
+-- answer_low_confidence
+-- answer_blocked
+-- answer_saved_to_wiki
+-- attached_file_ingest_started
+
Maintenance
|
+-- maintenance_idle
+-- maintenance_running
+-- auto_fix_applied
+-- review_queue_ready
+-- review_item_selected
+-- maintenance_failed
+
Settings
|
+-- config_loading
+-- provider_ready
+-- missing_api_key
+-- ollama_unavailable
+-- schema_dirty
+-- saving
+-- validation_failed
```

---

## 15. Content Labels

권장 UI 라벨:

```text
Source Library labels
- Add sources
- Source Library
- Original file
- Page images
- Raw markdown
- Add source

Automatic Ingest
- Auto-ingest
- Ingesting
- Ingested
- Needs review
- Reprocess
- New wiki page
- Updated wiki page
- New typed link
- Possible contradiction

Knowledge
- Knowledge Base
- Wiki
- Graph
- Entities filter
- Topics filter
- Claims
- Conflicts
- Evidence
- Backlinks
- Source provenance

Graph Prompt Composer
- Ask knowledge base
- Ask selected graph
- Attach files
- Add to knowledge base
- Ask only this time
- File description
- Ingest instruction
- Retrieval trace
- Grounded answer
- Citations
- Save as wiki page
- Append to page

Health Bell / Maintenance
- Knowledge maintenance
- Auto-fixed
- Needs review
- Conflict needs review
- Missing citation
- Maintenance log
- Review now
```

피해야 할 라벨:

```text
Just parse
OCR only
Graph preview
Capture
Screen Recording required
DeepSeek-only
Raw RAG answer
```

---

## 16. Implementation Mapping

현재 코드에서 바꿔야 하는 방향:

```text
apps/desktop/src/App.tsx
|
+-- ActivePanel
|   current: import | graph
|   target: knowledge | settings
|   note: health is not a panel; it is a top-right bell/popover
|   note: sidebar toggle is not right-aligned; it stays on the left side of the custom window bar
|   note: right inspector toggle lives at the far right of the custom window bar
|   note: health bell stays outside the right inspector rail
|
+-- MAIN_NAV_ITEMS
|   current labels likely Import / Graph
|   target labels Knowledge
|   top-right window bar has HealthBell and InspectorToggle
|
+-- ImportPanel
|   current role: file selection + latest markdown
|   target role: Knowledge source-library surface + source detail
|
+-- SettingsPanel
|   keep provider settings
|   add Knowledge Schema and Storage tabs later

apps/desktop/src/features/workspace/GraphWorkspace.tsx
|
+-- current role: graph workspace preview
+-- target role: Knowledge screen graph canvas
+-- graph canvas is the default surface and should not be wrapped in a large rounded card
+-- remove persistent mode tabs, counts, page-title chrome, and overview header from the default graph view
+-- expose Wiki / Sources / Claims / Conflicts as contextual surfaces, not default tabs
+-- keep Entities / Topics as filters/facets before making them full modes
+-- source-file nodes open Source Detail in the inspector, with actions to open/reveal the uploaded file
+-- Ask/Add Files composer opens on command/action and handles file attachment, attachment intent, and source metadata prompts

apps/desktop/src/features/workspace/types.ts
|
+-- current: WorkspaceProject, nodes, edges, evidence, answers
+-- target additions later:
    +-- SourceSummary
    +-- KnowledgePageSummary
    +-- KnowledgeClaim
    +-- KnowledgeUpdateProposal
    +-- KnowledgeLintFinding
    +-- TypedLinkKind
```

MVP implementation order:

```text
P0: Reposition UI without backend expansion
+-- Fold Import/Sources into Knowledge as a source-library surface
+-- Rename Graph -> Knowledge in navigation and copy
+-- Add IA-driven empty states explaining sources -> auto-ingest -> knowledge
+-- Keep existing parse pipeline, but present output as source-derived artifacts
+
P1: Add automatic ingest mental model
+-- Show source row status: ingesting / ingested / needs review / failed
+-- Show extracted nodes/edges/evidence as knowledge items, not graph demo
+-- Make source-file graph nodes clickable and route them to Source Detail / uploaded file preview
+-- Add command-opened graph prompt composer for Ask, file attachment, attachment intent, and source metadata prompts
+-- Auto-approve safe updates; route only conflicts/failures to Health Bell
+
P2: Add real knowledge artifacts
+-- Persist wiki/index/log style markdown outputs
+-- Add source metadata fields: description, user_context, ingest_instruction
+-- Add entities/topics filters/facets
+-- Add typed link kinds beyond current relation labels
+
P3: Add background maintenance loop
+-- Health bell in custom window bar
+-- auto-fix safe index/backlink/stale artifact issues
+-- notify only for conflicts, missing evidence, risky merges, failed jobs
+-- maintenance log and review modal/popover
```

---

## 17. Acceptance Criteria for UI Redesign

A redesigned HyprDuck UI is acceptable only if:

```text
[ ] A new user understands this is a knowledge base builder, not only a parser.
[ ] Sources are shown as immutable inputs.
[ ] Markdown is shown as one artifact among wiki/graph/evidence outputs, not the final product.
[ ] Adding a source automatically creates safe wiki/graph updates without requiring a separate Compile page.
[ ] Evidence is attached to every meaningful claim/node/link in the UI.
[ ] The graph is connected to wiki pages, claims, backlinks, and source provenance.
[ ] Clicking a source-file node in the graph exposes the uploaded file and its Source Detail without losing graph context.
[ ] Ask happens from the Knowledge/Graph composer when invoked, not a separate required navigation page.
[ ] The graph prompt composer is not permanently visible in the default graph canvas.
[ ] The graph prompt composer supports file attachments plus an optional file-description prompt before automatic ingest.
[ ] Default Knowledge view does not show persistent mode tabs, overview counts, or page-title chrome above the graph.
[ ] Default graph canvas is not wrapped in a large rounded card.
[ ] The right inspector is a separate rail with a clear left border, and Health Bell remains outside that rail.
[ ] File attachment distinguishes Add to knowledge base from Ask only this time, with Add to knowledge base as the default for supported docs.
[ ] File description prompts are saved as source metadata, not consumed as disposable prompts.
[ ] Ask answers can be saved back into the knowledge base.
[ ] Health is not a sidebar chore: safe maintenance runs automatically and only judgment-required issues notify via the top-right bell.
[ ] Provider errors remain specific and actionable.
[ ] PDF/DOCX/DOC import remains permission-free.
```

---

## 18. Change Management

IA를 바꾸는 경우:

```text
1. 먼저 이 문서의 ASCII 구조를 수정한다.
2. 변경이 Knowledge / Source Library / Graph Prompt Composer / Automatic Ingest / Maintenance 중 어디에 속하는지 명시한다.
3. source/file attachment -> auto-ingest -> knowledge/graph -> answer -> health 루프가 깨지지 않는지 확인한다.
4. 구현 파일 경로를 Implementation Mapping에 반영한다.
5. 구현 후 `pnpm --dir apps/desktop build`로 desktop UI 빌드를 확인한다.
```

이 문서는 HyprDuck UI 개편의 source of truth다.
