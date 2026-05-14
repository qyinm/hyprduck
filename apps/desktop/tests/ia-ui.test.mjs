import { expect, test } from "bun:test";
import { readFileSync } from "node:fs";

const appSource = readFileSync(new URL("../src/App.tsx", import.meta.url), "utf8");
const graphSource = readFileSync(
  new URL("../src/features/workspace/GraphWorkspace.tsx", import.meta.url),
  "utf8",
);
const previewSource = readFileSync(
  new URL("../src/features/workspace/buildWorkspacePreview.ts", import.meta.url),
  "utf8",
);
const stylesSource = readFileSync(new URL("../src/styles.css", import.meta.url), "utf8");
const iaSource = readFileSync(new URL("../IA.md", import.meta.url), "utf8");

test("desktop IA is the committed source of truth for the Knowledge workspace", () => {
  expect(iaSource).toMatch(/Source와 Ask는 destination이 아니라 Knowledge workspace 안의 interaction surface/);
  expect(iaSource).toMatch(/File attachment distinguishes Add to knowledge base from Ask only this time/);
});

test("app shell exposes only Knowledge and Settings as primary destinations", () => {
  expect(appSource).toMatch(/type ActivePanel = "knowledge" \| "settings"/);
  expect(appSource).toMatch(/label: "Knowledge"/);
  expect(appSource).not.toMatch(/label: "Import"/);
  expect(appSource).not.toMatch(/label: "Graph"/);
  expect(appSource).toMatch(/History/);
  expect(appSource).not.toMatch(/Trust Console/);
});

