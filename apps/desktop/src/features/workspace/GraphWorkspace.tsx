import {
  type Dispatch,
  useEffect,
  useMemo,
  useState,
} from "react";

import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { useI18n } from "@/i18n/I18nProvider";
import { cn } from "@/lib/utils";
import {
  Check,
  ExternalLink,
  FileText,
  FolderOpen,
  LoaderCircle,
  Share2,
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
  onOpenDocs: () => void;
  onOpenArtifact: (path: string, reveal: boolean) => Promise<void>;
  onApplyCorrection: (request: WorkspaceApplyCorrectionRequest) => Promise<void>;
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
    onOpenDocs,
    onOpenArtifact,
    onApplyCorrection,
    onRetryFailedPages,
  } = props;
  const { t } = useI18n();
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
  const answer = liveAnswer ?? baseAnswer;
  const graphPaneClass = project?.summary.stale
    ? "border-amber-300/70"
    : "border-border/80";
  const answerBadgeLabel =
    answer?.status === "stale"
      ? t("workspace.answer.stale")
      : answer?.status === "low_confidence"
      ? t("workspace.answer.lowConfidence")
      : answer?.status === "grounded"
      ? t("workspace.answer.grounded")
      : answer?.status === "blocked"
      ? t("workspace.answer.blocked")
      : t("workspace.answer.preview");
  const selectedSourcePath =
    selectedNode?.source?.sourcePath ?? selectedNode?.evidence[0]?.sourcePath ?? null;
  const selectedMarkdownPath = selectedNode?.source?.markdownPath ?? null;
  const selectedDeleteAction =
    selectedNode?.actions.find((action) => action.kind === "delete") ?? null;
  const deleteArmed = selectedNode
    ? deleteConfirmNodeId === selectedNode.node.id
    : false;
  const deleteDisabled =
    Boolean(selectedDeleteAction?.disabledReason) || pendingCorrectionKind !== null;
  const isSourceDelete =
    selectedNode?.node.kind === "source" || selectedNode?.node.kind === "document";
  useEffect(() => {
    setPendingCorrectionKind(null);
    setCorrectionError(null);
    setDeleteConfirmNodeId(null);
  }, [selectedNode?.node.id]);

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
            {t("workspace.empty.title")}
          </h2>
          <p className="mt-3 max-w-2xl text-sm leading-6 text-muted-foreground">
            {t("workspace.empty.body")}
          </p>
          <div className="mt-6 flex flex-wrap justify-center gap-2">
            <Button onClick={onOpenDocs} type="button">
              Open Docs
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
              copy={{
                answering: t("workspace.answer.answering"),
                close: t("workspace.answer.close"),
              }}
              onClose={() => dispatch({ type: "close_answer_dock" })}
              onOpenArtifact={onOpenArtifact}
              question={uiState.answerInput.trim()}
            />
          )}
        </section>

        {uiState.inspectorOpen && (
          <aside
            aria-label={t("workspace.inspector.label")}
            className="flex min-h-0 flex-col border-l border-border bg-background pt-14"
            style={{ width: "clamp(18rem, 28vw, 24rem)" }}
          >
            {selectedEdge ? (
              <div className="flex min-h-0 flex-1 flex-col gap-3 overflow-y-auto px-4 py-3">
                <section className="space-y-2 border-b border-border/70 pb-3">
                  <div className="flex items-center gap-2">
                    <Badge variant="outline">{t("workspace.inspector.connection")}</Badge>
                    <Badge variant="secondary">
                      {selectedEdge.edge.confidence === null
                        ? t("workspace.inspector.evidenceBacked")
                        : t("workspace.inspector.confidence", {
                            percent: Math.round(selectedEdge.edge.confidence * 100),
                          })}
                    </Badge>
                  </div>
                  <div>
                    <h4 className="text-base font-semibold leading-6 tracking-tight">
                      {t("workspace.inspector.connectsTo", {
                        source:
                          nodeById[selectedEdge.edge.sourceNodeId]?.label ??
                          selectedEdge.edge.sourceNodeId,
                        target:
                          nodeById[selectedEdge.edge.targetNodeId]?.label ??
                          selectedEdge.edge.targetNodeId,
                      })}
                    </h4>
                    <p className="mt-1 line-clamp-2 text-sm leading-5 text-muted-foreground">
                      {selectedEdge.explanation}
                    </p>
                  </div>
                  <div className="flex flex-wrap gap-1.5">
                    <span className="rounded-full bg-secondary px-2.5 py-1 text-xs text-secondary-foreground">
                      {selectedEdge.edge.label}
                    </span>
                    <span className="rounded-full bg-secondary px-2.5 py-1 text-xs text-secondary-foreground">
                      {selectedEdge.edge.evidenceCount} evidence
                    </span>
                  </div>
                </section>

                <section className="space-y-2">
                  <div className="flex items-center justify-between">
                    <h5 className="text-sm font-semibold">Why these are connected</h5>
                    <span className="text-xs text-muted-foreground">
                      {selectedEdge.evidence.length} evidence
                    </span>
                  </div>
                  <div className="space-y-2">
                    {selectedEdge.evidence.slice(0, 3).map((evidence) => (
                      <EvidenceCard
                        evidence={evidence}
                        key={evidence.id}
                      />
                    ))}
                  </div>
                </section>
              </div>
            ) : selectedNode ? (
              <div className="flex min-h-0 flex-1 flex-col gap-3 overflow-y-auto px-4 py-3">
                <section className="space-y-2 border-b border-border/70 pb-3">
                  <div className="flex items-center gap-2">
                    <Badge variant="outline">{workspaceSelectionKindLabel(selectedNode.node.kind)}</Badge>
                    <Badge variant="secondary">
                      {selectedNode.node.confidence === null
                        ? t("workspace.inspector.evidenceBacked")
                        : t("workspace.inspector.confidence", {
                            percent: Math.round(selectedNode.node.confidence * 100),
                          })}
                    </Badge>
                  </div>
                  <div>
                    <h4 className="text-base font-semibold leading-6 tracking-tight">
                      {selectedNode.canonicalName}
                    </h4>
                    {customerVisibleDescription(selectedNode.description) ? (
                      <p className="mt-1 line-clamp-2 text-sm leading-5 text-muted-foreground">
                        {customerVisibleDescription(selectedNode.description)}
                      </p>
                    ) : null}
                  </div>
                  <div className="flex flex-wrap gap-1.5">
                    {selectedNode.aliases.map((alias) => (
                      <span
                        key={alias}
                        className="rounded-full bg-secondary px-2 py-0.5 text-[11px] text-secondary-foreground"
                      >
                        {alias}
                      </span>
                    ))}
                  </div>
                </section>

                {selectedNode.source ||
                selectedNode.node.kind === "source" ||
                selectedNode.node.kind === "document" ? (
                  <section className="space-y-2 rounded-lg border border-border/70 bg-muted/10 px-3 py-3">
                    <div className="flex items-center justify-between gap-3">
                      <h5 className="text-sm font-semibold">Document</h5>
                      <div className="flex shrink-0 items-center gap-1">
                        {selectedNode.source?.format ? (
                          <span className="rounded-full bg-background px-2 py-0.5 text-[11px] font-medium uppercase text-muted-foreground">
                            {selectedNode.source.format}
                          </span>
                        ) : null}
                        <Button
                          aria-label={t("workspace.inspector.openFile")}
                          className="size-7 rounded-full"
                          disabled={!selectedSourcePath}
                          onClick={() =>
                            void handleOpenArtifact(selectedSourcePath, false)
                          }
                          size="icon"
                          title={t("workspace.inspector.openFile")}
                          type="button"
                          variant="ghost"
                        >
                          <ExternalLink size={14} />
                        </Button>
                        <Button
                          aria-label={t("workspace.inspector.openExtractedText")}
                          className="size-7 rounded-full"
                          disabled={!selectedMarkdownPath}
                          onClick={() =>
                            void handleOpenArtifact(selectedMarkdownPath, false)
                          }
                          size="icon"
                          title={t("workspace.inspector.openExtractedText")}
                          type="button"
                          variant="ghost"
                        >
                          <FileText size={14} />
                        </Button>
                        <Button
                          aria-label={t("workspace.inspector.revealInFinder")}
                          className="size-7 rounded-full"
                          disabled={!selectedSourcePath}
                          onClick={() =>
                            void handleOpenArtifact(selectedSourcePath, true)
                          }
                          size="icon"
                          title={t("workspace.inspector.revealInFinder")}
                          type="button"
                          variant="ghost"
                        >
                          <FolderOpen size={14} />
                        </Button>
                      </div>
                    </div>
                    <div className="grid gap-1.5 text-xs">
                      <div className="flex items-center justify-between gap-3">
                        <span className="text-muted-foreground">File</span>
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
                            : `${selectedNode.evidence.length} evidence`}
                        </span>
                      </div>
                    </div>
                  </section>
                ) : null}

                <section className="space-y-2">
                  <div className="flex items-center justify-between">
                    <h5 className="text-sm font-semibold">Evidence</h5>
                    <span className="text-xs text-muted-foreground">
                      {selectedNode.evidence.length} evidence
                    </span>
                  </div>
                  <div className="space-y-2">
                    {selectedNode.evidence.slice(0, 3).map((evidence) => (
                      <EvidenceCard
                        evidence={evidence}
                        key={evidence.id}
                      />
                    ))}
                  </div>
                </section>

                {selectedDeleteAction ? (
                  <section className="space-y-2 border-t border-border/70 pt-3">
                    <div className="rounded-xl border border-destructive/40 bg-destructive/5 px-3 py-3">
                      <div className="flex items-center justify-between gap-3">
                        <span className="text-sm font-medium text-destructive">
                          {selectedDeleteAction.label}
                        </span>
                        <Button
                          className="gap-1.5"
                          disabled={deleteDisabled}
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
                        {selectedDeleteAction.disabledReason ??
                          (isSourceDelete
                            ? "Remove this document and the knowledge items it created."
                            : "Remove this knowledge item and its connected links.")}
                      </p>
                    </div>
                    {correctionError ? (
                      <p className="text-xs leading-5 text-destructive">{correctionError}</p>
                    ) : null}
                  </section>
                ) : null}

                {selectedNode.node.kind !== "source" &&
                selectedNode.node.kind !== "document" &&
                selectedNode.evidence.some((evidence) => evidence.sourceId || evidence.sourcePath) ? (
                  <section className="space-y-2 border-t border-border/70 pt-3">
                    <h5 className="text-xs font-medium text-muted-foreground">From documents</h5>
                    <div className="flex flex-wrap gap-1.5">
                      {uniqueSourceProvenance(selectedNode.evidence).map((source) => {
                        const sourceNodeId =
                          source.sourceId && sourceNodeBySourceId[source.sourceId];
                        return (
                          <Button
                            className="h-7 max-w-full justify-start truncate px-2 text-xs"
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
                Select an item or connection to inspect its evidence.
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
  copy: {
    answering: string;
    close: string;
  };
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
    copy,
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
          aria-label={copy.close}
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
            <span>{copy.answering}</span>
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
    evidence.sourcePath ?? evidence.markdownPath ?? evidence.sourceId ?? "Document",
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

function workspaceSelectionKindLabel(kind: WorkspaceProject["nodes"][number]["kind"]): string {
  switch (kind) {
    case "source":
    case "document":
      return "Document";
    case "page":
      return "Page";
    case "concept":
      return "Concept";
    default:
      return "Item";
  }
}

function customerVisibleDescription(value: string): string | null {
  const trimmed = value.trim();
  if (!trimmed) {
    return null;
  }
  if (/^Node\s+\S+\s+is materialized in snapshot\b/.test(trimmed)) {
    return null;
  }
  if (/\bSource refs?:\s*/.test(trimmed)) {
    return null;
  }
  return trimmed;
}

function formatImportLifecycleTitle(status: string): string {
  switch (status) {
    case "imported":
      return "Import accepted";
    case "parsing":
      return "Parsing source";
    case "packaging":
      return "Packaging citations";
    case "citation_ready":
      return "Citation-ready";
    case "citation_ready_graph_pending":
      return "Citation-ready, graph pending";
    case "citation_ready_graph_skipped":
      return "Citation-ready, graph skipped";
    case "graph_retry_waiting":
      return "Citation-ready, graph retry waiting";
    case "context_ready":
      return "Context-ready";
    case "partial":
      return "Partial import";
    case "failed":
      return "Import failed";
    case "cancelled":
      return "Import cancelled";
    default:
      return "Import in progress";
  }
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
          <p className="text-sm font-semibold">{formatImportLifecycleTitle(status.status)}</p>
          <p className="mt-1 truncate text-sm text-muted-foreground">
            {fileNameFromPath(status.filePath)} · {status.format.toUpperCase()}
            {status.message ? ` · ${status.message}` : ""}
          </p>
        </div>
        <ImportStatusIndicator
          failed={failed}
          ready={
            status.status === "citation_ready" ||
            status.status === "citation_ready_graph_pending" ||
            status.status === "citation_ready_graph_skipped" ||
            status.status === "context_ready"
          }
          progress={progress}
        />
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

function ImportStatusIndicator(props: { failed: boolean; ready: boolean; progress: number }) {
  const { failed, ready, progress } = props;

  if (failed) {
    return (
      <div className="flex size-9 shrink-0 items-center justify-center rounded-full border border-destructive/30 bg-destructive/10 text-destructive">
        <X size={18} strokeWidth={2.4} aria-hidden="true" />
      </div>
    );
  }

  if (ready) {
    return (
      <div className="flex size-9 shrink-0 items-center justify-center rounded-full border border-emerald-300/60 bg-emerald-50 text-emerald-700">
        <Check size={18} strokeWidth={2.4} aria-hidden="true" />
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

interface EvidenceCardProps {
  evidence: WorkspaceEvidenceRef;
}

function EvidenceCard(props: EvidenceCardProps) {
  const { evidence } = props;
  const imageLabel = evidence.imagePath
    ? fileNameFromPath(evidence.imagePath)
    : extractMarkdownImageLabel(evidence.snippet);

  return (
    <article className="rounded-lg border border-border/70 bg-muted/10 px-3 py-2.5">
      <div className="flex items-center justify-between gap-2 text-xs text-muted-foreground">
        <span>
          {evidence.pageLabel}
          {typeof evidence.pageIndex === "number"
            ? ` · page index ${evidence.pageIndex + 1}`
            : ""}
        </span>
        <span className="truncate">
          {fileNameFromPath(evidence.sourcePath ?? evidence.sourceId ?? "Imported document")}
        </span>
      </div>
      <p className="mt-1.5 line-clamp-2 text-sm leading-5 text-foreground">
        {formatEvidenceSnippet(evidence.snippet)}
      </p>
      {imageLabel ? (
        <div className="mt-2 truncate rounded-md bg-background px-2 py-1 text-xs text-muted-foreground">
          Page image: {imageLabel}
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
