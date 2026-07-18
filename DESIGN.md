# Etyma Desktop Design System

## Product Context

- **Product:** A desktop application that compiles private sources into reusable, cited local context for coding agents.
- **Primary users:** Developers, researchers, and knowledge workers who need verifiable source evidence in agent workflows.
- **Product type:** A dense, repeat-use, macOS-first professional tool.
- **Core flow:** `Add Sources -> Ask With Citations -> Verify Evidence -> Reuse`.
- **Desired impression:** "A calm local knowledge tool that feels related to Finder, Notes, and the Xcode inspector."

Etyma is not a generic file chat product or an AI SaaS dashboard. It is a desktop workbench where users can inspect original sources, ingest state, evidence, relationships, and provider routes.

### Source Model

A source is a provenance-bearing original input with a stable identity, content hash, type, and evidence trail. Documents are the first supported source type, not the limit of the product model.

The canonical domain definition, including Source Revisions, containers, evidence locators, and lifecycle semantics, lives in [`docs/source-model.md`](docs/source-model.md).

- **Currently supported:** PDF, DOCX, and DOC files.
- **Future source families:** Plain text and Markdown, web content, code repositories and files, images, audio or video transcripts, and connected services.
- **Invariant:** Every source type must preserve origin, local or hosted processing disclosure, ingest warnings, and addressable evidence.
- **UI rule:** Use `Source` for the general object. Use `Document`, `Web Page`, `Repository`, or another specific type only when the distinction matters.

## Aesthetic Direction

- **Direction:** Native Mac Knowledge Workbench
- **Decoration:** Minimal
- **Layout:** macOS split views with contextual inspectors
- **Color:** Neutral surfaces with system semantic colors
- **Density:** Between compact and medium
- **Motion:** Short and functional

The app must feel like a professional macOS tool, not a web page placed inside a desktop window. Window structure, alignment, separators, selection states, and keyboard behavior create the visual identity.

### Core Principles

1. Each screen has one dominant work surface.
2. Every answer remains traceable to its original evidence.
3. Complexity is progressively disclosed in the right inspector.
4. Local and hosted execution states are never hidden.
5. The same source or evidence object keeps the same identity across Sources, Ask, and Knowledge.
6. The graph is an inspection surface, not decoration.

### Avoid

- Marketing-scale headings and hero whitespace
- Wrapping every section in a rounded card
- Repeating black pills for every button and input
- Decorative purple, orange, or green brand accents
- Gradient backgrounds, glows, orbs, and bokeh
- Decorative illustrations or mascot-led UI
- Oversized mobile-style controls
- Making generated answers appear more trustworthy than their evidence
- Treating the graph as a static background image

## Design Reference

- Pencil source: [`apps/desktop/design/etyma-apple-native.pen`](apps/desktop/design/etyma-apple-native.pen)

The Pencil document is the visual baseline. Implementation may differ to support accessibility, localization, and real data states, but the window structure, density, color roles, and hierarchy should remain consistent.

## Typography

### Font Stack

```css
font-family:
  Inter,
  ui-sans-serif,
  system-ui,
  -apple-system,
  BlinkMacSystemFont,
  "SF Pro Text",
  "Apple SD Gothic Neo",
  "Noto Sans KR",
  "Segoe UI",
  sans-serif;
```

- Prefer the system font on macOS.
- Use `Apple SD Gothic Neo` and `Noto Sans KR` as Korean fallbacks when localized content requires them.
- Use `ui-monospace`, `SFMono-Regular`, and `Menlo` for paths, source IDs, evidence IDs, revisions, and commands.
- Letter spacing is always `0` by default.

### Type Scale

| Role | Size | Weight | Line height | Usage |
| --- | ---: | ---: | ---: | --- |
| Screen title | 20px | 650 | 26px | Sources, Knowledge |
| Conversation title | 18px | 650 | 24px | Ask with your sources |
| Inspector title | 13px | 650 | 18px | Source Details, Citations, Selection |
| Section title | 12-13px | 600-650 | 18px | Evidence, Metadata |
| Body/UI | 12-12.5px | 400 | 18px | Answers, descriptions, buttons |
| List/table | 11-11.5px | 400-600 | 16px | Source rows and status |
| Supporting copy | 10.5px | 400 | 15px | Provider and timestamp |
| Metadata/badge | 9-10px | 500-650 | 13px | Evidence ID and object type |

