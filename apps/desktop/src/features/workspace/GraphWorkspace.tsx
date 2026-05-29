import {
  type Dispatch,
  type PointerEvent as ReactPointerEvent,
  type ReactNode,
  useEffect,
  useMemo,
  useRef,
  useState,
} from "react";

import type {
  AgentTerminalAgent,
  AgentTerminalEvent,
  AgentTerminalListResult,
  AgentTerminalSession,
  DesktopUnlisten,
} from "@/appTypes";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { AgentTerminal } from "@/features/agent-terminal/AgentTerminal";
import { cn } from "@/lib/utils";
import {
  ArrowUp,
  LoaderCircle,
  Maximize2,
  Plus,
  Share2,
  Terminal as TerminalIcon,
  Trash2,
  X,
} from "lucide-react";

import type { WorkspaceUiAction, WorkspaceUiState } from "./state";
import { fileNameFromPath } from "./pathUtils";
import { SigmaGraphCanvas } from "./SigmaGraphCanvas";
import type {
  WorkspaceApplyCorrectionRequest,
  WorkspaceEvidenceRef,
  WorkspaceProject,
} from "./types";

interface GraphWorkspaceProps {
  project: WorkspaceProject | null;
  workspaceId: string;
  uiState: WorkspaceUiState;
  importStatus: GraphImportStatus | null;
  dispatch: Dispatch<WorkspaceUiAction>;
  onOpenImport: () => void;
  onOpenArtifact: (path: string, reveal: boolean) => Promise<void>;
  onApplyCorrection: (request: WorkspaceApplyCorrectionRequest) => Promise<void>;
  onCreateAgentTerminalSession: (args: {
    kind?: "agent" | "shell";
    agentId?: AgentTerminalAgent["id"];
    nodeId: string | null;
  }) => Promise<AgentTerminalSession>;
  onListenAgentTerminalEvents: (
    handler: (event: AgentTerminalEvent) => void,
  ) => DesktopUnlisten;
  onListAgentTerminalAgents: () => Promise<AgentTerminalListResult>;
  onKillAgentTerminalSession: (args: {
    sessionId: string;
  }) => Promise<unknown>;
  onResizeAgentTerminalSession: (args: {
    sessionId: string;
    cols: number;
    rows: number;
  }) => Promise<unknown>;
  onWriteAgentTerminalSession: (args: {
    sessionId: string;
    input: string;
  }) => Promise<unknown>;
  onRetryFailedPages: () => Promise<void>;
}

interface GraphImportStatus {
  filePath: string;
  format: string;
  status: string;
  progressPercent: number;
  message: string | null;
  failureMessage?: string | null;
  failedPageCount?: number;
}

