# HyprDuck — Design System

## Overview

HyprDuck is a local document parsing workspace for turning files into agent-ready markdown. The design is intentionally direct: a paper-white canvas, black primary actions, pill-shaped controls, native-feeling typography, and documentation-like rhythm. The product should read like a trustworthy local tool, not like a generic AI SaaS page.

The system is intentionally plain. A user should understand the workflow in seconds: import a PDF, DOCX, or DOC file; convert pages into images; run the configured provider; save linked markdown under `~/Documents/HyprDuck/`; inspect the evidence graph when available. The UI should not look like a generic AI SaaS dashboard.

**Key characteristics:**
- Paper-white `{colors.canvas}` from edge to edge.
- Black `{colors.primary}` pills for primary actions.
- Soft gray pills for paths, local-mode state, provider state, and command-like snippets.
- A narrow, readable content column for landing/product pages.
- Compact desktop workspace with sidebar, import panel, markdown preview, and graph/evidence surface.
- Terminal or README-like previews for parse logs, local provider readiness, and markdown output.
- No decorative gradients, atmospheric backgrounds, heavy shadows, or stock imagery.

## Similar Brands

These brands are useful comparison points for tone and restraint. HyprDuck should not copy their layouts, assets, or product metaphors directly; use them to calibrate the level of polish, density, and confidence.

- **OpenAI:** calm monochrome surfaces, restrained hierarchy, clear prose, and product pages that feel serious without becoming visually heavy.
- **Linear:** dense but elegant workspace UI, fast command-oriented interactions, crisp borders, compact sidebars, and strong attention to states.
- **Vercel:** developer-tool clarity, black-and-white contrast, documentation-first rhythm, simple CTAs, and clean deployment/status surfaces.

## Colors

### Brand & Accent
- **Pure Black** (`{colors.primary}` — `#000000`): brand anchor, primary CTA, active nav text, strong headings, solid icons.
- **Ink Deep** (`{colors.ink-deep}` — `#090909`): pressed state for black pills and inverted controls.

### Surface
- **Canvas** (`{colors.canvas}` — `#ffffff`): primary page and desktop workspace background.
- **Soft Surface** (`{colors.surface-soft}` — `#fafafa`): path pills, import zones, search/input pills, inactive chips, quiet side panels.
- **Surface Dark** (`{colors.surface-dark}` — `#171717`): rare inverted surface for terminal previews, parse-log panels, and one strong "local ready" moment.
- **Hairline** (`{colors.hairline}` — `#e5e5e5`): card borders, dividers, terminal card borders, FAQ rows.
- **Hairline Strong** (`{colors.hairline-strong}` — `#d4d4d4`): stronger separators for unrelated groups or focused cards.

### Text
- **Ink** (`{colors.ink}` — `#000000`): headings, primary nav links, black-on-white controls.
- **Charcoal** (`{colors.charcoal}` — `#525252`): secondary labels, feature bullets, provider metadata, body text where slightly stronger contrast is needed.
- **Body** (`{colors.body}` — `#737373`): default paragraph copy, helper text, FAQ answers, footer links.
- **Mute** (`{colors.mute}` — `#a3a3a3`): captions, disabled controls, timestamp text, terminal comments.
- **On Dark** (`{colors.on-dark}` — `#ffffff`): text on `{colors.surface-dark}`.
- **On Dark Mute** (`{colors.on-dark-mute}` — `rgba(255,255,255,0.7)`): secondary text on dark terminal/log panels.

### Semantic
Semantic color should be sparse. HyprDuck is a local utility, not a status-heavy observability tool.

- **Terminal Red** (`{colors.terminal-red}` — `#ff5f56`): close-window dot inside terminal-like previews only.
- **Terminal Yellow** (`{colors.terminal-yellow}` — `#ffbd2e`): minimize dot inside terminal-like previews only.
- **Terminal Green** (`{colors.terminal-green}` — `#27c93f`): local-ready dot and terminal zoom dot.
- **Focus Ring** (`{colors.focus-ring}` — `rgba(59,130,246,0.5)`): browser-default focus ring. This is the only blue in the system.

## Typography

### Font Family
- **SF Pro Rounded** (display headings) — Apple's rounded geometric sans, used at weights 500 and 600 for headlines from `{typography.display-xl}` (36px) down to `{typography.heading-lg}` (24px). Falls back to `system-ui` -> `-apple-system`.
- **ui-sans-serif** (body, links, buttons, captions) — the operating system's default sans-serif. Carries every non-display text role at 12-20px.
- **ui-monospace** (code, paths, command tags) — the OS default monospace. Used inside terminal previews, output paths, inline command chips, and markdown/code previews.