Hierarchy comes from position, alignment, weight, separators, and selection state rather than large type.

### Numbers and Identifiers

- Use tabular numbers for page counts, evidence counts, percentages, and progress.
- Do not remove the middle of long source IDs or paths. Truncate to one line in compact surfaces and expose the full value in a tooltip or inspector.
- Do not expose internal IDs unless they help the user verify or debug an artifact.

## Color

### Base Palette

| Token | Value | Usage |
| --- | --- | --- |
| `--native-window` | `#F7F7F8` | Window background and inspector |
| `--native-sidebar` | `#ECECEF` | Left sidebar |
| `--native-content` | `#FFFFFF` | Primary work surface |
| `--native-hover` | `#E4E4E7` | Neutral hover |
| `--native-selection` | `#DCEBFF` | Selected rows and nodes |
| `--native-text` | `#1D1D1F` | Primary text |
| `--native-text-secondary` | `#68686D` | Descriptions and metadata |
| `--native-text-tertiary` | `#909095` | Inactive information |
| `--native-border` | `#D6D6D9` | Panel and control boundaries |
| `--native-separator` | `#E8E8EA` | List and table separators |
| `--native-accent` | `#0A84FF` | Selection, focus, and primary actions |
| `--native-accent-soft` | `#EAF4FF` | Citation and selection support surfaces |

### Semantic Colors

Semantic color must always appear with an icon or text label. Never communicate status through color alone.

| State | Value | Usage |
| --- | --- | --- |
| Information/focus | `#0A84FF` | Selection, links, focus ring |
| Success/ready | `#30A46C` | Indexed source, local engine ready |
| Warning/review | `#D97706` | Stale source, partial parse |
| Error/destructive | `#D14343` | Import failure, destructive action |

### Color Constraints

- At least 80% of a default screen should remain neutral.
- Blue is reserved for selection, primary actions, links, and citations.
- Green is reserved for completed and locally ready states.
- File-type colors are limited to small icon wells.
- Near-black is used for body text and small high-contrast controls such as Send.

## Materials and Surfaces

### App Window

- Reference size: `1440 x 900`
- Recommended minimum: `960 x 640`
- Window corner radius: 12px
- Do not obstruct the macOS traffic-light or drag regions.
- Content extends to the window edges. Do not place the entire application inside a centered card.

### Titlebar

- Height: 52px
- Background: `rgba(242, 242, 243, 0.91)`
- Bottom border: 1px `--native-border`
- Center: small product or workspace name
- Left: traffic lights and sidebar toggle
- Right: window-level actions such as search, history, and inspector toggle
- Screen titles and primary actions belong in the screen toolbar, not the titlebar.

### Sidebar

- Default width: 224px
- Background: `--native-sidebar`
- Right border: 1px `--native-border`
- Horizontal padding: 12px
- Navigation row height: 34px
- Active row radius: 7px
- Active rows use a translucent white surface with a subtle inner highlight.
- Place local engine status and Settings at the bottom.
- Show counts only when they materially help navigation.

### Primary Work Surface

- Background: `--native-content`
- Fills all space below the toolbar.
- One of the source list, conversation, or graph is the dominant surface.
- Prefer split views, toolbars, and separators over cards.
- Do not add a page-wide max-width card container.

### Inspector

- Default width: 320-360px
- Background: `--native-window`
- Left border: 1px `--native-border`
- Shows details for the selected source, citation, graph node, or graph edge.
- Follows the current selection and does not introduce separate page navigation.
- The primary work surface expands naturally when the inspector closes.

### Elevation

| Level | Treatment | Usage |
| --- | --- | --- |
| 0 | No shadow | Sidebar, toolbar, table, inspector |
| 1 | `0 1px 2px rgba(0,0,0,.04)` | Active navigation, small controls |
| 2 | `0 3px 10px rgba(0,0,0,.05)` | Document preview, composer |
| Overlay | `0 18px 48px rgba(0,0,0,.16)` | Popover, menu, sheet |

Repeated rows and structural panels do not use shadows.

## Spacing and Density

- **Base unit:** 4px
- **Toolbar height:** 56-64px
- **Screen horizontal padding:** 20-24px
- **Inspector padding:** 18-22px
- **Section gap:** 16-24px
- **Navigation row:** 34px
- **Source row:** 52-58px
- **General table row:** 36-44px
- **Button height:** 28-32px
- **Search/input height:** 30-34px
- **Icon button:** 28-32px square
- **Internal control gap:** 6-10px
- **Default radius:** 6-8px
- **Composer radius:** 12px
- **Pill shape:** Counts, statuses, and short filters only