export function GraphWorkspace(props: GraphWorkspaceProps) {
  const {
    project,
    workspaceId,
    uiState,
    importStatus,
    dispatch,
    onOpenImport,
    onOpenArtifact,
    onApplyCorrection,
    onCreateAgentTerminalSession,
    onListenAgentTerminalEvents,
    onListAgentTerminalAgents,
    onKillAgentTerminalSession,
    onResizeAgentTerminalSession,
    onWriteAgentTerminalSession,
    onRetryFailedPages,
  } = props;
  const projectNodes = project?.nodes ?? [];
  const nodeById = Object.fromEntries(projectNodes.map((node) => [node.id, node]));
  const selectedEdge =
    (uiState.selectedEdgeId &&
      project?.edgeDetailsById[uiState.selectedEdgeId]) ||
    null;
  const selectedNode =
    (!selectedEdge &&
      uiState.selectedNodeId &&
      project?.detailsByNodeId[uiState.selectedNodeId]) ||
    null;
  const sourceNodeBySourceId = useMemo(() => {
    const entries = Object.values(project?.detailsByNodeId ?? {})
      .map((detail) => [detail.source?.sourceId, detail.node.id] as const)
      .filter((entry): entry is readonly [string, string] => Boolean(entry[0]));
    return Object.fromEntries(entries);
  }, [project?.detailsByNodeId]);
  const mergeCandidates = useMemo(
    () =>
      projectNodes.filter(
        (node) =>
          node.kind === "concept" && node.id !== selectedNode?.node.id,
      ),
    [projectNodes, selectedNode?.node.id],
  );
  const [renameValue, setRenameValue] = useState(selectedNode?.canonicalName ?? "");
  const [mergeTargetNodeId, setMergeTargetNodeId] = useState<string | null>(
    mergeCandidates[0]?.id ?? null,
  );
  const [pendingCorrectionKind, setPendingCorrectionKind] = useState<
    WorkspaceApplyCorrectionRequest["kind"] | null
  >(null);
  const [correctionError, setCorrectionError] = useState<string | null>(null);
  const [deleteConfirmNodeId, setDeleteConfirmNodeId] = useState<string | null>(null);
  const defaultAnswerNodeId =
    selectedNode?.node.id ??
    projectNodes.find((node) => node.kind === "source")?.id ??
    projectNodes.find((node) => node.kind === "document")?.id ??
    null;
  const baseAnswer =
    (defaultAnswerNodeId &&
      project?.answerByNodeId[defaultAnswerNodeId]) ||
    null;
  const [liveAnswer, setLiveAnswer] =
    useState<WorkspaceProject["answerByNodeId"][string] | null>(null);
  const [answerError, setAnswerError] = useState<string | null>(null);
  const [answerPending, setAnswerPending] = useState(false);
  const [agentTerminalOpen, setAgentTerminalOpen] = useState(false);
  const answer = liveAnswer ?? baseAnswer;
  const hiddenConceptCount = project?.summary.hiddenConceptCount ?? 0;
  const hiddenRelationCount = project?.summary.hiddenRelationCount ?? 0;
  const projectionSummary =
    hiddenConceptCount > 0 || hiddenRelationCount > 0
      ? `${project?.nodes.filter((node) => node.kind === "concept").length ?? 0} concepts shown · ${hiddenConceptCount} hidden · ${project?.edges.length ?? 0} links shown · ${hiddenRelationCount} hidden`
      : null;
  const compactionSummary = project?.summary.compactionSummary ?? null;
  const graphMaterializationSummary =
    project?.summary.graphMaterializationSummary ?? null;
  const graphPaneClass = project?.summary.stale
    ? "border-amber-300/70"
    : "border-border/80";
  const answerBadgeLabel =
    answer?.status === "stale"
      ? "Stale"
      : answer?.status === "low_confidence"
      ? "Low confidence"
      : answer?.status === "grounded"
      ? "Grounded"
      : answer?.status === "blocked"
      ? "Blocked"
      : "Preview";
  const selectedSourcePath =
    selectedNode?.source?.sourcePath ?? selectedNode?.evidence[0]?.sourcePath ?? null;
  const selectedMarkdownPath = selectedNode?.source?.markdownPath ?? null;
  const selectedSourceId =
    selectedNode?.source?.sourceId ?? selectedNode?.evidence[0]?.sourceId ?? null;

  useEffect(() => {
    setRenameValue(selectedNode?.canonicalName ?? "");
    setMergeTargetNodeId(mergeCandidates[0]?.id ?? null);
    setPendingCorrectionKind(null);
    setCorrectionError(null);
    setDeleteConfirmNodeId(null);
  }, [mergeCandidates, selectedNode?.canonicalName, selectedNode?.node.id]);

  useEffect(() => {
    setLiveAnswer(null);
    setAnswerError(null);
    setAnswerPending(false);
  }, [project?.summary.projectId, selectedNode?.node.id, uiState.selectedEdgeId]);

  if (!project) {
    return (
      <div className="flex h-full min-h-[30rem] flex-col bg-background px-6 pb-6 pt-14">
        <div className="flex flex-1 flex-col items-center justify-center rounded-xl border border-dashed border-border bg-muted/15 p-10 text-center">
          <div className="mb-4 inline-flex size-12 items-center justify-center rounded-xl bg-secondary text-secondary-foreground">
            <Share2 size={20} />
          </div>
          <h2 className="text-xl font-semibold text-foreground">
            Add private docs
          </h2>
          <p className="mt-3 max-w-2xl text-sm leading-6 text-muted-foreground">
            Drop PDF, DOCX, or DOC files here. HyprDuck will prepare
            source-backed evidence that coding agents can reuse with citations.
          </p>
          <div className="mt-6 flex flex-wrap justify-center gap-2">
            <Button onClick={onOpenImport} type="button">
              Choose files
            </Button>
          </div>
        </div>
      </div>
    );
  }

  async function handleApplyCorrection(
    request: Omit<WorkspaceApplyCorrectionRequest, "projectId" | "nodeId">,
  ) {
    if (!project || !selectedNode) {
      return;
    }

    if (request.kind === "rename" && !(request.value ?? "").trim()) {
      setCorrectionError("Rename needs a non-empty canonical name.");
      return;
    }

    if (request.kind === "merge" && !request.targetNodeId) {
      setCorrectionError("Pick a target concept before applying merge.");
      return;
    }

    setPendingCorrectionKind(request.kind);
    setCorrectionError(null);
    try {
      await onApplyCorrection({
        projectId: project.summary.projectId,
        nodeId: selectedNode.node.id,
        ...request,
      });
      setDeleteConfirmNodeId(null);
    } catch (error) {
      setCorrectionError(String(error));
    } finally {
      setPendingCorrectionKind(null);
    }
  }

  async function handleOpenArtifact(path: string | null | undefined, reveal: boolean) {
    if (!path) {
      return;
    }
    try {
      await onOpenArtifact(path, reveal);
    } catch (error) {
      setAnswerError(String(error));
    }
  }

  return (
    <div className="flex min-h-0 flex-1 flex-col">
      <div
        className={cn(
          "grid min-h-0 flex-1 overflow-hidden bg-background",
          !uiState.inspectorOpen && "grid-cols-1",
        )}
        style={{
          gridTemplateColumns: uiState.inspectorOpen
            ? "minmax(0, 1fr) clamp(18rem, 28vw, 24rem)"
            : undefined,
        }}
      >
        <section
          className={cn(
            "relative flex min-h-0 flex-col bg-background px-6 pb-6 pt-14",
            graphPaneClass,
          )}
        >
          {importStatus && (
            <GraphImportStatusBanner
              onRetryFailedPages={onRetryFailedPages}
              status={importStatus}
            />
          )}
          <SigmaGraphCanvas
            className="flex-1"
            dispatch={dispatch}
            project={project}
            uiState={uiState}
          />
          {uiState.answerDockOpen && (
            <GraphAnswerWindow
              answer={answer}
              answerBadgeLabel={answerBadgeLabel}
              answerError={answerError}
              answerPending={answerPending}
              onClose={() => dispatch({ type: "close_answer_dock" })}
              onOpenArtifact={onOpenArtifact}
              question={uiState.answerInput.trim()}
            />
          )}
          <GraphPromptComposer
            agentTerminal={({ onMinimize }) => (
              <AgentTerminal
                nodeId={selectedNode?.node.id ?? null}
                onClose={() => setAgentTerminalOpen(false)}
                onCreateSession={onCreateAgentTerminalSession}
                onKillSession={onKillAgentTerminalSession}
                onListenAgentTerminalEvents={onListenAgentTerminalEvents}
                onListAgents={onListAgentTerminalAgents}
                onMinimize={onMinimize}
                onResizeSession={onResizeAgentTerminalSession}
                onWriteSession={onWriteAgentTerminalSession}
                open={agentTerminalOpen}
              />
            )}
            agentTerminalOpen={agentTerminalOpen}
            answerError={uiState.answerDockOpen ? null : answerError}
            answerPending={answerPending}
            inputValue={uiState.answerInput}
            onAttachFiles={onOpenImport}
            onOpenAgentTerminal={() => setAgentTerminalOpen(true)}
            onInputChange={(value) =>
              dispatch({
                type: "set_answer_input",
                value,
              })
            }
          />
        </section>

        {uiState.inspectorOpen && (
          <aside
            className="flex min-h-0 flex-col border-l border-border bg-background pt-14"
            style={{ width: "clamp(18rem, 28vw, 24rem)" }}
          >
            <div className="border-b border-border/70 px-4 py-3">
              <h3 className="text-sm font-semibold text-foreground">Right inspector</h3>
              <p className="text-xs text-muted-foreground">
                Selection detail, source provenance, and evidence stay visible
                without leaving the graph.
              </p>
              {projectionSummary ? (
                <p className="mt-2 text-xs font-medium text-muted-foreground">
                  {projectionSummary}
                </p>
              ) : null}
              {graphMaterializationSummary ? (
                <p className="mt-1 text-xs text-muted-foreground">
                  {graphMaterializationSummary}
                </p>
              ) : null}
              {compactionSummary ? (
                <p className="mt-1 text-xs text-muted-foreground">{compactionSummary}</p>
              ) : null}
            </div>

            {selectedEdge ? (
              <div className="flex min-h-0 flex-1 flex-col gap-4 overflow-y-auto px-4 py-4">
                <section className="space-y-2">
                  <div className="flex items-center gap-2">
                    <Badge variant="outline">edge</Badge>
                    <Badge variant="secondary">
                      {selectedEdge.edge.confidence === null
                        ? "No confidence yet"
                        : `${Math.round(selectedEdge.edge.confidence * 100)}% relation confidence`}
                    </Badge>
                  </div>
                  <div>
                    <h4 className="text-lg font-semibold tracking-tight">
                      {nodeById[selectedEdge.edge.sourceNodeId]?.label ?? selectedEdge.edge.sourceNodeId}
                      {" -> "}
                      {nodeById[selectedEdge.edge.targetNodeId]?.label ?? selectedEdge.edge.targetNodeId}
                    </h4>
                    <p className="mt-1 text-sm leading-6 text-muted-foreground">
                      {selectedEdge.explanation}
                    </p>
                  </div>
                  <div className="flex flex-wrap gap-2">
                    <span className="rounded-full bg-secondary px-2.5 py-1 text-xs text-secondary-foreground">
                      {selectedEdge.edge.label}
                    </span>
                    <span className="rounded-full bg-secondary px-2.5 py-1 text-xs text-secondary-foreground">
                      {selectedEdge.edge.evidenceCount} evidence refs
                    </span>
                  </div>
                </section>

                <section className="space-y-3">
                  <div className="flex items-center justify-between">
                    <h5 className="text-sm font-semibold">Why HyprDuck linked these</h5>
                    <span className="text-xs text-muted-foreground">
                      {selectedEdge.evidence.length} refs
                    </span>
                  </div>
                  <div className="space-y-2">
                    {selectedEdge.evidence.map((evidence) => (
                      <EvidenceCard
                        evidence={evidence}
                        key={evidence.id}
                        onOpenArtifact={onOpenArtifact}
                      />
                    ))}
                  </div>
                </section>
              </div>
            ) : selectedNode ? (
              <div className="flex min-h-0 flex-1 flex-col gap-4 overflow-y-auto px-4 py-4">
                <section className="space-y-2">
                  <div className="flex items-center gap-2">
                    <Badge variant="outline">{selectedNode.node.kind}</Badge>
                    <Badge variant="secondary">
                      {selectedNode.node.confidence === null
                        ? "No confidence yet"
                        : `${Math.round(selectedNode.node.confidence * 100)}% draft confidence`}
                    </Badge>
                  </div>
                  <div>
                    <h4 className="text-lg font-semibold tracking-tight">
                      {selectedNode.canonicalName}
                    </h4>
                    <p className="mt-1 text-sm leading-6 text-muted-foreground">
                      {selectedNode.description}
                    </p>
                  </div>
                  <div className="flex flex-wrap gap-2">
                    {selectedNode.aliases.map((alias) => (
                      <span
                        key={alias}
                        className="rounded-full bg-secondary px-2.5 py-1 text-xs text-secondary-foreground"
                      >
                        {alias}
                      </span>
                    ))}
                  </div>
                </section>

                {selectedNode.source ||
                selectedNode.node.kind === "source" ||
                selectedNode.node.kind === "document" ? (
                  <section className="space-y-3 rounded-xl border border-border/70 bg-muted/10 px-3 py-3">
                    <div>
                      <h5 className="text-sm font-semibold">Source Detail</h5>
                      <p className="mt-1 text-xs leading-5 text-muted-foreground">
                        Original source copy stays immutable. Derived markdown,
                        evidence, and linked graph nodes stay adjacent.
                      </p>
                    </div>
                    <div className="grid gap-2 rounded-xl border border-border/70 bg-background px-3 py-2 text-xs">
                      <div className="flex items-center justify-between gap-3">
                        <span className="text-muted-foreground">Source file</span>
                        <span className="truncate font-medium text-foreground">
                          {fileNameFromPath(
                            selectedNode.source?.sourcePath ??
                              selectedNode.evidence[0]?.sourcePath ??
                              selectedNode.canonicalName,
                          )}
                        </span>
                      </div>
                      <div className="flex items-center justify-between gap-3">
                        <span className="text-muted-foreground">Status</span>
                        <span className="font-medium text-foreground">
                          {selectedNode.source?.status ?? "preview"}
                        </span>
                      </div>
                      <div className="flex items-center justify-between gap-3">
                        <span className="text-muted-foreground">Pages</span>
                        <span className="font-medium text-foreground">
                          {selectedNode.source
                            ? `${selectedNode.source.successCount}/${selectedNode.source.pageCount} parsed`
                            : `${selectedNode.evidence.length} evidence refs`}
                        </span>
                      </div>
                      <div className="flex items-center justify-between gap-3">
                        <span className="text-muted-foreground">Raw markdown</span>
                        <span className="truncate font-medium text-foreground">
                          {selectedNode.source?.markdownPath
                            ? fileNameFromPath(selectedNode.source.markdownPath)
                            : "Unavailable"}
                        </span>
                      </div>
                      {selectedNode.source?.description ? (
                        <div className="border-t border-border/70 pt-2">
                          <span className="text-muted-foreground">Description</span>
                          <p className="mt-1 leading-5 text-foreground">
                            {selectedNode.source.description}
                          </p>
                        </div>
                      ) : null}
                      {selectedNode.source?.userContext ? (
                        <div className="border-t border-border/70 pt-2">
                          <span className="text-muted-foreground">User context</span>
                          <p className="mt-1 leading-5 text-foreground">
                            {selectedNode.source.userContext}
                          </p>
                        </div>
                      ) : null}
                      {selectedNode.source?.ingestInstruction ? (
                        <div className="border-t border-border/70 pt-2">
                          <span className="text-muted-foreground">Ingest instruction</span>
                          <p className="mt-1 leading-5 text-foreground">
                            {selectedNode.source.ingestInstruction}
                          </p>
                        </div>
                      ) : null}
                    </div>
                    <div className="grid gap-2">
                      <Button
                        disabled={!selectedSourcePath}
                        onClick={() =>
                          void handleOpenArtifact(selectedSourcePath, false)
                        }
                        size="sm"
                        type="button"
                        variant="outline"
                      >
                        Open source copy
                      </Button>
                      <Button
                        disabled={!selectedMarkdownPath}
                        onClick={() =>
                          void handleOpenArtifact(selectedMarkdownPath, false)
                        }
                        size="sm"
                        type="button"
                        variant="outline"
                      >
                        Open raw markdown
                      </Button>
                      <Button
                        disabled={!selectedSourcePath}
                        onClick={() =>
                          void handleOpenArtifact(selectedSourcePath, true)
                        }
                        size="sm"
                        type="button"
                        variant="outline"
                      >
                        Reveal in Finder
                      </Button>
                    </div>
                  </section>
                ) : null}

                <section className="space-y-3">
                  <div className="flex items-center justify-between">
                    <h5 className="text-sm font-semibold">Visible evidence</h5>
                    <span className="text-xs text-muted-foreground">
                      {selectedNode.evidence.length} refs
                    </span>
                  </div>
                  <div className="space-y-2">
                    {selectedNode.evidence.map((evidence) => (
                      <EvidenceCard
                        evidence={evidence}
                        key={evidence.id}
                        onOpenArtifact={onOpenArtifact}
                      />
                    ))}
                  </div>
                </section>

                {(selectedNode.actions ?? []).length > 0 ? (
                  <section className="space-y-3">
                    <h5 className="text-sm font-semibold">Correction actions</h5>
                    <div className="grid gap-2">
                      {(selectedNode.actions ?? []).map((action) => {
                        const disabled =
                          Boolean(action.disabledReason) || pendingCorrectionKind !== null;

                        if (action.disabledReason) {
                          return (
                            <div
                              key={action.kind}
                              className="rounded-xl border border-dashed border-border/80 px-3 py-3"
                            >
                              <div className="flex items-center justify-between gap-3">
                                <span className="text-sm font-medium">{action.label}</span>
                                <Button disabled size="xs" type="button" variant="outline">
                                  Unavailable
                                </Button>
                              </div>
                              <p className="mt-2 text-xs leading-5 text-muted-foreground">
                                {action.disabledReason}
                              </p>
                            </div>
                          );
                        }

                        if (action.kind === "rename") {
                          return (
                            <div
                              key={action.kind}
                              className="rounded-xl border border-border/80 px-3 py-3"
                            >
                              <div className="flex items-center justify-between gap-3">
                                <span className="text-sm font-medium">{action.label}</span>
                                <Button
                                  disabled={disabled || !renameValue.trim()}
                                  onClick={() =>
                                    void handleApplyCorrection({
                                      kind: "rename",
                                      value: renameValue.trim(),
                                    })
                                  }
                                  size="xs"
                                  type="button"
                                >
                                  {pendingCorrectionKind === "rename"
                                    ? "Applying…"
                                    : "Apply"}
                                </Button>
                              </div>
                              <p className="mt-2 text-xs leading-5 text-muted-foreground">
                                Update the canonical concept name. HyprDuck keeps the previous
                                label as an alias so provenance stays intact.
                              </p>
                              <Input
                                className="mt-3"
                                disabled={pendingCorrectionKind !== null}
                                onChange={(event) => setRenameValue(event.target.value)}
                                value={renameValue}
                              />
                            </div>
                          );
                        }

                        if (action.kind === "merge") {
                          return (
                            <div
                              key={action.kind}
                              className="rounded-xl border border-border/80 px-3 py-3"
                            >
                              <div className="flex items-center justify-between gap-3">
                                <span className="text-sm font-medium">{action.label}</span>
                                <Button
                                  disabled={disabled || !mergeTargetNodeId}
                                  onClick={() =>
                                    void handleApplyCorrection({
                                      kind: "merge",
                                      targetNodeId: mergeTargetNodeId,
                                    })
                                  }
                                  size="xs"
                                  type="button"
                                >
                                  {pendingCorrectionKind === "merge"
                                    ? "Applying…"
                                    : "Apply"}
                                </Button>
                              </div>
                              <p className="mt-2 text-xs leading-5 text-muted-foreground">
                                Fold this concept into another canonical node. HyprDuck keeps the
                                evidence and aliases on the surviving concept.
                              </p>
                              <select
                                className="mt-3 h-9 w-full rounded-md border border-input bg-background px-3 text-sm"
                                disabled={pendingCorrectionKind !== null}
                                onChange={(event) => setMergeTargetNodeId(event.target.value)}
                                value={mergeTargetNodeId ?? ""}
                              >
                                {mergeCandidates.map((node) => (
                                  <option key={node.id} value={node.id}>
                                    {node.label}
                                  </option>
                                ))}
                              </select>
                            </div>
                          );
                        }

                        if (action.kind === "delete") {
                          const deleteArmed = deleteConfirmNodeId === selectedNode.node.id;
                          const isSourceDelete =
                            selectedNode.node.kind === "source" ||
                            selectedNode.node.kind === "document";
                          return (
                            <div
                              key={action.kind}
                              className="rounded-xl border border-destructive/40 bg-destructive/5 px-3 py-3"
                            >
                              <div className="flex items-center justify-between gap-3">
                                <span className="text-sm font-medium text-destructive">
                                  {action.label}
                                </span>
                                <Button
                                  className="gap-1.5"
                                  disabled={disabled}
                                  onClick={() => {
                                    if (!deleteArmed) {
                                      setDeleteConfirmNodeId(selectedNode.node.id);
                                      return;
                                    }
                                    void handleApplyCorrection({
                                      kind: "delete",
                                    });
                                  }}
                                  size="xs"
                                  type="button"
                                  variant={deleteArmed ? "destructive" : "outline"}
                                >
                                  <Trash2 size={13} />
                                  {pendingCorrectionKind === "delete"
                                    ? "Deleting..."
                                    : deleteArmed
                                      ? "Confirm"
                                      : "Delete"}
                                </Button>
                              </div>
                              <p className="mt-2 text-xs leading-5 text-muted-foreground">
                                {isSourceDelete
                                  ? "Remove this source node, the source-backed concept nodes it created, and every connected edge."
                                  : "Remove this concept node and every connected edge."}
                              </p>
                            </div>
                          );
                        }

                        return (
                          <div
                            key={action.kind}
                            className="rounded-xl border border-border/80 px-3 py-3"
                          >
                            <div className="flex items-center justify-between gap-3">
                              <span className="text-sm font-medium">{action.label}</span>
                              <Button
                                disabled={disabled}
                                onClick={() =>
                                  void handleApplyCorrection({
                                    kind: "keep_separate",
                                  })
                                }
                                size="xs"
                                type="button"
                              >
                                {pendingCorrectionKind === "keep_separate"
                                  ? "Applying…"
                                  : "Apply"}
                              </Button>
                            </div>
                            <p className="mt-2 text-xs leading-5 text-muted-foreground">
                              Split the visible aliases under this concept into separate nodes
                              without hiding the original evidence.
                            </p>
                          </div>
                        );
                      })}
                    </div>
                    {correctionError ? (
                      <p className="text-xs leading-5 text-destructive">{correctionError}</p>
                    ) : null}
                  </section>
                ) : null}

                {selectedNode.node.kind !== "source" &&
                selectedNode.node.kind !== "document" &&
                selectedNode.evidence.some((evidence) => evidence.sourceId || evidence.sourcePath) ? (
                  <section className="space-y-3">
                    <h5 className="text-sm font-semibold">Source provenance</h5>
                    <div className="flex flex-wrap gap-2">
                      {uniqueSourceProvenance(selectedNode.evidence).map((source) => {
                        const sourceNodeId =
                          source.sourceId && sourceNodeBySourceId[source.sourceId];
                        return (
                          <Button
                            disabled={!sourceNodeId}
                            key={`${source.sourceId ?? "path"}:${source.sourcePath ?? ""}`}
                            onClick={() =>
                              sourceNodeId &&
                              dispatch({ type: "select_node", nodeId: sourceNodeId })
                            }
                            size="sm"
                            type="button"
                            variant="outline"
                          >
                            {fileNameFromPath(source.sourcePath ?? source.sourceId ?? "Source")}
                          </Button>
                        );
                      })}
                    </div>
                  </section>
                ) : null}
              </div>
            ) : (
              <div className="flex flex-1 items-center justify-center px-6 text-center text-sm text-muted-foreground">
                Select a node or edge to inspect its evidence and provenance.
              </div>
            )}
          </aside>
        )}
      </div>

    </div>
  );
}

