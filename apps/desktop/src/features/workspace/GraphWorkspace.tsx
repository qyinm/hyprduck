import { type Dispatch, useMemo } from "react";

import { Button } from "@/components/ui/button";
import { useI18n } from "@/i18n/I18nProvider";
import { cn } from "@/lib/utils";
import {
  Check,
  LoaderCircle,
  Share2,
  X,
} from "lucide-react";

import { GraphEdgeInspector } from "./GraphEdgeInspector";
import { GraphNodeInspector } from "./GraphNodeInspector";
import { fileNameFromPath } from "./pathUtils";
import type { WorkspaceUiAction, WorkspaceUiState } from "./state";
import type {
  WorkspaceApplyCorrectionRequest,
  WorkspaceProject,
} from "./types";
import { WorkspaceGraphCanvas } from "./WorkspaceGraphCanvas";

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
    workspaceId: _workspaceId,
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
  const graphPaneClass = project?.summary.stale
    ? "border-amber-300/70"
    : "border-border/80";

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
          <WorkspaceGraphCanvas
            className="flex-1"
            dispatch={dispatch}
            project={project}
            uiState={uiState}
          />
        </section>

        {uiState.inspectorOpen && (
          <aside
            aria-label={t("workspace.inspector.label")}
            className="flex min-h-0 flex-col border-l border-border bg-background pt-14"
            style={{ width: "clamp(18rem, 28vw, 24rem)" }}
          >
            {selectedEdge ? (
              <GraphEdgeInspector nodeById={nodeById} selectedEdge={selectedEdge} />
            ) : selectedNode ? (
              <GraphNodeInspector
                dispatch={dispatch}
                onApplyCorrection={onApplyCorrection}
                onOpenArtifact={onOpenArtifact}
                projectId={project.summary.projectId}
                selectedNode={selectedNode}
                sourceNodeBySourceId={sourceNodeBySourceId}
              />
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
