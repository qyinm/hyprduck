import { expect, test } from "bun:test";
import { readFileSync } from "node:fs";

const appSource = readFileSync(new URL("../src/App.tsx", import.meta.url), "utf8");
const graphSource = readFileSync(
  new URL("../src/features/workspace/GraphWorkspace.tsx", import.meta.url),
  "utf8",
);
const agentTerminalSource = readFileSync(
  new URL("../src/features/agent-terminal/AgentTerminal.tsx", import.meta.url),
  "utf8",
);
const appTypesSource = readFileSync(new URL("../src/appTypes.ts", import.meta.url), "utf8");
const previewSource = readFileSync(
  new URL("../src/features/workspace/buildWorkspacePreview.ts", import.meta.url),
  "utf8",
);
const settingsSource = readFileSync(new URL("../src/SettingsPanel.tsx", import.meta.url), "utf8");
const localesSource = readFileSync(new URL("../src/i18n/locales.ts", import.meta.url), "utf8");
const previewApiSource = readFileSync(new URL("../src/webPreviewApi.ts", import.meta.url), "utf8");
const cliShimSource = readFileSync(new URL("../main/cli-shim.cjs", import.meta.url), "utf8");
const stylesSource = readFileSync(new URL("../src/styles.css", import.meta.url), "utf8");
const iaSource = readFileSync(new URL("../IA.md", import.meta.url), "utf8");
const modelTaskMatrixSource = readFileSync(
  new URL("../../../docs/model-task-matrix.md", import.meta.url),
  "utf8",
);

test("desktop IA is the committed source of truth for the Knowledge workspace", () => {
  expect(iaSource).toMatch(/Source와 Ask는 destination이 아니라 Knowledge workspace 안의 interaction surface/);
  expect(iaSource).toMatch(/File attachment distinguishes Add to knowledge base from Ask only this time/);
});

test("app shell exposes only Knowledge and Settings as primary destinations", () => {
  expect(appSource).toMatch(/type ActivePanel = "knowledge" \| "settings"/);
  expect(appSource).toMatch(/label: "Knowledge"/);
  expect(appSource).not.toMatch(/label: "Import"/);
  expect(appSource).not.toMatch(/label: "Graph"/);
  expect(appSource).not.toMatch(/History/);
});