interface GraphAnswerWindowProps {
  answer: WorkspaceProject["answerByNodeId"][string] | null;
  answerBadgeLabel: string;
  answerError: string | null;
  answerPending: boolean;
  onClose: () => void;
  onOpenArtifact: (path: string, reveal: boolean) => Promise<void>;
  question: string;
}

function GraphAnswerWindow(props: GraphAnswerWindowProps) {
  const {
    answer,
    answerBadgeLabel,
    answerError,
    answerPending,
    onClose,
    onOpenArtifact,
    question,
  } = props;

  return (
    <section className="pointer-events-auto absolute inset-x-6 bottom-24 z-30 mx-auto max-h-[min(34rem,calc(100%-9rem))] w-[min(50rem,calc(100%-3rem))] overflow-y-auto rounded-2xl border border-border/80 bg-background/95 px-4 py-4 shadow-[0_18px_80px_rgba(15,23,42,0.16)] backdrop-blur">
      <div className="flex items-start justify-between gap-3">
        <div className="min-w-0">
          <div className="flex flex-wrap items-center gap-2">
            <h3 className="text-sm font-semibold text-foreground">Answer</h3>
            <Badge
              variant="outline"
              className={cn(
                answer?.status === "stale" || answer?.status === "low_confidence"
                  ? "border-amber-200 bg-amber-50 text-amber-700"
                  : "border-border bg-secondary text-foreground",
              )}
            >
              {answerBadgeLabel}
            </Badge>
          </div>
          {question ? (
            <p className="mt-1 truncate text-xs text-muted-foreground">{question}</p>
          ) : null}
        </div>
        <Button
          aria-label="Close answer"
          className="size-8 shrink-0"
          onClick={onClose}
          size="icon"
          type="button"
          variant="ghost"
        >
          <X size={16} />
        </Button>
      </div>

      <div className="mt-4">
        {answerPending ? (
          <div className="flex items-center gap-2 text-sm text-muted-foreground">
            <LoaderCircle size={15} className="animate-spin" />
            <span>Answering...</span>
          </div>
        ) : answerError ? (
          <p className="text-sm leading-6 text-destructive">{answerError}</p>
        ) : (
          <p className="whitespace-pre-wrap text-sm leading-6 text-foreground">
            {answer?.text ??
              answer?.explanation ??
              "Ask a question from the prompt below."}
          </p>
        )}
      </div>

      {answer?.citations.length ? (
        <div className="mt-4 border-t border-border/70 pt-3">
          <p className="text-xs font-medium uppercase tracking-[0.14em] text-muted-foreground">
            Sources
          </p>
          <div className="mt-2 grid gap-2 md:grid-cols-2">
            {answer.citations.slice(0, 4).map((citation) => (
              <CompactEvidenceRow
                evidence={citation}
                key={citation.id}
                onOpenArtifact={onOpenArtifact}
              />
            ))}
          </div>
        </div>
      ) : null}
    </section>
  );
}