The typography should feel native and documentation-like. Avoid branded display faces that make the product feel like a marketing page.

### Hierarchy

| Token | Size | Weight | Line Height | Letter Spacing | Use |
|---|---:|---:|---:|---:|---|
| `{typography.display-xl}` | 36px | 500 | 1.11 | 0 | Hero headline and first-run empty-state headline |
| `{typography.display-lg}` | 30px | 500 | 1.2 | 0 | Major section headlines |
| `{typography.heading-lg}` | 24px | 600 | 1.33 | 0 | Import workflow and local setup headings |
| `{typography.heading-md}` | 20px | 500 | 1.4 | 0 | Panel titles and card titles |
| `{typography.heading-sm}` | 18px | 500 | 1.56 | 0 | FAQ questions and inspector titles |
| `{typography.body-md}` | 16px | 400 | 1.5 | 0 | Default body, markdown explanations, FAQ answers |
| `{typography.body-strong}` | 16px | 500 | 1.5 | 0 | Inline emphasis and active nav link |
| `{typography.body-sm}` | 14px | 400 | 1.43 | 0 | Metadata, helper copy, footer links |
| `{typography.body-sm-strong}` | 14px | 500 | 1.43 | 0 | Button label, chip label, provider label |
| `{typography.caption-sm}` | 12px | 400 | 1.33 | 0 | Small utility and timestamps |
| `{typography.code-md}` | 16px | 400 | 1.5 | 0 | Output path, command preview, terminal command |
| `{typography.code-sm}` | 14px | 400 | 1.43 | 0 | Terminal output, markdown preview, inline code |
| `{typography.button-md}` | 14px | 500 | 1 | 0 | Button labels |

## Layout

### Spacing System
- **Base unit:** 8px, with 2/4/6px available for tight inline gaps.
- **Major section gap:** `{spacing.section}` (88px) on marketing/product pages.
- **Desktop app gap:** 16-24px between panels; avoid landing-page whitespace inside repeated workflows.
- **Card internal padding:** 24-32px depending on density.
- **FAQ row padding:** 16px vertical with no heavy container.

### Grid & Container
- **Marketing/documentation page:** one narrow reading column at ~720px, with occasional 2-column workflow splits.
- **Desktop shell:** compact sidebar + main workspace. Import, settings, markdown preview, and graph/evidence panels should be directly reachable without a marketing hero.
- **Graph/evidence workspace:** prefer split panes and inspector panels over floating decorative cards.
- **Mobile/web preview:** stack panels vertically; keep primary import action visible.

### Whitespace Philosophy
Whitespace is the design. The page should feel like a Markdown document rendered with care: plain white air between sections, simple headings, short paragraphs, and focused command snippets. Do not use colored bands, floating card stacks, or decorative background treatments to create hierarchy.

## Elevation & Depth

| Level | Treatment | Use |
|---|---|---|
| 0 — Flat | No border, no shadow | Hero, prose sections, footer, simple product explanation |
| 1 — Hairline border | 1px solid `{colors.hairline}` | Import card, terminal preview, evidence panel, FAQ rows |
| 2 — Inverted dark | `{colors.surface-dark}` fill | Terminal/log panel, local-ready proof, one high-attention preview |

There are no drop-shadow-heavy cards. Depth comes from hairline borders, inverted panels, and whitespace.

## Shapes

| Token | Value | Use |
|---|---:|---|
| `{rounded.none}` | 0px | Structural dividers, footer, nav lines |
| `{rounded.sm}` | 6px | Inline code chips and tiny command tags |
| `{rounded.md}` | 8px | Rare dropdown panels |
| `{rounded.lg}` | 12px | Import cards, terminal preview, evidence panels |
| `{rounded.full}` | 9999px | Buttons, path pills, search pills, status chips, traffic-light dots |

The dominant vocabulary is pills for interactive controls and 12px cards for functional panels. Avoid arbitrary medium radii.

## Components

### Buttons

**`button-primary`** — universal HyprDuck action
- Background `{colors.primary}`, text `{colors.on-dark}`, type `{typography.button-md}`, padding `8px 20px`, height `36px`, rounded `{rounded.full}`.
- Used for "Import document", "Start parse", "Retry failed pages", "Open output", and "Download desktop".
- Pressed state: `{colors.ink-deep}` background.