test("app shell exposes fixed window chrome independent of sidebar", () => {
  expect(appSource).toMatch(/fixed left-\[88px\] top-\[10px\]/);
  expect(appSource).toMatch(/fixed right-3 top-\[10px\]/);
  expect(appSource).toMatch(/native titlebar area stays empty/);
  expect(appSource).toMatch(/windowChromeButtonClass/);
  expect(appSource).toMatch(/data-electron-no-drag/);
  expect(appSource).toMatch(/setSidebarCollapsed\(true\)/);
  expect(appSource).toMatch(/setSidebarCollapsed\(false\)/);
  expect(stylesSource).toMatch(/\[data-electron-no-drag\]/);
  expect(appSource).not.toMatch(/className=\"size-7\"/);
});

test("app shell removes the History surface from the titlebar", () => {
  expect(appSource).not.toMatch(/HistoryPanel/);
  expect(appSource).not.toMatch(/aria-label="History"/);
  expect(appSource).not.toMatch(/Recent activity/);
  expect(appSource).not.toMatch(/setHistoryOpen/);
  expect(appSource).not.toMatch(/brain_health/);
  expect(appSource).not.toMatch(/Pending changes/);
  expect(appSource).not.toMatch(/Review Queue/);
  expect(appSource).not.toMatch(/Change proposed|Change resolved/);
});

test("desktop visual tokens follow DESIGN.md restraint", () => {
  expect(stylesSource).toMatch(/--primary: oklch\(0 0 0\)/);
  expect(stylesSource).not.toMatch(/0\.55 0\.28 300/);
  expect(stylesSource).not.toMatch(/fontsource-variable\/geist/);
  expect(graphSource).not.toMatch(/teal|gradient/);
});

test("settings page hides debug readiness internals", () => {
  expect(settingsSource).toMatch(/settings\.ai\.title/);
  expect(settingsSource).toMatch(/settings\.ai\.connections/);
  expect(localesSource).toMatch(/AI model/);
  expect(localesSource).toMatch(/Connections/);
  expect(settingsSource).toMatch(/settings\.general\.uiLanguage/);
  expect(settingsSource).toMatch(/localeOptions\.map/);
  expect(localesSource).toMatch(/English/);
  expect(localesSource).toMatch(/한국어/);
  expect(localesSource).toMatch(/日本語/);
  expect(localesSource).toMatch(/settings\.ai\.localModelCaution\.title/);
  expect(localesSource).toMatch(/settings\.ai\.hostedQuality\.body/);
  expect(settingsSource).toMatch(/onRefreshReadiness/);
  expect(localesSource).toMatch(/Refresh/);
  expect(settingsSource).toMatch(/export type SettingsTab = "general" \| "ai"/);
  expect(appSource).toMatch(/label: "General"/);
  expect(settingsSource).not.toMatch(/<Label htmlFor="prompt-template-select">/);
  expect(settingsSource).not.toMatch(/Configure prompt templates and output behavior/);
  expect(settingsSource).not.toMatch(/Document processing/);
  expect(settingsSource).not.toMatch(/Runtime readiness/);
  expect(settingsSource).not.toMatch(/\(readiness\?\.checks \?\? \[\]\)\.map/);
  expect(settingsSource).not.toMatch(/check\.message/);
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

test("launch copy stays agent-ready without unsupported provider claims", () => {
  const buyerFacingCopy = [appSource, graphSource, settingsSource, localesSource].join("\n");
  const publicModelGuidance = modelTaskMatrixSource;

  expect(buyerFacingCopy).toMatch(/Local model caution/);
  expect(buyerFacingCopy).toMatch(/Hosted quality path/);
  expect(buyerFacingCopy).toMatch(/agent-ready outputs/);
  expect(buyerFacingCopy).toMatch(/source-backed evidence that coding agents can reuse with citations/);
  expect(buyerFacingCopy).not.toMatch(/DeepSeek-only|generic PDF chat|PDF chat/);
  expect(buyerFacingCopy).not.toMatch(/Context Pack v0/);
  expect(buyerFacingCopy).not.toMatch(/OpenAI|Anthropic/);
  expect(buyerFacingCopy).not.toMatch(/project memory output|generated graph output/);
  expect(publicModelGuidance).toMatch(/local document parsing and agent-ready evidence reuse/);
  expect(publicModelGuidance).not.toMatch(/brain-corpus|local brain|Graph materialization|Provider-generated graph records|generated graph/);
});

test("Knowledge workspace keeps the graph canvas and removes onboarding checklist chrome", () => {
  expect(graphSource).toMatch(/SigmaGraphCanvas/);
  expect(graphSource).toMatch(/aria-label="Open Agent Terminal"/);
  expect(graphSource).toMatch(/placeholder="Open Agent Terminal\.\.\."/);
  expect(graphSource).not.toMatch(/FirstRunActivationStrip/);
  expect(graphSource).not.toMatch(/aria-label="First-run activation"/);
  expect(graphSource).not.toMatch(/Register the local MCP server/);
  expect(graphSource).not.toMatch(/Ask a second question from the same source set/);
  expect(graphSource).not.toMatch(/Screen Recording|Accessibility/);
});

test("Graph workspace centers the canvas with inspector actions", () => {
  expect(graphSource).toMatch(/SigmaGraphCanvas/);
  expect(graphSource).toMatch(/GraphPromptComposer/);
  expect(graphSource).toMatch(/Document/);
  expect(graphSource).toMatch(/File/);
  expect(graphSource).toMatch(/aria-label="Open file"/);
  expect(graphSource).toMatch(/aria-label="Open extracted text"/);
  expect(graphSource).toMatch(/aria-label="Reveal in Finder"/);
  expect(graphSource).toMatch(/ExternalLink/);
  expect(graphSource).toMatch(/FileText/);
  expect(graphSource).toMatch(/FolderOpen/);
  expect(graphSource).toMatch(/Review suggestions/);
  expect(graphSource).toMatch(/selectedNode\.evidence\.slice\(0, 3\)/);
  expect(graphSource).toMatch(/workspaceSelectionKindLabel/);
  expect(graphSource).toMatch(/customerVisibleDescription/);
  expect(graphSource).not.toMatch(/<Badge variant="outline">\{selectedNode\.node\.kind\}<\/Badge>/);
  expect(graphSource).not.toMatch(/graphMaterializationSummary/);
  expect(graphSource).not.toMatch(/projectionSummary/);
});

test("bottom prompt composer opens Agent Terminal from focus or submit", () => {
  expect(graphSource).toMatch(/GraphPromptComposer/);
  expect(graphSource).toMatch(/AgentTerminal/);
  expect(graphSource).toMatch(/GraphAnswerWindow/);
  expect(graphSource).toMatch(/aria-label="Attach files"/);
  expect(graphSource).toMatch(/onFocus=\{openTerminal\}/);
  expect(graphSource).toMatch(/onOpenAgentTerminal\(\);/);
  expect(graphSource).toMatch(/onCreateSession=\{onCreateAgentTerminalSession\}/);
  expect(agentTerminalSource).toMatch(/aria-label="Minimize Agent Terminal"/);
  expect(graphSource).toMatch(/aria-label="Resize Agent Terminal"/);
  expect(graphSource).toMatch(/aria-label="Restore Agent Terminal"/);
  expect(graphSource).not.toMatch(/onAskProject/);
  expect(graphSource).not.toMatch(/answer_workspace_project/);
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

  expect(mainSource).toMatch(/ensureHyprduckShellCommand\(app\)/);
  expect(mainSource).toMatch(/require\("\.\/main\/cli-shim\.cjs"\)/);
  expect(cliShimSource).toMatch(/function resolveCliPath\(\)/);
  expect(cliShimSource).toMatch(/HYPRDUCK_CLI_BIN/);
  expect(cliShimSource).toMatch(/HYPRDUCK_INSTALL_CLI_SHIM/);
  expect(cliShimSource).toMatch(/isManagedHyprduckCliTarget/);
  expect(cliShimSource).toMatch(/isDirectoryOnPath/);
  expect(cliShimSource).toMatch(/existing-symlink/);
  expect(cliShimSource).toMatch(/"\.local", "bin"/);
  expect(cliShimSource).toMatch(/"hyprduck"/);
  expect(cliShimSource).toMatch(/fs\.symlinkSync\(cliPath, shimPath\)/);
});

test("desktop import jobs use HyprDuck citation lifecycle states", () => {
  const mainSource = readFileSync(new URL("../main.cjs", import.meta.url), "utf8");
  const activeJobStatusAssignments = [
    ...mainSource.matchAll(/snapshot\.activeJob\.status\s*=\s*"([^"]+)"/g),
  ].map((match) => match[1]);
  const activeJobInitializers = [
    ...mainSource.matchAll(/snapshot\.activeJob\s*=\s*\{[\s\S]*?status:\s*"([^"]+)"/g),
  ].map((match) => match[1]);
  const graphRebuildStatusPatches = [
    ...mainSource.matchAll(/updateActiveGraphRebuildJob\([^)]*\{\s*status:\s*"([^"]+)"/g),
  ].map((match) => match[1]);
  const importJobStatuses = [
    ...activeJobStatusAssignments,
    ...activeJobInitializers,
    ...graphRebuildStatusPatches,
  ];

  expect(appTypesSource).toMatch(
    /type ImportJobLifecycleStatus =\s*\|\s*"imported"\s*\|\s*"parsing"\s*\|\s*"packaging"\s*\|\s*"citation_ready"\s*\|\s*"context_ready"\s*\|\s*"failed"\s*\|\s*"cancelled"\s*\|\s*"partial"/,
  );
  expect(importJobStatuses).toContain("imported");
  expect(importJobStatuses).toContain("parsing");
  expect(importJobStatuses).toContain("packaging");
  expect(importJobStatuses).toContain("citation_ready");
  expect(importJobStatuses).toContain("context_ready");
  expect(importJobStatuses).not.toContain("queued");
  expect(importJobStatuses).not.toContain("running");
  expect(importJobStatuses).not.toContain("completed");
});

test("desktop provider validation preserves issue codes", () => {
  expect(appTypesSource).toMatch(/interface ValidationIssue \{\s*code: string;\s*message: string;\s*\}/);
  expect(previewApiSource).toMatch(/code: "provider_config"/);
  expect(previewApiSource).toMatch(/validation\.issues\.map\(\(issue\) => issue\.message\)/);
});

test("workspace snapshot refresh exposes loading fallback and error states", () => {
  expect(appTypesSource).toMatch(/type WorkspaceLoadStatus =\s*\|\s*"idle"\s*\|\s*"loading"\s*\|\s*"ready"\s*\|\s*"fallback"\s*\|\s*"error"/);
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
  expect(appSource).toMatch(/invoke\("retry_failed_pages"\)/);
  const mainSource = readFileSync(new URL("../main.cjs", import.meta.url), "utf8");
  expect(mainSource).toMatch(/case "retry_failed_pages"/);
  expect(mainSource).toMatch(/function retryFailedPages/);
  expect(mainSource).toMatch(/command: "retry_failed_pages"/);
  expect(mainSource).toMatch(/sourceManifestPath: manifestPath/);
  expect(mainSource).not.toMatch(/onRetryFailedPages=\{startParse\}/);
});

test("graph import banner names citation-ready and context-ready states", () => {
  expect(graphSource).toMatch(/formatImportLifecycleTitle/);
  expect(graphSource).toMatch(/Citation-ready/);
  expect(graphSource).toMatch(/Context-ready/);
  expect(graphSource).toMatch(/Packaging citations/);
  expect(graphSource).not.toMatch(/Importing source file/);
});

test("MCP docs describe import lifecycle states", () => {
  const mcpDocs = readFileSync(new URL("../../../docs/mcp.md", import.meta.url), "utf8");
  const agentMcpDocs = readFileSync(
    new URL("../../../docs/agents/mcp-client-setup.md", import.meta.url),
    "utf8",
  );
  const docs = `${mcpDocs}\n${agentMcpDocs}`;

  expect(docs).toMatch(/imported -> parsing -> packaging -> citation_ready -> context_ready/);
  expect(docs).toMatch(/citation_ready/);
  expect(docs).toMatch(/context_ready/);
  expect(docs).not.toMatch(/poll `import_status` until the job is completed/);
});