interface CompactEvidenceRowProps {
  evidence: WorkspaceEvidenceRef;
  onOpenArtifact: (path: string, reveal: boolean) => Promise<void>;
}

function CompactEvidenceRow(props: CompactEvidenceRowProps) {
  const { evidence, onOpenArtifact } = props;
  const primaryPath = evidence.markdownPath ?? evidence.sourcePath ?? evidence.imagePath;
  const sourceLabel = fileNameFromPath(
    evidence.sourcePath ?? evidence.markdownPath ?? evidence.sourceId ?? "Source",
  );

  return (
    <button
      className="min-w-0 rounded-lg border border-border/70 bg-muted/10 px-3 py-2 text-left disabled:cursor-default"
      disabled={!primaryPath}
      onClick={() => primaryPath && void onOpenArtifact(primaryPath, false)}
      type="button"
    >
      <div className="flex items-center justify-between gap-3 text-xs text-muted-foreground">
        <span className="truncate">{sourceLabel}</span>
        <span className="shrink-0">{evidence.pageLabel}</span>
      </div>
      <p className="mt-1 line-clamp-2 text-xs leading-5 text-foreground">
        {formatEvidenceSnippet(evidence.snippet)}
      </p>
    </button>
  );
}

function GraphImportStatusBanner(props: {
  status: GraphImportStatus;
  onRetryFailedPages: () => Promise<void>;
}) {
  const { onRetryFailedPages, status } = props;
  const failed = status.status === "failed" || Boolean(status.failureMessage);
  const partial = status.status === "partial";
  const progress = Math.max(0, Math.min(100, Math.round(status.progressPercent)));
  const failedPageCount = status.failedPageCount ?? 0;
  const canRetryFailedPages = failedPageCount > 0;

  return (
    <div
      className={cn(
        "pointer-events-auto absolute left-6 right-6 top-14 z-30 rounded-xl border px-4 py-3 shadow-sm backdrop-blur",
        failed
          ? "border-destructive/25 bg-destructive/10 text-destructive"
          : partial
            ? "border-amber-300/60 bg-amber-50 text-amber-950"
          : "border-border bg-background/95 text-foreground",
      )}
    >
      <div className="flex items-start justify-between gap-4">
        <div className="min-w-0">
          <p className="text-sm font-semibold">
            {failed
              ? "Import failed"
              : partial
                ? "Partial import"
                : "Importing source file"}
          </p>
          <p className="mt-1 truncate text-sm text-muted-foreground">
            {fileNameFromPath(status.filePath)} · {status.format.toUpperCase()}
            {status.message ? ` · ${status.message}` : ""}
          </p>
        </div>
        <ImportStatusIndicator failed={failed} progress={progress} />
      </div>
      {!failed && !partial && (
        <div className="mt-3 h-1.5 overflow-hidden rounded-full bg-secondary">
          <div
            className="h-full rounded-full bg-foreground transition-all"
            style={{ width: `${progress}%` }}
          />
        </div>
      )}
      {failed && status.failureMessage ? (
        <p className="mt-2 text-sm leading-5">{status.failureMessage}</p>
      ) : null}
      {canRetryFailedPages ? (
        <div className="mt-3 flex flex-wrap items-center justify-between gap-2">
          <p className="text-xs font-medium">
            {failedPageCount} failed {failedPageCount === 1 ? "page" : "pages"} can be retried.
          </p>
          <Button
            className="h-8 border-destructive/30 text-xs"
            onClick={() => void onRetryFailedPages()}
            size="sm"
            type="button"
            variant="outline"
          >
            Retry failed pages
          </Button>
        </div>
      ) : null}
    </div>
  );
}

