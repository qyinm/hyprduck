import { expect, test } from "bun:test";
import { readFileSync } from "node:fs";

const appSource = readFileSync(new URL("../src/App.tsx", import.meta.url), "utf8");
const historyPanelSource = appSource.slice(
  appSource.indexOf("function HistoryPanel"),
  appSource.indexOf("function formatEventType"),
);
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

test("History panel exposes recent activity without graph review controls", () => {
  expect(appSource).toMatch(/HistoryPanel/);
  expect(historyPanelSource).toMatch(/aria-label="History"/);
  expect(historyPanelSource).toMatch(/w-\[min\(26rem,calc\(100vw-1\.5rem\)\)\]/);
  expect(historyPanelSource).toMatch(/max-h-\[min\(24rem,calc\(100vh-4rem\)\)\]/);
  expect(historyPanelSource).toMatch(/Recent activity/);
  expect(historyPanelSource).toMatch(/recentEvents\.filter\(isHistoryActivityEvent\)/);
  expect(historyPanelSource).not.toMatch(/recentEvents\.map/);
  expect(historyPanelSource).not.toMatch(/Pending changes/);
  expect(historyPanelSource).not.toMatch(/Review Queue/);
  expect(historyPanelSource).not.toMatch(/aria-label=\{`Accept \$\{item\.title\}`\}/);
  expect(historyPanelSource).not.toMatch(/aria-label=\{`Reject \$\{item\.title\}`\}/);
  expect(appSource).not.toMatch(/brainHealth && brainHealth\.attentionCount > 0/);
  expect(appSource).not.toMatch(/Change proposed|Change resolved/);
  expect(historyPanelSource).toMatch(/formatEventType/);
});

test("desktop visual tokens follow DESIGN.md restraint", () => {
  expect(stylesSource).toMatch(/--primary: oklch\(0 0 0\)/);
  expect(stylesSource).not.toMatch(/0\.55 0\.28 300/);
  expect(stylesSource).not.toMatch(/fontsource-variable\/geist/);
  expect(graphSource).not.toMatch(/teal|gradient/);
});

test("Knowledge empty state focuses first users on importing source files", () => {
  expect(graphSource).toMatch(/Add private docs/);
  expect(graphSource).toMatch(/Drop PDF, DOCX, or DOC files here/);
  expect(graphSource).toMatch(/Choose files/);
  expect(graphSource).toMatch(/source-backed evidence that coding agents can reuse with citations/);
  expect(graphSource).not.toMatch(/Add files or ask about your knowledge/);
  expect(graphSource).not.toMatch(/Go to Import/);
  expect(graphSource).not.toMatch(/compile|compiled|compiler/i);
  expect(previewSource).not.toMatch(/compile|compiled|compiler/i);
});