Density must remain consistent within a screen. Do not make the inspector unusually cramped or the composer unusually large.

## Application Structure

```text
App Window
|-- Titlebar: 52px
`-- Body
    |-- Sidebar: 224px
    |   |-- Workspace picker
    |   |-- Sources
    |   |-- Ask
    |   |-- Knowledge
    |   |-- Local engine status
    |   `-- Settings
    `-- Active workspace
        |-- Toolbar: 56-64px
        |-- Primary work area: fluid
        `-- Inspector: 320-360px, contextual
```

### Window Width Behavior

- `>= 1180px`: Sidebar + main surface + inspector
- `960-1179px`: Sidebar + main surface; inspector opens on demand
- `< 960px`: Sidebar collapses; inspector opens as an overlay sheet

Etyma Desktop is not a mobile application. Small windows must preserve reading and core actions without introducing bottom navigation or oversized touch controls.

## Screen Architecture

### Sources

**Primary question:** Which sources are available, and are they ready to use?

```text
Sources toolbar
|-- Title + source count
|-- Search
`-- Add Source

Source workspace
|-- Compact drop zone
`-- Source list
    |-- Name / format
    |-- Status
    |-- Pages
    |-- Evidence
    `-- Modified

Source Inspector
|-- Original preview
|-- Source identity and readiness
|-- Open / Reveal / Graph
|-- Metadata
`-- Citation readiness
```

- The import drop zone must not dominate the screen.
- The source table is the primary work surface.
- Selecting a row populates the right inspector.
- Do not place every possible action inside each row. Use the inspector or a context menu.
- Represent the import queue as temporary rows or inline progress above the source list.
- Connect warnings and failures directly to their source rows and inspector details.

### Ask

**Primary question:** What do my sources support, and where can I verify the answer?

```text
Ask toolbar
|-- Thread title
|-- Local/hosted disclosure
`-- Provider selector

Conversation
|-- User message
|-- Assistant answer
|   `-- Inline citation chips
`-- Composer

Citation Inspector
|-- Source preview
|-- Selected evidence
|-- Page / region / confidence
`-- Open in source
```

- The path to verifying a citation must never be visually weaker than the answer.
- Citation chips use a short `page + evidence ID` label.
- Selecting a citation highlights the same evidence in the inspector.
- Assistant answers sit directly in the conversation flow, not inside cards.
- Only user messages may use a compact message bubble.
- Keep the composer anchored at the bottom without obscuring content.
- Clearly disclose local and hosted provider state in the toolbar.
- Only show currently supported provider paths: OpenRouter and Ollama.

### Knowledge

**Primary question:** How are sources, concepts, and evidence connected?

```text
Knowledge toolbar
|-- Node/link count
|-- All / Sources / Concepts filters
|-- Search
`-- Center / layout controls