function ImportStatusIndicator(props: { failed: boolean; progress: number }) {
  const { failed, progress } = props;

  if (failed) {
    return (
      <div className="flex size-9 shrink-0 items-center justify-center rounded-full border border-destructive/30 bg-destructive/10 text-destructive">
        <X size={18} strokeWidth={2.4} aria-hidden="true" />
      </div>
    );
  }

  return (
    <div className="relative flex size-9 shrink-0 items-center justify-center rounded-full border border-border bg-background text-foreground">
      <LoaderCircle
        size={34}
        strokeWidth={1.8}
        className="absolute animate-spin text-muted-foreground"
        aria-hidden="true"
      />
      <span className="text-[10px] font-medium tabular-nums">{progress}%</span>
    </div>
  );
}

interface GraphPromptComposerProps {
  agentTerminal: (props: { onMinimize: () => void }) => ReactNode;
  agentTerminalOpen: boolean;
  answerError: string | null;
  answerPending: boolean;
  inputValue: string;
  onAttachFiles: () => void;
  onOpenAgentTerminal: () => void;
  onInputChange: (value: string) => void;
}

const TERMINAL_DEFAULT_SIZE = { width: 800, height: 544 };
const TERMINAL_MIN_SIZE = { width: 480, height: 260 };

function clampTerminalSize(value: number, min: number, max: number) {
  return Math.max(min, Math.min(max, Math.round(value)));
}