**`button-secondary`** — outline alternative on light canvas
- Background `{colors.canvas}`, text `{colors.ink}`, 1px solid `{colors.hairline-strong}`, type `{typography.button-md}`, padding `8px 20px`, height `36px`, rounded `{rounded.full}`.
- Used for "Cancel", "Reveal in Finder", "Change provider", and lower-priority commands.

**`button-pill-on-dark`** — white pill on dark preview
- Background `{colors.canvas}`, text `{colors.ink}`, type `{typography.button-md}`, rounded `{rounded.full}`.
- Used inside terminal/log panels when a command needs an action.

**`button-disabled`**
- Background `{colors.surface-soft}`, text `{colors.mute}`, rounded `{rounded.full}`.

### Inputs & Forms

**`search-pill`** + **`search-pill-focused`**
- Default: background `{colors.surface-soft}`, text `{colors.ink}`, type `{typography.body-sm}`, padding `8px 16px`, height `36px`, rounded `{rounded.full}`.
- Used for searching imported documents, graph nodes, and markdown snippets.
- Focused: background `{colors.canvas}` with `{colors.focus-ring}`.

**`text-input`** + **`text-input-focused`**
- Default: background `{colors.canvas}`, 1px solid `{colors.hairline}`, type `{typography.body-md}`, padding `8px 16px`, height `40px`, rounded `{rounded.full}`.
- Focused: 1px ink border + `{colors.focus-ring}`.

**`path-pill`** — output or local model path
- Background `{colors.surface-soft}`, text `{colors.ink}`, type `{typography.code-sm}`, padding `8px 14px`, rounded `{rounded.full}`.
- Used for `~/Documents/HyprDuck/...`, local model paths, and parse job identifiers.

**`command-snippet`** — local action preview
- Background `{colors.surface-soft}`, text `{colors.ink}` in `{typography.code-md}`, padding `12px 20px`, min-height `48px`, rounded `{rounded.full}`.
- Examples: `hyprduck parse ./report.pdf`, `local model ready`, `~/Documents/HyprDuck/project.md`.

**`command-tag`**
- Background `{colors.surface-soft}`, text `{colors.ink}` in `{typography.code-sm}`, padding `6px 12px`, rounded `{rounded.full}`.
- Used for short provider/model tags: `local`, `openai`, `anthropic`, `openrouter`, `pdf`, `docx`.

### Cards & Containers

**`terminal-card`** — product proof and local runtime preview
- Container: background `{colors.canvas}`, 1px solid `{colors.hairline}`, padding `{spacing.lg}` (16px), rounded `{rounded.lg}`.
- Header: three `{component.terminal-traffic-lights}` dots.
- Body: parse logs and command output rendered in `{typography.code-sm}` with comments in `{colors.mute}` and active commands in `{colors.ink}`.

**`terminal-traffic-lights`**
- Three 12px filled circles at `{rounded.full}`: `{colors.terminal-red}`, `{colors.terminal-yellow}`, `{colors.terminal-green}`.

**`workflow-card`**
- Container: background `{colors.canvas}`, 1px solid `{colors.hairline}`, padding `{spacing.xxl}` (32px), rounded `{rounded.lg}`.
- Used for Import -> Render -> Analyze -> Save workflow steps.
- Keep text short; pair each card with one icon or command tag, not an illustration.

**`evidence-card`**
- Container: background `{colors.canvas}`, 1px solid `{colors.hairline}`, padding `16px`, rounded `{rounded.lg}`.
- Used for page evidence, markdown snippets, graph node detail, and grounded answer citations.
- Use `{typography.code-sm}` for source paths and `{typography.body-sm}` for snippets.

**`local-ready-card`** — rare inverted proof surface
- Background `{colors.surface-dark}`, text `{colors.on-dark}`, secondary text `{colors.on-dark-mute}`, rounded `{rounded.lg}`.
- Use once per page or view for "Local Mode ready", "Provider connected", or "Output saved locally".

**`faq-row`**
- Background `{colors.canvas}`, padding `16px 0`, 1px bottom border `{colors.hairline}`.
- Question: `{typography.heading-sm}` in `{colors.ink}`.
- Answer: `{typography.body-md}` in `{colors.body}`.
- Always expanded; avoid accordion chrome unless space requires it.

### Inline

