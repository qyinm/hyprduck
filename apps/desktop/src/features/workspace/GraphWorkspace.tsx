import { type Dispatch, useEffect, useMemo, useState } from "react";

import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { cn } from "@/lib/utils";
import {
  AlertTriangle,
  PanelBottomClose,
  PanelBottomOpen,
  PanelRightClose,
  PanelRightOpen,
  RefreshCw,
  Share2,
} from "lucide-react";

import type { WorkspaceUiAction, WorkspaceUiState } from "./state";
import type {
  WorkspaceAnswerProjectRequest,
  WorkspaceApplyCorrectionRequest,
  WorkspaceProject,
} from "./types";

interface GraphWorkspaceProps {
  project: WorkspaceProject | null;
  uiState: WorkspaceUiState;
  dispatch: Dispatch<WorkspaceUiAction>;
  onOpenImport: () => void;
  onApplyCorrection: (request: WorkspaceApplyCorrectionRequest) => Promise<void>;
  onAskProject: (request: WorkspaceAnswerProjectRequest) => Promise<WorkspaceProject["answerByNodeId"][string]>;
}

export function GraphWorkspace(props: GraphWorkspaceProps) {
  const {
    project,
    uiState,
    dispatch,
    onOpenImport,
    onApplyCorrection,
    onAskProject,
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
  const defaultAnswerNodeId =
    selectedNode?.node.id ??
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
  const answer = liveAnswer ?? baseAnswer;
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

  useEffect(() => {
    setRenameValue(selectedNode?.canonicalName ?? "");
    setMergeTargetNodeId(mergeCandidates[0]?.id ?? null);
    setPendingCorrectionKind(null);
    setCorrectionError(null);
  }, [mergeCandidates, selectedNode?.canonicalName, selectedNode?.node.id]);

  useEffect(() => {
    setLiveAnswer(null);
    setAnswerError(null);
    setAnswerPending(false);
  }, [project?.summary.projectId, selectedNode?.node.id, uiState.selectedEdgeId]);

  if (!project) {
    return (
      <div className="flex h-full min-h-[30rem] flex-col items-center justify-center rounded-[24px] border border-dashed border-border bg-muted/15 p-10 text-center">
        <div className="mb-4 inline-flex size-12 items-center justify-center rounded-2xl bg-secondary text-secondary-foreground">
          <Share2 size={20} />
        </div>
        <h2 className="text-xl font-semibold text-foreground">
          Build a graph workspace from your first import
        </h2>
        <p className="mt-3 max-w-2xl text-sm leading-6 text-muted-foreground">
          DuckDocs now has a dedicated graph workspace shell. Import a document to
          seed the first preview project, then use the right inspector and bottom
          answer dock to review evidence before the compile-backed knowledge layer
          lands.
        </p>
        <div className="mt-6 flex flex-wrap justify-center gap-2">
          <Button onClick={onOpenImport} type="button">
            Go to Import
          </Button>
          <Button
            onClick={() => dispatch({ type: "open_answer_dock" })}
            type="button"
            variant="outline"
          >
            Preview answer dock
          </Button>
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
    } catch (error) {
      setCorrectionError(String(error));
    } finally {
      setPendingCorrectionKind(null);
    }
  }

  async function handleAskProject() {
    if (!project) {
      return;
    }

    setAnswerPending(true);
    setAnswerError(null);
    try {
      const nextAnswer = await onAskProject({
        projectId: project.summary.projectId,
        nodeId: selectedNode?.node.id ?? null,
        question: uiState.answerInput,
      });
      setLiveAnswer(nextAnswer);
    } catch (error) {
      setAnswerError(String(error));
    } finally {
      setAnswerPending(false);
    }
  }

  return (
    <div className="flex min-h-0 flex-1 flex-col gap-4">
      <section className="rounded-[24px] border border-border/80 bg-background/80 px-5 py-4">
        <div className="flex flex-col gap-3 lg:flex-row lg:items-start lg:justify-between">
          <div className="space-y-2">
            <div className="flex flex-wrap items-center gap-2">
              <Badge variant="outline" className="border-emerald-200 bg-emerald-50 text-emerald-700">
                {project.summary.status === "preview"
                  ? "Preview workspace"
                  : "Knowledge workspace"}
              </Badge>
              {project.summary.stale && (
                <Badge
                  variant="outline"
                  className="border-amber-200 bg-amber-50 text-amber-700"
                >
                  Stale while new write job runs
                </Badge>
              )}
            </div>
            <div>
              <h2 className="text-xl font-semibold tracking-tight text-foreground">
                {project.summary.title}
              </h2>
              <p className="mt-1 max-w-4xl text-sm leading-6 text-muted-foreground">
                {project.summary.summary}
              </p>
            </div>
          </div>

          <div className="flex flex-wrap gap-2">
            <Button
              onClick={() => dispatch({ type: "toggle_inspector" })}
              type="button"
              variant="outline"
            >
              {uiState.inspectorOpen ? (
                <>
                  <PanelRightClose size={16} />
                  Hide inspector
                </>
              ) : (
                <>
                  <PanelRightOpen size={16} />
                  Show inspector
                </>
              )}
            </Button>
            <Button
              onClick={() =>
                dispatch({
                  type: uiState.answerDockOpen
                    ? "close_answer_dock"
                    : "open_answer_dock",
                })
              }
              type="button"
              variant="outline"
            >
              {uiState.answerDockOpen ? (
                <>
                  <PanelBottomClose size={16} />
                  Hide answer
                </>
              ) : (
                <>
                  <PanelBottomOpen size={16} />
                  Open answer
                </>
              )}
            </Button>
          </div>
        </div>
      </section>

      {project.summary.stale && (
        <section className="rounded-[20px] border border-amber-200 bg-amber-50/85 px-4 py-3 text-sm text-amber-900">
          <div className="flex items-start gap-3">
            <AlertTriangle size={18} className="mt-0.5 shrink-0" />
            <div className="space-y-1">
              <p className="font-medium">Read path stays open while write jobs run.</p>
              <p className="leading-6">
                You are looking at the most recent stable workspace snapshot. New
                compile, re-import, or correction writes can finish in the
                background without freezing the graph.
              </p>
            </div>
          </div>
        </section>
      )}

      <div
        className={cn(
          "grid min-h-0 flex-1 gap-4",
          uiState.inspectorOpen
            ? "grid-cols-1 xl:grid-cols-[minmax(0,1.65fr)_minmax(19rem,24rem)]"
            : "grid-cols-1",
        )}
      >
        <section
          className={cn(
            "flex min-h-[26rem] flex-col rounded-[24px] border bg-background",
            graphPaneClass,
          )}
        >
          <div className="flex flex-wrap items-center justify-between gap-2 border-b border-border/70 px-4 py-3">
            <div>
              <h3 className="text-sm font-semibold text-foreground">
                Graph workspace
              </h3>
              <p className="text-xs text-muted-foreground">
                Graph remains the primary surface. Evidence and answers stay
                attached to the selected node.
              </p>
            </div>
            <div className="flex flex-wrap gap-2 text-xs text-muted-foreground">
              <span>{project.summary.nodeCount} nodes</span>
              <span>•</span>
              <span>{project.summary.relationshipCount} relationships</span>
              <span>•</span>
              <span>{project.summary.evidenceCount} visible evidence refs</span>
            </div>
          </div>

          <div className="relative flex-1 overflow-hidden p-4">
            <div className="absolute inset-4 rounded-[20px] bg-[radial-gradient(circle_at_top,rgba(17,94,89,0.08),transparent_38%),linear-gradient(180deg,rgba(246,244,239,0.7),rgba(255,255,255,0.95))]" />
            <svg
              aria-hidden="true"
              className="absolute inset-4 size-[calc(100%-2rem)]"
              viewBox="0 0 100 100"
            >
              {project.edges.map((edge) => {
                const sourceNode = nodeById[edge.sourceNodeId];
                const targetNode = nodeById[edge.targetNodeId];
                if (!sourceNode || !targetNode) {
                  return null;
                }
                const selected = uiState.selectedEdgeId === edge.id;
                return (
                  <g key={edge.id}>
                    <line
                      stroke={
                        selected
                          ? "rgba(13, 148, 136, 0.95)"
                          : edge.kind === "source_document"
                          ? "rgba(148, 163, 184, 0.55)"
                          : "rgba(93, 104, 112, 0.45)"
                      }
                      strokeDasharray={edge.kind === "source_document" ? "3 4" : undefined}
                      strokeWidth={selected ? 2.4 : 1.5}
                      x1={sourceNode.position.x}
                      x2={targetNode.position.x}
                      y1={sourceNode.position.y}
                      y2={targetNode.position.y}
                    />
                    <line
                      onClick={() => dispatch({ type: "select_edge", edgeId: edge.id })}
                      stroke="transparent"
                      strokeWidth="9"
                      style={{ pointerEvents: "stroke" }}
                      x1={sourceNode.position.x}
                      x2={targetNode.position.x}
                      y1={sourceNode.position.y}
                      y2={targetNode.position.y}
                    />
                  </g>
                );
              })}
            </svg>

            <div className="relative size-full">
              {project.nodes.map((node) => {
                const selected = uiState.selectedNodeId === node.id;
                const edgeConnected = Boolean(
                  selectedEdge &&
                    (selectedEdge.edge.sourceNodeId === node.id ||
                      selectedEdge.edge.targetNodeId === node.id),
                );
                return (
                  <button
                    key={node.id}
                    onClick={() =>
                      dispatch({ type: "select_node", nodeId: node.id })
                    }
                    type="button"
                    className={cn(
                      "absolute min-w-32 -translate-x-1/2 -translate-y-1/2 rounded-2xl border px-3 py-2 text-left shadow-[0_10px_25px_rgba(15,23,42,0.04)] transition",
                      node.kind === "document"
                        ? "border-teal-300/60 bg-white text-foreground"
                        : "border-stone-300/90 bg-white/95 text-foreground",
                      selected &&
                        "border-teal-600 ring-2 ring-teal-600/20 shadow-[0_12px_24px_rgba(15,23,42,0.08)]",
                      edgeConnected &&
                        !selected &&
                        "border-teal-400/80 ring-2 ring-teal-500/10 shadow-[0_12px_24px_rgba(15,23,42,0.06)]",
                    )}
                    style={{
                      left: `${node.position.x}%`,
                      top: `${node.position.y}%`,
                    }}
                  >
                    <div className="text-[11px] uppercase tracking-[0.14em] text-muted-foreground">
                      {node.kind}
                    </div>
                    <div className="mt-1 text-sm font-medium">{node.label}</div>
                    <div className="mt-2 text-xs text-muted-foreground">
                      {node.evidenceCount} evidence refs
                    </div>
                  </button>
                );
              })}
            </div>
          </div>
        </section>

        {uiState.inspectorOpen && (
          <aside className="flex min-h-[26rem] flex-col rounded-[24px] border border-border/80 bg-background">
            <div className="border-b border-border/70 px-4 py-3">
              <h3 className="text-sm font-semibold text-foreground">Inspector</h3>
              <p className="text-xs text-muted-foreground">
                Evidence stays visible even when answers stay cautious.
              </p>
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
                    <h5 className="text-sm font-semibold">Why DuckDocs linked these</h5>
                    <span className="text-xs text-muted-foreground">
                      {selectedEdge.evidence.length} refs
                    </span>
                  </div>
                  <div className="space-y-2">
                    {selectedEdge.evidence.map((evidence) => (
                      <article
                        key={evidence.id}
                        className="rounded-2xl border border-border/70 bg-muted/10 px-3 py-3"
                      >
                        <div className="flex items-center justify-between gap-2 text-xs text-muted-foreground">
                          <span>{evidence.pageLabel}</span>
                          <span>
                            {evidence.sourcePath?.split("/").pop() ?? "Imported source"}
                          </span>
                        </div>
                        <p className="mt-2 text-sm leading-6 text-foreground">
                          {evidence.snippet}
                        </p>
                      </article>
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

                <section className="space-y-3">
                  <div className="flex items-center justify-between">
                    <h5 className="text-sm font-semibold">Visible evidence</h5>
                    <span className="text-xs text-muted-foreground">
                      {selectedNode.evidence.length} refs
                    </span>
                  </div>
                  <div className="space-y-2">
                    {selectedNode.evidence.map((evidence) => (
                      <article
                        key={evidence.id}
                        className="rounded-2xl border border-border/70 bg-muted/10 px-3 py-3"
                      >
                        <div className="flex items-center justify-between gap-2 text-xs text-muted-foreground">
                          <span>{evidence.pageLabel}</span>
                          <span>
                            {evidence.sourcePath?.split("/").pop() ?? "Imported source"}
                          </span>
                        </div>
                        <p className="mt-2 text-sm leading-6 text-foreground">
                          {evidence.snippet}
                        </p>
                      </article>
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
                              className="rounded-2xl border border-dashed border-border/80 px-3 py-3"
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
                              className="rounded-2xl border border-border/80 px-3 py-3"
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
                                Update the canonical concept name. DuckDocs keeps the previous
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
                              className="rounded-2xl border border-border/80 px-3 py-3"
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
                                Fold this concept into another canonical node. DuckDocs keeps the
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

                        return (
                          <div
                            key={action.kind}
                            className="rounded-2xl border border-border/80 px-3 py-3"
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
              </div>
            ) : (
              <div className="flex flex-1 items-center justify-center px-6 text-center text-sm text-muted-foreground">
                Select a node or edge to inspect its evidence and trust signals.
              </div>
            )}
          </aside>
        )}
      </div>

      {uiState.answerDockOpen && (
        <section className="rounded-[24px] border border-border/80 bg-background">
          <div className="flex flex-wrap items-start justify-between gap-3 border-b border-border/70 px-4 py-3">
            <div>
              <h3 className="text-sm font-semibold text-foreground">Grounded answer dock</h3>
              <p className="text-xs text-muted-foreground">
                Bottom-docked so the graph stays visible while answers remain
                attached to evidence.
              </p>
            </div>
            <Badge
              variant="outline"
              className={cn(
                answer?.status === "stale" || answer?.status === "low_confidence"
                  ? "border-amber-200 bg-amber-50 text-amber-700"
                  : "border-teal-200 bg-teal-50 text-teal-700",
              )}
            >
              {answerBadgeLabel}
            </Badge>
          </div>

          <div className="grid gap-4 px-4 py-4 xl:grid-cols-[minmax(0,1.1fr)_minmax(0,0.9fr)]">
            <div className="space-y-3">
              <label className="text-xs font-medium uppercase tracking-[0.14em] text-muted-foreground">
                Ask this workspace
              </label>
              <Input
                onChange={(event) =>
                  dispatch({
                    type: "set_answer_input",
                    value: event.target.value,
                  })
                }
                placeholder="What does this node appear to cover?"
                value={uiState.answerInput}
              />
              <div className="flex flex-wrap gap-2">
                <Button
                  disabled={answerPending || !uiState.answerInput.trim()}
                  onClick={() => void handleAskProject()}
                  type="button"
                >
                  {answerPending ? "Answering…" : "Ask workspace"}
                </Button>
                <Button
                  onClick={() => dispatch({ type: "close_answer_dock" })}
                  type="button"
                  variant="outline"
                >
                  Close dock
                </Button>
              </div>
              <p className="text-xs leading-5 text-muted-foreground">
                Questions now go through the backend project reader, so corrections
                and stored graph state show up here immediately.
              </p>
              {answerError ? (
                <p className="text-xs leading-5 text-destructive">{answerError}</p>
              ) : null}
            </div>

            <div className="space-y-4 rounded-[20px] border border-border/70 bg-muted/10 px-4 py-4">
              <div className="flex items-center gap-2">
                <RefreshCw size={14} className="text-muted-foreground" />
                <p className="text-sm font-medium">
                  {liveAnswer ? "Live answer state" : "Stored answer state"}
                </p>
              </div>
              <p className="text-sm leading-6 text-foreground">
                {answer?.text ??
                  answer?.explanation ??
                  "Select a node to view the answer state."}
              </p>
              {answer?.citations.length ? (
                <div className="space-y-2">
                  <p className="text-xs font-medium uppercase tracking-[0.14em] text-muted-foreground">
                    Cited evidence
                  </p>
                  {answer.citations.map((citation) => (
                    <article
                      key={citation.id}
                      className="rounded-2xl border border-border/70 bg-background px-3 py-3"
                    >
                      <div className="text-xs text-muted-foreground">
                        {citation.pageLabel}
                      </div>
                      <p className="mt-1 text-sm leading-6">{citation.snippet}</p>
                    </article>
                  ))}
                </div>
              ) : null}
              {answer?.suggestedActions.length ? (
                <div className="space-y-2">
                  <p className="text-xs font-medium uppercase tracking-[0.14em] text-muted-foreground">
                    Suggested next actions
                  </p>
                  <div className="space-y-2">
                    {answer.suggestedActions.map((action) => (
                      <div
                        key={action.kind}
                        className="rounded-2xl border border-dashed border-border/70 px-3 py-3"
                      >
                        <div className="text-sm font-medium">{action.label}</div>
                        <p className="mt-1 text-xs leading-5 text-muted-foreground">
                          {action.description}
                        </p>
                      </div>
                    ))}
                  </div>
                </div>
              ) : null}
            </div>
          </div>
        </section>
      )}
    </div>
  );
}