function GraphPromptComposer(props: GraphPromptComposerProps) {
  const {
    agentTerminal,
    agentTerminalOpen,
    answerError,
    answerPending,
    inputValue,
    onAttachFiles,
    onOpenAgentTerminal,
    onInputChange,
  } = props;
  const [terminalContentVisible, setTerminalContentVisible] = useState(false);
  const [terminalMinimized, setTerminalMinimized] = useState(false);
  const [terminalResizing, setTerminalResizing] = useState(false);
  const [terminalSize, setTerminalSize] = useState(TERMINAL_DEFAULT_SIZE);
  const resizeCleanupRef = useRef<(() => void) | null>(null);

  useEffect(() => {
    if (!agentTerminalOpen) {
      setTerminalContentVisible(false);
      setTerminalMinimized(false);
      return undefined;
    }
    if (terminalMinimized) {
      setTerminalContentVisible(false);
      return undefined;
    }
    const timer = window.setTimeout(() => {
      setTerminalContentVisible(true);
    }, 180);
    return () => window.clearTimeout(timer);
  }, [agentTerminalOpen, terminalMinimized]);

  useEffect(
    () => () => {
      resizeCleanupRef.current?.();
    },
    [],
  );

  const openTerminal = () => {
    setTerminalMinimized(false);
    onOpenAgentTerminal();
  };

  const minimizeTerminal = () => {
    setTerminalContentVisible(false);
    setTerminalMinimized(true);
  };

  const beginTerminalResize = (
    event: ReactPointerEvent<HTMLButtonElement>,
  ) => {
    if (!agentTerminalOpen || terminalMinimized) {
      return;
    }
    event.preventDefault();
    const startX = event.clientX;
    const startY = event.clientY;
    const startSize = terminalSize;
    setTerminalResizing(true);

    const onPointerMove = (moveEvent: PointerEvent) => {
      const viewportMaxWidth = Math.max(
        TERMINAL_MIN_SIZE.width,
        window.innerWidth - 48,
      );
      const viewportMaxHeight = Math.max(
        TERMINAL_MIN_SIZE.height,
        window.innerHeight - 80,
      );
      setTerminalSize({
        width: clampTerminalSize(
          startSize.width + (moveEvent.clientX - startX) * 2,
          TERMINAL_MIN_SIZE.width,
          viewportMaxWidth,
        ),
        height: clampTerminalSize(
          startSize.height - (moveEvent.clientY - startY),
          TERMINAL_MIN_SIZE.height,
          viewportMaxHeight,
        ),
      });
    };
    const stopResize = () => {
      window.removeEventListener("pointermove", onPointerMove);
      window.removeEventListener("pointerup", stopResize);
      resizeCleanupRef.current = null;
      setTerminalResizing(false);
    };
    resizeCleanupRef.current = stopResize;
    window.addEventListener("pointermove", onPointerMove);
    window.addEventListener("pointerup", stopResize);
  };

  const showTerminalContent =
    agentTerminalOpen && !terminalMinimized && terminalContentVisible;
  const composerStyle =
    agentTerminalOpen && !terminalMinimized
      ? {
          width: `min(${terminalSize.width}px, calc(100vw - 3rem))`,
          height: `min(${terminalSize.height}px, calc(100vh - 5rem))`,
        }
      : undefined;
  const terminalStyle =
    agentTerminalOpen && !terminalMinimized
      ? {
          height: `min(${terminalSize.height}px, calc(100vh - 5rem))`,
        }
      : undefined;

  return (
    <form
      className={cn(
        "agent-terminal-composer-frame pointer-events-auto absolute inset-x-6 bottom-6 mx-auto flex w-[min(50rem,calc(100%-3rem))] items-end gap-3",
        agentTerminalOpen && !terminalMinimized
          ? "z-30 h-[min(34rem,calc(100vh-5rem))]"
          : "z-20 h-14",
        terminalResizing && "transition-none",
      )}
      style={composerStyle}
      onSubmit={(event) => {
        event.preventDefault();
        openTerminal();
      }}
    >
      <Button
        aria-label="Attach files"
        className={cn(
          "h-14 rounded-full border-border/80 bg-background/95 shadow-[0_18px_60px_rgba(15,23,42,0.12)] backdrop-blur transition-[width,opacity] duration-200",
          agentTerminalOpen && !terminalMinimized
            ? "pointer-events-none w-0 border-0 opacity-0"
            : "w-14 opacity-100",
        )}
        onClick={onAttachFiles}
        size="icon"
        type="button"
        variant="outline"
      >
        <Plus size={19} />
      </Button>
      <div
        className={cn(
          "agent-terminal-composer-pill relative min-w-0 flex-1 overflow-hidden border shadow-[0_18px_60px_rgba(15,23,42,0.12)] backdrop-blur",
          agentTerminalOpen && !terminalMinimized
            ? "h-[min(34rem,calc(100vh-5rem))] rounded-2xl border-zinc-700/80 bg-zinc-900/95 text-zinc-100 shadow-[0_24px_80px_rgba(0,0,0,0.28)]"
            : "flex h-14 items-center gap-2 rounded-full border-border/80 bg-background/95 px-3",
          showTerminalContent ? "p-0" : "flex items-end gap-2 px-3 pb-2",
          terminalResizing && "transition-none",
        )}
        style={terminalStyle}
      >
        {agentTerminalOpen ? (
          <div
            className={cn(
              "h-full w-full",
              showTerminalContent
                ? "animate-in fade-in duration-200"
                : "pointer-events-none absolute inset-0 opacity-0",
            )}
          >
            {agentTerminal({ onMinimize: minimizeTerminal })}
          </div>
        ) : null}
        {terminalMinimized ? (
          <>
            <div className="flex min-w-0 flex-1 items-center gap-2 px-2">
              <TerminalIcon size={17} className="shrink-0 text-muted-foreground" />
              <span className="truncate text-sm font-medium text-muted-foreground">
                Agent Terminal
              </span>
            </div>
            <Button
              aria-label="Restore Agent Terminal"
              className="mb-0.5 size-9 rounded-full"
              onClick={openTerminal}
              size="icon"
              type="button"
            >
              <Maximize2 size={16} />
            </Button>
          </>
        ) : showTerminalContent ? (
          <button
            aria-label="Resize Agent Terminal"
            className="absolute right-1 top-1 z-40 size-5 cursor-nesw-resize rounded text-zinc-500 hover:bg-zinc-800 hover:text-zinc-200"
            onPointerDown={beginTerminalResize}
            type="button"
          >
            <span className="pointer-events-none block size-full rounded border-r border-t border-current" />
          </button>
        ) : (
          <>
            <input
              aria-label="Open Agent Terminal"
              className={cn(
                "h-10 min-w-0 flex-1 bg-transparent px-2 text-base outline-none",
                agentTerminalOpen
                  ? "text-zinc-100 placeholder:text-zinc-500"
                  : "text-foreground placeholder:text-muted-foreground",
              )}
              onChange={(event) => onInputChange(event.target.value)}
              onFocus={openTerminal}
              placeholder="Open Agent Terminal..."
              value={inputValue}
            />
            {answerPending ? (
              <span className="hidden text-xs font-medium text-muted-foreground sm:inline">
                Answering...
              </span>
            ) : null}
            <Button
              aria-label="Open Agent Terminal"
              className={cn(
                "mb-0.5 size-9 rounded-full",
                agentTerminalOpen
                  ? "bg-zinc-700 text-zinc-200 hover:bg-zinc-600 hover:text-zinc-50"
                  : "",
              )}
              size="icon"
              type="submit"
            >
              <ArrowUp size={18} />
            </Button>
          </>
        )}
      </div>
      {answerError ? (
        <p className="absolute left-16 top-full mt-2 text-xs leading-5 text-destructive">
          {answerError}
        </p>
      ) : null}
    </form>
  );
}