**`link-inline`**
- `{colors.ink}` text with underline.

**`link-mute`**
- `{colors.body}` text with underline.

**`status-chip`**
- Background `{colors.surface-soft}`, text `{colors.charcoal}`, rounded `{rounded.full}`.
- Optional green terminal dot only for local-ready state.

### Navigation

**`primary-nav`**
- Background `{colors.canvas}`, text `{colors.ink}`, height 56px, type `{typography.body-sm-strong}`, rounded `{rounded.none}`.
- Layout: HyprDuck mark/name left, links for Product / Docs / GitHub, optional search pill center, right cluster with "Download" or "Open app".

**Desktop sidebar**
- Background `{colors.canvas}` or `{colors.surface-soft}`, 1px right border `{colors.hairline}`.
- Items should use simple icons, body-sm labels, and pill/soft active states.

### Footer

**`footer-section`**
- Background `{colors.canvas}`, 1px top border `{colors.hairline}`, padding `32px 24px`, type `{typography.caption-sm}` `{colors.body}`.
- Links: Download, Docs, GitHub, Privacy, Terms, Contact.

## Do's and Don'ts

### Do
- Treat the design like a local README rendered as an app.
- Use `{component.button-primary}` black pills for primary actions.
- Default to `{rounded.full}` for interactive controls and `{rounded.lg}` for functional panels.
- Render paths, parse commands, provider IDs, and output locations as first-class UI elements.
- Make Local Mode, provider readiness, provider configuration, and output ownership explicit.
- Keep graph/evidence features grounded in visible snippets and source paths.
- Keep product language focused on local document parsing, linked markdown, and agent-ready knowledge.

### Don't
- Don't introduce gradients, atmospheric backgrounds, or decorative color systems.
- Don't add SaaS dashboard chrome that hides the file import workflow.
- Don't make hosted providers feel like the default path; local operation should remain visible.
- Don't imply file parsing requires Screen Recording or Accessibility permissions.
- Don't lift cards with heavy shadows.
- Don't replace native/system typography with expressive brand fonts.
- Don't over-design the duck identity; use it like a small mark, not a mascot-heavy illustration system.

## Responsive Behavior

### Breakpoints

| Name | Width | Key Changes |
|---|---:|---|
| desktop-large | 1280px+ | Default desktop, 720px marketing column, full desktop workspace |
| desktop | 1024px | Same layout; nav remains horizontal |
| tablet | 850px | Workflow cards compress; app panels start stacking |
| tablet-narrow | 768px | Primary nav becomes hamburger; desktop sidebar collapses |
| mobile | 640px | Hero headline drops from 36px to ~28px; command snippets wrap; panels become single column |

### Touch Targets
Interactive elements should sit in the 36-44px height range. Buttons and pills at 36px are acceptable when padding creates an effective larger hit area. Text inputs should be at least 40px high.

### Collapsing Strategy
- **Primary nav:** desktop horizontal -> tablet-narrow hamburger at 768px.
- **Search pill:** desktop fixed width -> mobile icon or full-width overlay.
- **Workflow cards:** 3-up or 2-up -> 1-up stacked.
- **Desktop app panels:** sidebar collapses first; import/markdown/graph panels stack vertically.
- **Command snippets:** wrap command text rather than truncating it.

## Image Behavior

HyprDuck may use a small duck/document mark, but the primary visual vocabulary is text, pills, terminal cards, file paths, markdown previews, and graph/evidence snippets. Treat brand art like a logo, not a hero illustration.

## Iteration Guide

1. Start with the import workflow. If the file picker, local state, parse progress, markdown output, and saved path are not clear, the design is not done.
2. Use component and token names directly (`{colors.primary}`, `{component.button-primary}`, `{rounded.full}`).
3. Use terminal cards for local runtime proof, not as decoration.
4. Keep `{colors.primary}` scarce per viewport. One black primary action per fold is enough.
5. Before adding a new component, ask whether it can be expressed with existing pills, hairline panels, terminal cards, or evidence cards.
6. Prefer source paths and evidence snippets over abstract "AI insight" cards.
7. When adapting this to the desktop app, keep density higher than the marketing page. The app is a repeated-use tool, not a landing page.

## Known Gaps

- Mobile screenshots have not been captured.
- Hover states are intentionally not documented.
- Authenticated/team surfaces are not part of the current product scope.
- Graph workspace details may need additional density rules once relation editing and grounded answers mature.