test("first-run activation presents document-agent-citation flow before graph controls", () => {
  expect(graphSource).toMatch(/FirstRunActivationStrip/);
  expect(graphSource).toMatch(/aria-label="First-run activation"/);
  expect(graphSource).toMatch(/Add Docs/);
  expect(graphSource).toMatch(/Connect Agent/);
  expect(graphSource).toMatch(/Ask With Citations/);
  expect(graphSource).toMatch(/Verify Evidence/);
  expect(graphSource).toMatch(/Reuse/);
  expect(graphSource).toMatch(/Register the local MCP server/);
  expect(graphSource).toMatch(/Open source, page, and evidence refs/);
  expect(graphSource).toMatch(/Ask a second question from the same source set/);
  expect(graphSource).toMatch(/activeStep=\{\s*project\.summary\.documentCount > 0 \? "connect" : "add"\s*\}/);
  expect(graphSource).toMatch(/id: "add",[\s\S]*?complete: hasImport/);
  expect(graphSource).toMatch(/id: "connect",[\s\S]*?complete: false/);
  expect(graphSource).toMatch(/id: "ask",[\s\S]*?complete: false/);
  expect(graphSource).toMatch(/id: "verify",[\s\S]*?complete: false/);
  expect(graphSource).toMatch(/id: "reuse",[\s\S]*?complete: false/);
  expect(graphSource).toMatch(/aria-label="Ask with citations"/);
  expect(graphSource).toMatch(/placeholder="Ask with citations\.\.\."/);
  expect(graphSource).not.toMatch(/Screen Recording|Accessibility/);
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

test("bottom prompt composer opens a floating answer window above the prompt", () => {
  expect(graphSource).toMatch(/GraphPromptComposer/);
  expect(graphSource).toMatch(/GraphAnswerWindow/);
  expect(graphSource).toMatch(/aria-label="Attach files"/);
  expect(graphSource).toMatch(/dispatch\(\{ type: "open_answer_dock" \}\)/);
  expect(graphSource).toMatch(/bottom-24/);
  expect(graphSource).toMatch(/Close answer/);
  expect(graphSource).toMatch(/Answering\.\.\./);
  expect(graphSource).toMatch(/CompactEvidenceRow/);
  expect(graphSource).not.toMatch(/Ask or add files to this knowledge base/);
  expect(graphSource).not.toMatch(/name="attachment-intent"/);
  expect(graphSource).not.toMatch(/Ask workspace graph or attach files/);
  expect(graphSource).not.toMatch(/Live answer state|Stored answer state/);
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

test("desktop app prepares the short HyprDuck MCP shell command", () => {
  const mainSource = readFileSync(new URL("../main.cjs", import.meta.url), "utf8");

  expect(mainSource).toMatch(/ensureHyprduckShellCommand\(\)/);
  expect(mainSource).toMatch(/function resolveCliPath\(\)/);
  expect(mainSource).toMatch(/HYPRDUCK_CLI_BIN/);
  expect(mainSource).toMatch(/HYPRDUCK_INSTALL_CLI_SHIM/);
  expect(mainSource).toMatch(/isManagedHyprduckCliTarget/);
  expect(mainSource).toMatch(/isDirectoryOnPath/);
  expect(mainSource).toMatch(/existing-symlink/);
  expect(mainSource).toMatch(/"\.local", "bin"/);
  expect(mainSource).toMatch(/"hyprduck"/);
  expect(mainSource).toMatch(/fs\.symlinkSync\(cliPath, shimPath\)/);
});

test("desktop provider validation preserves issue codes", () => {
  expect(appSource).toMatch(/interface ValidationIssue \{\s*code: string;\s*message: string;\s*\}/);
  expect(appSource).toMatch(/code: "provider_config"/);
  expect(appSource).toMatch(/validation\.issues\.map\(\(issue\) => issue\.message\)/);
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

test("partial import failures expose failed-page retry affordance", () => {
  expect(appSource).toMatch(/failedPageCount: snapshot\.lastResult\?\.failedCount/);
  expect(appSource).toMatch(/snapshot\.lastResult && snapshot\.lastResult\.failedCount > 0/);
  expect(appSource).toMatch(/status: "partial"/);
  expect(graphSource).toMatch(/failedPageCount/);
  expect(graphSource).toMatch(/const partial = status\.status === "partial"/);
  expect(graphSource).toMatch(/canRetryFailedPages = failedPageCount > 0/);
  expect(graphSource).toMatch(/Retry failed pages/);
  expect(graphSource).toMatch(/failedPageCount === 1 \? "page" : "pages"/);
  expect(graphSource).toMatch(/onRetryFailedPages/);
  expect(appSource).toMatch(/invoke<void>\("retry_failed_pages"\)/);
  const mainSource = readFileSync(new URL("../main.cjs", import.meta.url), "utf8");
  expect(mainSource).toMatch(/case "retry_failed_pages"/);
  expect(mainSource).toMatch(/function retryFailedPages/);
  expect(mainSource).toMatch(/command: "retry_failed_pages"/);
  expect(mainSource).toMatch(/sourceManifestPath: manifestPath/);
  expect(mainSource).not.toMatch(/onRetryFailedPages=\{startParse\}/);
});