Graph canvas
|-- Source nodes
|-- Concept/entity/topic nodes
|-- Artifact nodes
`-- Evidence-backed edges

Graph Inspector
|-- Selection identity
|-- Type and confidence
|-- Connected evidence
|-- Source provenance
`-- Open original / extracted text
```

- The graph canvas is the largest area on the screen.
- Distinguish source, concept, and artifact nodes with icons and type labels in addition to color.
- Selected nodes use a soft system-blue background and a 2px accent border.
- Selecting an edge highlights both connected nodes and the supporting evidence.
- Zoom, pan, center, and filter actions must not move structural UI.
- An empty graph offers source import or reprocessing actions instead of decorative artwork.
- Do not market graph, wiki, claims, memory, or event history as the first-run product promise.

### Settings

- Preserve sidebar navigation and use setting rows in the main surface.
- Expose `General` and `AI` first.
- Use a `label / control / helper text` row structure.
- Explain missing provider keys, hosted/local state, and connection errors specifically.
- Ollama remains configurable without an API key.
- Confirm saved changes near the toolbar or changed setting row.

## Components

### Buttons

#### Primary

- Height: 30-32px
- Background: `--native-accent`
- Text: white
- Radius: 7px
- Use for one primary action per screen or toolbar.
- Examples: `Add Source`, `Open`, and saving provider settings.

#### Secondary

- Background: white
- Border: 1px `--native-border`
- Text: `--native-text`
- Hover: `--native-window`

#### Ghost and Icon Buttons

- Transparent by default
- Use `--native-hover` only on hover
- Prefer a familiar Lucide symbol over a text label when the action is universally understood.
- Every icon button requires an `aria-label` and tooltip.

#### Destructive

- Neutral secondary appearance by default
- Introduce error color only at the point of confirmation.
- Place destructive actions in an overflow menu or dedicated inspector danger section.

### Inputs and Search

- Height: 30-34px
- Background: `--native-window` or white
- Border: 1px `--native-border`
- Radius: 6-7px
- Focus: 1px `--native-accent` with a 3px translucent ring
- Placeholder: `--native-text-tertiary`
- Pair error borders with an explanatory sentence.
- Search includes a leading icon and supports clearing with `Escape`.

### Lists and Tables

- Header height: 32-36px
- Header background: `--native-window`
- Separate rows with 1px `--native-separator`.
- Selected row: `--native-selection`
- Hover uses a neutral tone weaker than selection.
- Keep column alignment stable between states.
- Use consistent right or tabular alignment for numeric columns.
- Status badges and long filenames must not change row height.

### Status

- Default form: 6-7px dot + text label
- Normal states use low-saturation gray or green.
- Reserve stronger semantic color for warning and failure.
- Use actionable language such as `Ready`, `Indexing`, `Warning`, `Failed`, and `Stale`.
- Pair progress states with a percentage or stage description.

### Citation

- Background: `--native-accent-soft`
- Text: `--native-accent`
- Radius: 5px
- Height: 18-20px
- Format: `p.2 - E4`
- Hover and focus preview the connected source and evidence.
- An answer without citations must not appear equivalent to a citation-ready answer.

### Source Preview

- Preserve the original page aspect ratio.
- Only the page surface receives a paper-white fill and subtle shadow.
- Use icon controls for page navigation, zoom, and fullscreen.
- Switch between original and parsed Markdown with tabs or a segmented control.
- Synchronize the current page between the original and parsed output when possible.

### Composer

- Maximum radius: 12px
- Default height: 64-72px
- May grow within a constrained range as input expands.
- Place scope and attachment controls on the left and Send/Stop on the right.
- Send is a 28px square icon button.
- During streaming, the same control changes to Stop without moving.

### Graph Node

- Radius: 8px
- Background: white
- Border: 1px `--native-border`
- Selection: `--native-selection` with 2px `--native-accent`
- Icon well: 24-28px
- Always pair the label with a type.
- At high density, reveal labels progressively according to zoom and selection.

### Popover and Menu

- Radius: 8px
- Background: `rgba(255,255,255,.92)`
- Backdrop blur: 18-24px
- Item height: 28-32px
- Clearly distinguish selection, keyboard focus, and destructive items.
- Do not nest another card-styled container inside a popover.

## Icons

- Use `lucide-react` throughout the product UI.
- Default size: 14-16px
- Primary sidebar icons: 16px
- Primary object icons: 16-18px
- Keep stroke weight and perceived size consistent.
- Use `FileText` for files, `MessageCircle` for Ask, and `Waypoints` or `Network` for relationships.
- The feather mark is a small titlebar product mark, not a mascot illustration.
- Do not create custom SVG illustrations to explain features.

## Interaction and State

Every data-driven screen supports:

- Loading
- Empty
- Ready
- Partial/warning
- Error
- Permission/restriction

### Loading

- Preserve the layout and show progress inside rows, previews, or inspectors.
- Do not cover the entire work surface with a spinner.
- Already-ready sources remain usable while another import runs.

### Empty

- Explain the screen purpose in one sentence.
- Emphasize one next action.
- Show real controls and supported formats instead of decorative graphics.

### Error

- Explain what failed and what the user can do next.
- Do not collapse missing provider keys, an unavailable Ollama instance, and parse failures into one generic error.
- Technical details may be expandable, but the primary error message remains visible.

### Selection

- List, graph, and inspector selection share one state.
- Clear the inspector when the selected object disappears.
- Pointer and keyboard selection use the same visual state.

## Accessibility

- Every workflow is operable with a keyboard.
- Never remove focus rings.
- Do not distinguish selection, status, or file type with color alone.
- Icon buttons have an `aria-label` and tooltip.
- Lists and tables expose selection with `aria-selected` or an appropriate semantic state.
- The graph provides a searchable node list or equivalent keyboard navigation path.
- Citations expose source, page, and evidence in their accessible name.
- Text contrast meets WCAG AA.
- Remove position and composer-size transitions under `prefers-reduced-motion`.
- Minimum pointer target: 28px. Primary actions are at least 30px.

## Motion

- Sidebar open/close: 180ms ease
- Inspector open/close: 180-220ms ease
- Hover/focus: 100-140ms
- Popover/menu: 140-180ms
- Composer height: 220-320ms with restrained spring-like easing
- Graph selection: 120ms
- Import progress: animate only continuous value changes

Do not use decorative entrance animations, full-screen fades, or excessive springs. Data refreshes must not move structural UI.

## Implementation Principles

- React and Electron own window behavior, interaction, and rendering.
- The Rust engine owns parsing, provider execution, artifact generation, and persistence.
- Use Tailwind and the existing UI primitives without treating default shadcn styling as the product identity.
- Define global tokens in `apps/desktop/src/styles.css`.
- Build small native control primitives instead of repeating arbitrary class combinations across screens.
- Prefer Lucide icons.
- Redact local paths by default in agent-facing or shareable output.
- Preserve translation contracts in `apps/desktop/src/i18n/` when copy changes.
- Check `apps/desktop/IA.md` and its tests when visible information architecture changes.
- Compare implementation against the Pencil reference at `1440x900`, `1180x760`, and `960x640`.

## Do and Do Not

### Do

- Make source lists as scannable as Finder.
- Keep reading and question flow as quiet as Notes.
- Put selected-object details in a right inspector, as Xcode does.
- Clearly disclose local/hosted state and provider routes.
- Connect source, page, and evidence through one object model.
- Place warnings near the affected source or evidence.
- Provide keyboard shortcuts and context menus for repeated actions.

### Do Not

- Add a landing-page hero to the desktop app.
- Nest cards inside cards.
- Turn every control into a pill.
- Replace the source table with a decorative card grid.
- Reduce the graph to a screenshot or miniature preview.
- Hide citations or provenance behind generated content.
- Imply direct OpenAI or Anthropic integrations before they ship.
- Make Screen Recording or Accessibility permissions prerequisites for source import.

## Design Review Checklist

### Structure

- [ ] Is there one dominant work surface?
- [ ] Are sidebar, toolbar, main surface, and inspector roles clear?
- [ ] Are repeated sections free of unnecessary cards?
- [ ] Are core actions visible at `960x640`?

### Information

- [ ] Are source name, status, and critical metadata visible?
- [ ] Can a user trace every cited answer to the original?
- [ ] Does graph selection stay synchronized with the inspector?
- [ ] Is local/hosted provider state accurate?

### Visual

- [ ] Are neutral surfaces dominant and semantic colors restrained?
- [ ] Is compact UI free of oversized headings?
- [ ] Are button and input heights consistently within 28-34px?
- [ ] Are radius and shadow used only for functional boundaries?
- [ ] Does all text fit without overlap?

### Interaction

- [ ] Are loading, empty, warning, and error states defined?
- [ ] Can the primary flow be completed without a pointer?
- [ ] Do destructive actions provide clear confirmation and recovery?
- [ ] Do state changes preserve layout stability?

## Decision Log

| Date | Decision | Reason |
| --- | --- | --- |
| 2026-07-18 | Adopt Native Mac Knowledge Workbench | Establish the feel of a repeat-use local professional tool |
| 2026-07-18 | Retire black-pill-first styling | Repeated pills gave every interaction the same visual weight |
| 2026-07-18 | Use `Sources / Ask / Knowledge` as top-level destinations | Separate import, cited answers, and relationship inspection |
| 2026-07-18 | Use a 224px sidebar and 320-360px inspector | Support list comparison and detail inspection in one split view |
| 2026-07-18 | Use system blue as the only primary accent | Unify selection, focus, primary actions, and citations |
| 2026-07-18 | Maintain a three-screen Pencil reference | Give implementation and design review one visual baseline |

## Known Scope

- The current baseline covers light appearance. Define dark appearance tokens with a dedicated design.
- Team and cloud collaboration surfaces are outside the current design scope.
- Add detailed relation-editing and correction workflows when product behavior is finalized.
- Windows and Linux preserve the information architecture while providing fallbacks for macOS-specific materials.
