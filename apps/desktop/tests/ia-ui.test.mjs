import { expect, test } from "bun:test";
import { readFileSync } from "node:fs";

const appSource = readFileSync(new URL("../src/App.tsx", import.meta.url), "utf8");
const graphSource = readFileSync(
  new URL("../src/features/workspace/GraphWorkspace.tsx", import.meta.url),
  "utf8",
);
const docsSource = readFileSync(
  new URL("../src/features/workspace/DocsWorkspace.tsx", import.meta.url),
  "utf8",
);
const agentChatSource = readFileSync(
  new URL("../src/features/agent-chat/AgentChatWorkspace.tsx", import.meta.url),
  "utf8",
);
const aiElementsMessageSource = readFileSync(
  new URL("../src/components/ai-elements/message.tsx", import.meta.url),
  "utf8",
);
const aiElementsInlineCitationSource = readFileSync(
  new URL("../src/components/ai-elements/inline-citation.tsx", import.meta.url),
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
const mainSource = readFileSync(new URL("../main.cjs", import.meta.url), "utf8");
const packageSource = readFileSync(new URL("../package.json", import.meta.url), "utf8");
const preloadSource = readFileSync(new URL("../preload.cjs", import.meta.url), "utf8");

test("desktop IA is the committed source of truth for Docs, Agent, and Graph", () => {
  expect(iaSource).toMatch(/`Docs \/ Agent \/ Graph` 세 destination/);
  expect(iaSource).toMatch(/Agent는 terminal surface가 아니라 thread list와 central composer/);
  expect(iaSource).toMatch(/source count, Ready\/Setup, node count 같은 상태 배지를 넣지 않는다/);
});

test("app shell exposes Docs, Agent, Graph, and Settings as primary destinations", () => {
  expect(appSource).toMatch(/type MainPanel = "docs" \| "agent" \| "graph"/);
  expect(appSource).toMatch(/type ActivePanel = MainPanel \| "settings"/);
  expect(appSource).toMatch(/labelKey: "nav\.docs"/);
  expect(appSource).toMatch(/labelKey: "nav\.agent"/);
  expect(appSource).toMatch(/labelKey: "nav\.graph"/);
  expect(appSource).toMatch(/t\("nav\.settings"\)/);
  expect(localesSource).toMatch(/"nav\.docs": "Docs"/);
  expect(localesSource).toMatch(/"nav\.agent": "Agent"/);
  expect(localesSource).toMatch(/"nav\.graph": "Graph"/);
  expect(appSource).toMatch(/useState<ActivePanel>\("docs"\)/);
  expect(appSource).not.toMatch(/navBadgeForPanel/);
  expect(appSource).not.toMatch(/Ready"\s*:\s*"Setup/);
  expect(appSource).not.toMatch(/\$\{graphNodeCount\} nodes/);
  expect(appSource).not.toMatch(/label: "Import"/);
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
  expect(appSource).toMatch(/labelKey: "nav\.general"/);
  expect(settingsSource).not.toMatch(/<Label htmlFor="prompt-template-select">/);
  expect(settingsSource).not.toMatch(/Configure prompt templates and output behavior/);
  expect(settingsSource).not.toMatch(/Document processing/);
  expect(settingsSource).not.toMatch(/Runtime readiness/);
  expect(settingsSource).not.toMatch(/\(readiness\?\.checks \?\? \[\]\)\.map/);
  expect(settingsSource).not.toMatch(/check\.message/);
});

test("Docs page focuses first users on importing source files", () => {
  expect(appSource).toMatch(/<DocsWorkspace/);
  expect(docsSource).toMatch(/Add Sources/);
  expect(docsSource).toMatch(/Supported: PDF, DOCX, DOC/);
  expect(docsSource).toMatch(/Import Queue/);
  expect(docsSource).toMatch(/Parse Warnings/);
  expect(docsSource).toMatch(/onViewInGraph/);
  expect(docsSource).toMatch(/Details/);
  expect(docsSource).toMatch(/FileSearch/);
  expect(docsSource).toMatch(/Waypoints/);
  expect(docsSource).toMatch(/View in Graph/);
  expect(docsSource).toMatch(/SourceDetailWorkspace/);
  expect(docsSource).toMatch(/Original/);
  expect(docsSource).toMatch(/Parsed Markdown/);
  expect(docsSource).toMatch(/Document/);
  expect(docsSource).toMatch(/Page/);
  expect(docsSource).toMatch(/useState<MarkdownViewMode>\("raw"\)/);
  expect(docsSource).toMatch(/visiblePageRange/);
  expect(docsSource).toMatch(/visiblePageNumbers/);
  expect(docsSource).toMatch(/pageNumber=\{pageNumber\}/);
  expect(docsSource).toMatch(/onScroll=\{updateVisiblePages\}/);
  expect(docsSource).not.toMatch(/Array\.from\(\{ length: numPages \}/);
  expect(docsSource).toMatch(/pdfjs\.GlobalWorkerOptions\.workerSrc/);
  expect(packageSource).toMatch(/"react-pdf": "\^10\.4\.1"/);
  expect(packageSource).toMatch(/"pdfjs-dist": "5\.4\.296"/);
  expect(docsSource).toMatch(/top-12 z-\[60\]/);
  expect(docsSource).toMatch(/data-electron-no-drag/);
  expect(docsSource).not.toMatch(/type="application\/pdf"/);
  expect(docsSource).toMatch(/Preview/);
  expect(docsSource).toMatch(/Raw/);
  expect(docsSource).toMatch(/Copy/);
  expect(docsSource).toMatch(/MessageResponse/);
  expect(docsSource).not.toMatch(/Open extracted text/);
  expect(docsSource).toMatch(/Reveal in Finder/);
  expect(docsSource).not.toMatch(/Filter/);
  expect(docsSource).not.toMatch(/readOnly/);
  expect(docsSource).toMatch(/value=\{sourceSearch\}/);
  expect(docsSource).toMatch(/visibleSources\.map/);
  expect(docsSource).toMatch(/normalize\("NFC"\)/);
  expect(appSource).toMatch(/onReadSourceDetail=\{readSourceDetail\}/);
  expect(appSource).toMatch(/invoke\("read_source_detail"/);
  expect(appTypesSource).toMatch(/interface SourceDetailResult/);
  expect(appTypesSource).toMatch(/read_source_detail/);
  expect(mainSource).toMatch(/case "read_source_detail"/);
  expect(mainSource).toMatch(/resolveKnownWorkspacePath\(candidatePath\)/);
  expect(mainSource).toMatch(/readOriginalPreview\(\[originalPath, sourcePath\]/);
  expect(mainSource).toMatch(/resolveFirstKnownWorkspacePath/);
  expect(mainSource).toMatch(/hyprduck-source/);
  expect(docsSource).toMatch(/previewableSourcePath\(source\)/);
  expect(previewApiSource).toMatch(/read_source_detail/);
  expect(localesSource).toMatch(/Add private docs/);
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

test("Graph workspace keeps the graph canvas and removes chat/import chrome", () => {
  expect(graphSource).toMatch(/SigmaGraphCanvas/);
  expect(graphSource).toMatch(/onOpenDocs/);
  expect(graphSource).not.toMatch(/AgentTerminal/);
  expect(graphSource).not.toMatch(/GraphPromptComposer/);
  expect(graphSource).not.toMatch(/workspace\.prompt\.openTerminal/);
  expect(graphSource).not.toMatch(/workspace\.prompt\.placeholder/);
  expect(graphSource).not.toMatch(/FirstRunActivationStrip/);
  expect(graphSource).not.toMatch(/aria-label="First-run activation"/);
  expect(graphSource).not.toMatch(/Register the local MCP server/);
  expect(graphSource).not.toMatch(/Ask a second question from the same source set/);
  expect(graphSource).not.toMatch(/Screen Recording|Accessibility/);
});

test("Graph workspace centers the canvas with inspector actions", () => {
  expect(graphSource).toMatch(/SigmaGraphCanvas/);
  expect(graphSource).toMatch(/Document/);
  expect(graphSource).toMatch(/File/);
  expect(graphSource).toMatch(/workspace\.inspector\.openFile/);
  expect(graphSource).toMatch(/workspace\.inspector\.openExtractedText/);
  expect(graphSource).toMatch(/workspace\.inspector\.revealInFinder/);
  expect(graphSource).toMatch(/ExternalLink/);
  expect(graphSource).toMatch(/FileText/);
  expect(graphSource).toMatch(/FolderOpen/);
  expect(graphSource).toMatch(/Trash2/);
  expect(graphSource).not.toMatch(/workspace\.inspector\.reviewSuggestions/);
  expect(graphSource).toMatch(/selectedNode\.evidence\.slice\(0, 3\)/);
  expect(graphSource).toMatch(/workspaceSelectionKindLabel/);
  expect(graphSource).toMatch(/customerVisibleDescription/);
  expect(graphSource).not.toMatch(/<Badge variant="outline">\{selectedNode\.node\.kind\}<\/Badge>/);
  expect(graphSource).not.toMatch(/graphMaterializationSummary/);
  expect(graphSource).not.toMatch(/projectionSummary/);
});

test("Agent page renders chat UI and uses the streaming agent chat IPC contract", () => {
  expect(appSource).toMatch(/<AgentChatWorkspace/);
  expect(agentChatSource).toMatch(/STORAGE_KEY = "hyprduck\.agentChatThreads\.v4"/);
  expect(agentChatSource).toMatch(/window\.localStorage/);
  expect(agentChatSource).toMatch(/What should we work on\?/);
  expect(agentChatSource).toMatch(/mode: "auto"/);
  expect(agentChatSource).not.toMatch(/setScopeMode/);
  expect(agentChatSource).not.toMatch(/Selected source/);
  expect(agentChatSource).not.toMatch(/Graph context/);
  expect(agentChatSource).toMatch(/onStartAgentChat/);
  expect(agentChatSource).toMatch(/onStopAgentChat/);
  expect(agentChatSource).toMatch(/onListenAgentChatEvents/);
  expect(agentChatSource).not.toMatch(/graph_context/);
  expect(agentChatSource).toMatch(/selectedNodeId: null/);
  expect(agentChatSource).not.toMatch(/selectedNodeId \?/);
  expect(agentChatSource).toMatch(/Trash2/);
  expect(agentChatSource).toMatch(/deleteThread/);
  expect(agentChatSource).toMatch(/Delete chat \$\{thread\.title\}/);
  expect(agentChatSource).toMatch(/removeKeys/);
  expect(agentChatSource).toMatch(/event\.type === "delta"/);
  expect(agentChatSource).toMatch(/event\.type === "stopped"/);
  expect(agentChatSource).toMatch(/hasConversation/);
  expect(agentChatSource).toMatch(/Ask a follow-up about your indexed docs/);
  expect(agentChatSource).toMatch(/overflow-hidden px-6 pb-5/);
  expect(agentChatSource).toMatch(/\[scrollbar-gutter:stable\]/);
  expect(agentChatSource).toMatch(/mx-auto w-full max-w-4xl space-y-6 pr-4/);
  expect(agentChatSource).toMatch(/rounded-2xl border border-border bg-background p-3 shadow-sm/);
  expect(agentChatSource).toMatch(/MessageResponse/);
  expect(agentChatSource).toMatch(/components=\{citationComponents\}/);
  expect(agentChatSource).toMatch(/createCitationComponents/);
  expect(agentChatSource).toMatch(/InlineCitationMarker/);
  expect(agentChatSource).toMatch(/linkifyCitationMarkers/);
  expect(agentChatSource).toMatch(/#citation-\$\{marker\}/);
  expect(agentChatSource).toMatch(/CitationSources/);
  expect(agentChatSource).toMatch(/aria-label="Sources"/);
  expect(agentChatSource).toMatch(/formatAssistantDisplayText/);
  expect(agentChatSource).toMatch(/ensureCitationMarkers/);
  expect(agentChatSource).toMatch(/resultsByMessageId: Record<string, AgentChatAskResult>/);
  expect(agentChatSource).toMatch(/persistThreads\(\{ version: STORAGE_VERSION, threads, activeThreadId, resultsByMessageId \}\)/);
  expect(agentChatSource).toMatch(/parsed\.resultsByMessageId \?\? \{\}/);
  expect(agentChatSource).not.toMatch(/border-t border-border bg-muted\/20/);
  expect(agentChatSource).not.toMatch(/<p className="whitespace-pre-wrap">\{message\.text\}<\/p>/);
  expect(agentChatSource).toMatch(/grid min-h-0 flex-1 grid-cols-\[16rem_minmax\(0,1fr\)\] bg-background/);
  expect(agentChatSource).toMatch(/border-r border-border bg-muted\/20 px-3 pb-3 pt-14/);
  expect(agentChatSource).not.toMatch(/grid min-h-0 flex-1 grid-cols-\[16rem_minmax\(0,1fr\)\] bg-background pt-12/);
  expect(aiElementsMessageSource).toMatch(/export const MessageResponse/);
  expect(aiElementsMessageSource).toMatch(/Streamdown/);
  expect(aiElementsMessageSource).toMatch(/plugins=\{streamdownPlugins\}/);
  expect(aiElementsInlineCitationSource).toMatch(/export const InlineCitation/);
  expect(aiElementsInlineCitationSource).toMatch(/export const InlineCitationCardTrigger/);
  expect(aiElementsInlineCitationSource).toMatch(/export const InlineCitationQuote/);
  expect(aiElementsInlineCitationSource).toMatch(/max-h-72/);
  expect(appSource).toMatch(/invoke\("agent_chat_start"/);
  expect(appSource).toMatch(/invoke\("agent_chat_stop"/);
  expect(appSource).toMatch(/hyprduck:\/\/agent-chat/);
  expect(appTypesSource).toMatch(/interface AgentChatAskPayload/);
  expect(appTypesSource).toMatch(/interface AgentChatAskResult/);
  expect(appTypesSource).toMatch(/type AgentChatStreamEvent/);
  expect(mainSource).toMatch(/case "agent_chat_start"/);
  expect(mainSource).toMatch(/case "agent_chat_stop"/);
  expect(mainSource).toMatch(/command: "agent_chat_ask"/);
  expect(preloadSource).toMatch(/hyprduck:\/\/agent-chat/);
  expect(graphSource).not.toMatch(/AgentTerminal/);
  expect(graphSource).not.toMatch(/GraphPromptComposer/);
  expect(agentChatSource).not.toMatch(/AgentTerminal/);
  expect(agentChatSource).not.toMatch(/terminal/i);
});

test("evidence is rendered as UI content instead of raw markdown", () => {
  expect(graphSource).toMatch(/formatEvidenceSnippet/);
  expect(graphSource).toMatch(/extractMarkdownImageLabel/);
  expect(graphSource).toMatch(/Page image:/);
});

test("workspace graph reader loads the latest materialized snapshot first", () => {
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
  expect(appSource).toMatch(/workspaceLoadStateFromResult\(result, t\)/);
  expect(appSource).toMatch(/workspaceLoadStateFromResult\(initialWorkspaceLoad, t\)/);
  expect(appSource).toMatch(/status: "fallback"/);
  expect(appSource).toMatch(/status: "error"/);
  expect(appSource).toMatch(/WorkspaceSnapshotStatusBanner/);
  expect(appSource).toMatch(/workspace\.status\.refreshingTitle/);
  expect(appSource).toMatch(/workspace\.status\.errorTitle/);
  expect(localesSource).toMatch(/Refreshing latest workspace snapshot/);
  expect(localesSource).toMatch(/Could not refresh the workspace snapshot/);
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