interface EvidenceCardProps {
  evidence: WorkspaceEvidenceRef;
  onOpenArtifact: (path: string, reveal: boolean) => Promise<void>;
}

function EvidenceCard(props: EvidenceCardProps) {
  const { evidence, onOpenArtifact } = props;
  const imageLabel = evidence.imagePath
    ? fileNameFromPath(evidence.imagePath)
    : extractMarkdownImageLabel(evidence.snippet);
  const artifactRows = [
    evidence.markdownPath
      ? { label: "Markdown", path: evidence.markdownPath }
      : null,
    evidence.imagePath ? { label: "Page image", path: evidence.imagePath } : null,
    evidence.sourcePath ? { label: "Source", path: evidence.sourcePath } : null,
  ].filter((row): row is { label: string; path: string } => Boolean(row));

  return (
    <article className="rounded-xl border border-border/70 bg-muted/10 px-3 py-3">
      <div className="flex items-center justify-between gap-2 text-xs text-muted-foreground">
        <span>
          {evidence.pageLabel}
          {typeof evidence.pageIndex === "number"
            ? ` · page index ${evidence.pageIndex + 1}`
            : ""}
        </span>
        <span className="truncate">
          {fileNameFromPath(evidence.sourcePath ?? evidence.sourceId ?? "Imported source")}
        </span>
      </div>
      {evidence.provenance ? (
        <p className="mt-2 text-xs leading-5 text-muted-foreground">
          {evidence.provenance}
        </p>
      ) : null}
      <p className="mt-2 text-sm leading-6 text-foreground">
        {formatEvidenceSnippet(evidence.snippet)}
      </p>
      {imageLabel ? (
        <div className="mt-2 rounded-xl border border-border/70 bg-background px-3 py-2 text-xs text-muted-foreground">
          Page image: {imageLabel}
        </div>
      ) : null}
      {artifactRows.length ? (
        <div className="mt-3 grid gap-2">
          {artifactRows.map((row) => (
            <div
              className="flex items-center justify-between gap-3 rounded-xl border border-border/70 bg-background px-3 py-2 text-xs"
              key={`${row.label}:${row.path}`}
            >
              <span className="text-muted-foreground">{row.label}</span>
              <Button
                className="h-7 min-w-0 max-w-[11rem] justify-start truncate px-2 text-xs"
                onClick={() => void onOpenArtifact(row.path, false)}
                size="sm"
                type="button"
                variant="ghost"
              >
                {fileNameFromPath(row.path)}
              </Button>
            </div>
          ))}
        </div>
      ) : null}
    </article>
  );
}

function extractMarkdownImageLabel(value: string): string | null {
  return value.match(/!\[([^\]]*)\]\(([^)]+)\)/)?.[1]?.trim() || null;
}

function formatEvidenceSnippet(value: string): string {
  return value
    .replace(/!\[[^\]]*\]\([^)]+\)/g, "")
    .replace(/\s+/g, " ")
    .trim() || "No text evidence is available for this page yet.";
}

function uniqueSourceProvenance(evidenceRefs: WorkspaceEvidenceRef[]) {
  const seen = new Set<string>();
  return evidenceRefs
    .map((evidence) => ({
      sourceId: evidence.sourceId ?? null,
      sourcePath: evidence.sourcePath ?? null,
    }))
    .filter((source) => {
      const key = source.sourceId ?? source.sourcePath;
      if (!key || seen.has(key)) {
        return false;
      }
      seen.add(key);
      return true;
    });
}