test("app shell exposes fixed window chrome independent of sidebar", () => {
  expect(appSource).toMatch(/fixed left-\[88px\] top-\[10px\]/);
  expect(appSource).toMatch(/fixed right-3 top-\[10px\]/);
  expect(appSource).toMatch(/native titlebar area stays empty/);
  expect(appSource).toMatch(/windowChromeButtonClass/);
  expect(appSource).toMatch(/data-electron-no-drag/);
  expect(appSource).toMatch(/setHistoryOpen\(\(open\) => !open\)/);
  expect(appSource).toMatch(/setSidebarCollapsed\(true\)/);
  expect(appSource).toMatch(/setSidebarCollapsed\(false\)/);
  expect(stylesSource).toMatch(/\[data-electron-no-drag\]/);
  expect(appSource).not.toMatch(/className=\"size-7\"/);
});

test("History panel exposes activity and keeps pending changes secondary", () => {
  expect(appSource).toMatch(/HistoryPanel/);
  expect(appSource).toMatch(/aria-label="History"/);
  expect(appSource).not.toMatch(/Trust Console/);
  expect(appSource).toMatch(/w-\[min\(26rem,calc\(100vw-1\.5rem\)\)\]/);
  expect(appSource).toMatch(/max-h-\[min\(24rem,calc\(100vh-4rem\)\)\]/);
  expect(appSource).toMatch(/Recent activity/);
  expect(appSource).toMatch(/Pending changes/);
  expect(appSource).not.toMatch(/Review Queue/);
  expect(appSource).toMatch(/aria-label=\{`Accept \$\{item\.title\}`\}/);
  expect(appSource).toMatch(/aria-label=\{`Reject \$\{item\.title\}`\}/);
  expect(appSource).not.toMatch(/Needs review/);
  expect(appSource).not.toMatch(/Provenance/);
  expect(appSource).not.toMatch(/Optional review note/);
  expect(appSource).not.toMatch(/Review decision/);
  expect(appSource).not.toMatch(/Accept applies the durable save-back/);
  expect(appSource).not.toMatch(/Recent brain events/);
  expect(appSource).toMatch(/resolve_brain_review/);
  expect(appSource).toMatch(/formatEventType/);
  expect(appSource).toMatch(/formatProposalKind/);
});

test("desktop visual tokens follow DESIGN.md restraint", () => {
  expect(stylesSource).toMatch(/--primary: oklch\(0 0 0\)/);
  expect(stylesSource).not.toMatch(/0\.55 0\.28 300/);
  expect(stylesSource).not.toMatch(/fontsource-variable\/geist/);
  expect(graphSource).not.toMatch(/teal|gradient/);
});

test("Knowledge empty state focuses first users on importing source files", () => {
  expect(graphSource).toMatch(/Your knowledge base is empty/);
  expect(graphSource).toMatch(/Drop PDF, DOCX, or DOC files here/);
  expect(graphSource).toMatch(/Choose files/);
  expect(graphSource).not.toMatch(/Add files or ask about your knowledge/);
  expect(graphSource).not.toMatch(/Go to Import/);
  expect(graphSource).not.toMatch(/compile|compiled|compiler/i);
  expect(previewSource).not.toMatch(/compile|compiled|compiler/i);
});

test("Graph workspace centers the canvas with inspector actions", () => {
  expect(graphSource).toMatch(/SigmaGraphCanvas/);
  expect(graphSource).toMatch(/GraphPromptComposer/);
  expect(graphSource).toMatch(/Source Detail/);
  expect(graphSource).toMatch(/Source file/);
  expect(graphSource).toMatch(/Raw markdown/);
  expect(graphSource).toMatch(/Open source copy/);
  expect(graphSource).toMatch(/Open raw markdown/);
  expect(graphSource).toMatch(/Reveal in Finder/);
  expect(graphSource).toMatch(/Right inspector/);
  expect(graphSource).toMatch(/without leaving the graph/);
});

test("bottom prompt composer supports attachment intent and source metadata", () => {
  expect(graphSource).toMatch(/Ask or add files to this knowledge base/);
  expect(graphSource).toMatch(/Add to knowledge base/);
  expect(graphSource).toMatch(/Ask only this time/);
  expect(graphSource).toMatch(/File description/);
  expect(graphSource).toMatch(/\+ Attach files/);
  expect(graphSource).toMatch(/source\.description|source metadata/);
});

test("evidence is rendered as UI content instead of raw markdown", () => {
  expect(graphSource).toMatch(/formatEvidenceSnippet/);
  expect(graphSource).toMatch(/extractMarkdownImageLabel/);
  expect(graphSource).toMatch(/Page image:/);
});

test("workspace graph reader loads the latest materialized snapshot first", () => {
  const mainSource = readFileSync(new URL("../main.cjs", import.meta.url), "utf8");

  expect(mainSource).toMatch(/case "load_materialized_graph_snapshot"/);
  expect(mainSource).toMatch(/command: "read_graph_snapshot"/);
  expect(appSource).toMatch(/materializedGraphSnapshotToWorkspaceEnvelope/);
  expect(appSource).toMatch(/loadGraphWorkspaceEnvelope/);
  expect(appSource).toMatch(/loadGraphWorkspaceEnvelopeResult/);
  expect(appSource).toMatch(/load_materialized_graph_snapshot/);
  expect(appSource).toMatch(/load_workspace_project/);
  expect(appSource).toMatch(/const nextLoad = await loadGraphWorkspaceEnvelopeResult/);
  expect(appSource).toMatch(/setLoadedWorkspaceEnvelope\(nextLoad\.envelope\)/);
  expect(appSource).toMatch(/\}, \[workspaceProject\]\);/);
  expect(appSource).not.toMatch(/setLoadedWorkspaceEnvelope\(\(current\) => \(\{\s*project,/);
});

test("workspace snapshot refresh exposes loading fallback and error states", () => {
  expect(appSource).toMatch(/type WorkspaceLoadStatus = "idle" \| "loading" \| "ready" \| "fallback" \| "error"/);
  expect(appSource).toMatch(/loadGraphWorkspaceEnvelopeResult/);
  expect(appSource).toMatch(/setWorkspaceLoadState\(\{\s*status: "loading"/);
  expect(appSource).toMatch(/workspaceLoadStateFromResult\(result\)/);
  expect(appSource).toMatch(/workspaceLoadStateFromResult\(initialWorkspaceLoad\)/);
  expect(appSource).toMatch(/status: "fallback"/);
  expect(appSource).toMatch(/status: "error"/);
  expect(appSource).toMatch(/WorkspaceSnapshotStatusBanner/);
  expect(appSource).toMatch(/Refreshing latest workspace snapshot/);
  expect(appSource).toMatch(/Could not refresh the workspace snapshot/);
});
