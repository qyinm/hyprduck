import { useMemo, useRef, useState } from "react";
import {
  AlertTriangle,
  ArrowLeft,
  ExternalLink,
  FileSearch,
  FileText,
  FolderOpen,
  Search,
  Upload,
  Waypoints,
  X,
} from "lucide-react";

import type { SourceDetailResult } from "@/appTypes";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { cn } from "@/lib/utils";

import { ParsedMarkdownPreview } from "./docs/ParsedMarkdownPreview";
import { PdfOriginalPreview } from "./docs/PdfOriginalPreview";
import {
  IconAction,
  SourceDetailError,
  SourceDetailLoading,
  SourceDetailPanel,
  UnsupportedOriginalPreview,
  type SourceDetailLoadState,
} from "./docs/sourceDetailShared";
import { fileNameFromPath } from "./pathUtils";
import type { WorkspaceSourceSummary } from "./types";

export interface WorkspaceImportStatus {
  filePath: string;
  format: string;
  status: string;
  progressPercent: number;
  message: string | null;
  failureMessage?: string | null;
  failedPageCount?: number;
}

interface DocsWorkspaceProps {
  importStatus: WorkspaceImportStatus | null;
  sources: WorkspaceSourceSummary[];
  onChooseFile: () => Promise<void>;
  onOpenArtifact: (path: string, reveal: boolean) => Promise<void>;
  onReadSourceDetail: (source: WorkspaceSourceSummary) => Promise<SourceDetailResult>;
  onRetryFailedPages: () => Promise<void>;
  onViewInGraph: (sourceId: string) => void;
}

export function DocsWorkspace(props: DocsWorkspaceProps) {
  const {
    importStatus,
    sources,
    onChooseFile,
    onOpenArtifact,
    onReadSourceDetail,
    onRetryFailedPages,
    onViewInGraph,
  } = props;
  const [sourceSearch, setSourceSearch] = useState("");
  const [detailSource, setDetailSource] = useState<WorkspaceSourceSummary | null>(null);
  const [detailState, setDetailState] = useState<SourceDetailLoadState>({
    status: "idle",
    data: null,
    error: null,
  });
  const detailRequestId = useRef(0);
  const visibleSources = useMemo(() => {
    const query = normalizeSearchText(sourceSearch.trim());
    if (!query) {
      return sources;
    }

    return sources.filter((source) => {
      const fileName = fileNameFromPath(source.original_path || source.source_path);
      return [
        fileName,
        source.original_path,
        source.source_path,
        source.markdown_path,
        statusLabel(source.status),
      ].some((value) => normalizeSearchText(value).includes(query));
    });
  }, [sourceSearch, sources]);
  const warnings = sources.filter((source) => source.status === "failed" || source.status === "stale");

  const openSourceDetail = (source: WorkspaceSourceSummary) => {
    const requestId = detailRequestId.current + 1;
    detailRequestId.current = requestId;
    setDetailSource(source);
    setDetailState({ status: "loading", data: null, error: null });
    void onReadSourceDetail(source)
      .then((detail) => {
        if (detailRequestId.current === requestId) {
          setDetailState({ status: "ready", data: detail, error: null });
        }
      })
      .catch((error) => {
        if (detailRequestId.current !== requestId) {
          return;
        }
        setDetailState({
          status: "error",
          data: null,
          error: error instanceof Error ? error.message : String(error),
        });
      });
  };

  if (detailSource) {
    return (
      <SourceDetailWorkspace
        detailState={detailState}
        onBack={() => {
          detailRequestId.current += 1;
          setDetailSource(null);
          setDetailState({ status: "idle", data: null, error: null });
        }}
        onOpenArtifact={onOpenArtifact}
        onRetry={() => openSourceDetail(detailSource)}
        onViewInGraph={onViewInGraph}
        source={detailSource}
      />
    );
  }

  return (
    <div className="min-h-0 flex-1 overflow-y-auto bg-background px-6 pb-8 pt-14">
      <div className="mx-auto flex w-full max-w-6xl flex-col gap-4">
        <section className="rounded-lg border border-border bg-background p-5">
          <div className="flex flex-wrap items-start justify-between gap-4">
            <div>
              <h1 className="text-lg font-semibold text-foreground">Add Sources</h1>
              <p className="mt-1 text-sm text-muted-foreground">
                Add files from your computer. HyprDuck will parse and index them for evidence.
              </p>
            </div>
            <Badge variant="secondary">{sources.length} sources</Badge>
          </div>
          <button
            className="mt-5 flex min-h-32 w-full items-center justify-center rounded-lg border border-dashed border-border bg-muted/10 px-4 text-left transition hover:bg-muted/25"
            onClick={() => void onChooseFile()}
            type="button"
          >
            <div className="flex items-center gap-4">
              <span className="flex size-11 items-center justify-center rounded-full border border-border bg-background text-foreground">
                <Upload size={20} />
              </span>
              <span>
                <span className="block text-sm font-medium text-foreground">Drop files here</span>
                <span className="mt-1 block text-sm text-muted-foreground">or click to browse</span>
              </span>
            </div>
          </button>
          <div className="mt-4 flex flex-wrap items-center justify-between gap-3 text-xs text-muted-foreground">
            <span>Supported: PDF, DOCX, DOC</span>
            <span>Max file size: 200 MB</span>
          </div>
        </section>

        {importStatus && (
          <section className="rounded-lg border border-border bg-background p-5">
            <div className="flex items-center justify-between gap-4">
              <h2 className="text-sm font-semibold text-foreground">Import Queue</h2>
              {importStatus.failedPageCount ? (
                <Button onClick={() => void onRetryFailedPages()} size="sm" type="button" variant="outline">
                  Retry failed pages
                </Button>
              ) : null}
            </div>
            <div className="mt-4 rounded-lg border border-border">
              <div className="grid grid-cols-[minmax(0,1.5fr)_5rem_minmax(8rem,1fr)_8rem_2rem] items-center gap-4 px-4 py-3 text-sm">
                <div className="flex min-w-0 items-center gap-3">
                  <FileText className="size-4 shrink-0 text-muted-foreground" />
                  <span className="truncate font-medium text-foreground">
                    {fileNameFromPath(importStatus.filePath)}
                  </span>
                </div>
                <span className="text-xs uppercase text-muted-foreground">{importStatus.format}</span>
                <ProgressBar value={importStatus.progressPercent} />
                <span className="text-xs text-muted-foreground">
                  {importStatus.message ?? importStatus.status}
                </span>
                <X className="size-4 text-muted-foreground" />
              </div>
              {importStatus.failureMessage ? (
                <div className="border-t border-border px-4 py-3 text-xs text-destructive">
                  {importStatus.failureMessage}
                </div>
              ) : null}
            </div>
          </section>
        )}

        <section className="rounded-lg border border-border bg-background p-5">
          <div className="flex flex-wrap items-center justify-between gap-3">
            <div className="flex items-center gap-2">
              <h2 className="text-lg font-semibold text-foreground">Sources</h2>
              <Badge variant="secondary">{sources.length}</Badge>
            </div>
            <div className="min-w-[16rem]">
              <div className="relative min-w-0 flex-1">
                <Search className="pointer-events-none absolute left-3 top-1/2 size-4 -translate-y-1/2 text-muted-foreground" />
                <input
                  aria-label="Search sources"
                  autoComplete="off"
                  className="ui-input"
                  onChange={(event) => setSourceSearch(event.currentTarget.value)}
                  placeholder="Search sources"
                  spellCheck={false}
                  style={{ paddingLeft: "2.5rem" }}
                  type="search"
                  value={sourceSearch}
                />
              </div>
            </div>
          </div>
          <div className="mt-4 overflow-hidden rounded-lg border border-border">
            <div className="grid grid-cols-[minmax(12rem,1fr)_7rem_5rem_8rem_10rem_8rem] gap-4 border-b border-border bg-muted/20 px-4 py-2 text-xs font-medium text-muted-foreground">
              <span>Source</span>
              <span>Status</span>
              <span>Pages</span>
              <span>Evidence</span>
              <span>Last indexed</span>
              <span className="text-right">Actions</span>
            </div>
            {sources.length === 0 ? (
              <div className="px-4 py-8 text-center text-sm text-muted-foreground">
                No sources indexed yet.
              </div>
            ) : visibleSources.length === 0 ? (
              <div className="px-4 py-8 text-center text-sm text-muted-foreground">
                No sources match "{sourceSearch.trim()}".
              </div>
            ) : (
              visibleSources.map((source) => (
                <div
                  className="grid grid-cols-[minmax(12rem,1fr)_7rem_5rem_8rem_10rem_8rem] items-center gap-4 border-b border-border px-4 py-3 text-sm last:border-b-0"
                  key={source.source_id}
                >
                  <div className="flex min-w-0 items-center gap-3">
                    <FileText className="size-4 shrink-0 text-muted-foreground" />
                    <span className="truncate font-medium text-foreground">
                      {fileNameFromPath(source.original_path || source.source_path)}
                    </span>
                  </div>
                  <StatusBadge status={source.status} />
                  <span className="text-muted-foreground">{source.page_count}</span>
                  <span className="text-muted-foreground">{source.success_count}</span>
                  <span className="text-muted-foreground">{formatTimestamp(source.updated_at)}</span>
                  <div className="flex justify-end gap-1">
                    <IconAction label="Details" onClick={() => openSourceDetail(source)}>
                      <FileSearch size={15} />
                    </IconAction>
                    <IconAction
                      label="Reveal in Finder"
                      onClick={() => void onOpenArtifact(previewableSourcePath(source), true)}
                    >
                      <FolderOpen size={15} />
                    </IconAction>
                    <IconAction label="View in Graph" onClick={() => onViewInGraph(source.source_id)}>
                      <Waypoints size={15} />
                    </IconAction>
                  </div>
                </div>
              ))
            )}
          </div>
        </section>

        {warnings.length > 0 && (
          <section className="rounded-lg border border-border bg-background p-5">
            <div className="flex items-center gap-2">
              <AlertTriangle className="size-5 text-amber-500" />
              <h2 className="text-lg font-semibold text-foreground">Parse Warnings</h2>
              <Badge variant="secondary">{warnings.length}</Badge>
            </div>
            <div className="mt-4 divide-y divide-border rounded-lg border border-border">
              {warnings.map((source) => (
                <div className="grid grid-cols-[minmax(0,1fr)_8rem] gap-4 px-4 py-3 text-sm" key={source.source_id}>
                  <span className="truncate text-foreground">
                    {fileNameFromPath(source.original_path || source.source_path)}
                  </span>
                  <StatusBadge status={source.status} />
                </div>
              ))}
            </div>
          </section>
        )}
      </div>
    </div>
  );
}

function SourceDetailWorkspace({
  detailState,
  onBack,
  onOpenArtifact,
  onRetry,
  onViewInGraph,
  source,
}: {
  detailState: SourceDetailLoadState;
  onBack: () => void;
  onOpenArtifact: (path: string, reveal: boolean) => Promise<void>;
  onRetry: () => void;
  onViewInGraph: (sourceId: string) => void;
  source: WorkspaceSourceSummary;
}) {
  const originalPath = source.original_path || source.source_path;
  const sourceArtifactPath = previewableSourcePath(source);
  const title =
    detailState.status === "ready"
      ? detailState.data.fileName
      : fileNameFromPath(originalPath || source.markdown_path);

  return (
    <div className="min-h-0 flex-1 bg-background px-5 pb-5 pt-12">
      <div className="mx-auto flex h-full w-full max-w-[92rem] flex-col gap-3">
        <header className="flex min-h-14 flex-wrap items-center justify-between gap-3 border-b border-border bg-background px-1 pb-3">
          <div className="flex min-w-0 items-center gap-3">
            <Button aria-label="Back to sources" onClick={onBack} size="icon" type="button" variant="ghost">
              <ArrowLeft size={17} />
            </Button>
            <div className="min-w-0">
              <h1 className="truncate text-lg font-semibold text-foreground">{title}</h1>
              <div className="mt-1 flex flex-wrap items-center gap-2 text-xs text-muted-foreground">
                <StatusBadge status={source.status} />
                <span>{source.page_count} pages</span>
                <span>{source.success_count} evidence</span>
                <span>{source.format.toUpperCase()}</span>
              </div>
            </div>
          </div>
          <div className="flex items-center gap-1">
            <IconAction label="Open original" onClick={() => void onOpenArtifact(sourceArtifactPath, false)}>
              <ExternalLink size={15} />
            </IconAction>
            <IconAction label="Reveal in Finder" onClick={() => void onOpenArtifact(sourceArtifactPath, true)}>
              <FolderOpen size={15} />
            </IconAction>
            <IconAction label="View in Graph" onClick={() => onViewInGraph(source.source_id)}>
              <Waypoints size={15} />
            </IconAction>
          </div>
        </header>

        <div className="grid min-h-0 flex-1 grid-cols-1 gap-3 xl:grid-cols-2">
          <OriginalPreviewPanel
            detailState={detailState}
            onOpenOriginal={() => void onOpenArtifact(sourceArtifactPath, false)}
          />
          <ParsedMarkdownPreview detailState={detailState} onRetry={onRetry} />
        </div>
      </div>
    </div>
  );
}

function OriginalPreviewPanel({
  detailState,
  onOpenOriginal,
}: {
  detailState: SourceDetailLoadState;
  onOpenOriginal: () => void;
}) {
  return (
    <SourceDetailPanel title="Original">
      <OriginalPreview detailState={detailState} onOpenOriginal={onOpenOriginal} />
    </SourceDetailPanel>
  );
}

function OriginalPreview({
  detailState,
  onOpenOriginal,
}: {
  detailState: SourceDetailLoadState;
  onOpenOriginal: () => void;
}) {
  if (detailState.status === "loading" || detailState.status === "idle") {
    return <SourceDetailLoading label="Loading original preview..." />;
  }
  if (detailState.status === "error") {
    return <SourceDetailError message={detailState.error} />;
  }

  const original = detailState.data.original;
  if (original.kind === "pdf" && original.previewUrl) {
    return <PdfOriginalPreview onOpenOriginal={onOpenOriginal} previewUrl={original.previewUrl} />;
  }
  if (original.kind === "text" && original.text !== null) {
    return (
      <div className="h-full overflow-auto bg-muted/20 p-4">
        <pre className="min-h-full whitespace-pre-wrap break-words rounded-md bg-background p-4 text-xs leading-6 text-foreground shadow-xs">
          {original.text}
        </pre>
        {original.truncated ? (
          <p className="mt-3 text-xs text-muted-foreground">
            Preview is truncated to the first 2 MB.
          </p>
        ) : null}
      </div>
    );
  }
  if (original.kind === "missing") {
    return <SourceDetailError message={original.error ?? "Original file is missing."} />;
  }
  return (
    <UnsupportedOriginalPreview
      message={original.error ?? "Inline preview is not available for this file type."}
      onOpenOriginal={onOpenOriginal}
    />
  );
}

function ProgressBar({ value }: { value: number }) {
  return (
    <div className="h-2 rounded-full bg-muted">
      <div
        className="h-full rounded-full bg-primary transition-all"
        style={{ width: `${Math.max(0, Math.min(100, value))}%` }}
      />
    </div>
  );
}

function StatusBadge({ status }: { status: WorkspaceSourceSummary["status"] }) {
  const tone =
    status === "ingested"
      ? "bg-emerald-100 text-emerald-700"
      : status === "failed"
        ? "bg-red-100 text-red-700"
        : status === "stale"
          ? "bg-amber-100 text-amber-700"
          : "bg-blue-100 text-blue-700";
  return <span className={cn("w-fit rounded-full px-2 py-1 text-xs font-medium", tone)}>{statusLabel(status)}</span>;
}

function statusLabel(status: WorkspaceSourceSummary["status"]) {
  return status
    .split("_")
    .map((part) => part.charAt(0).toUpperCase() + part.slice(1))
    .join(" ");
}

function normalizeSearchText(value: string) {
  return value.normalize("NFC").toLowerCase();
}

function previewableSourcePath(source: WorkspaceSourceSummary) {
  return source.source_path || source.original_path;
}

function formatTimestamp(value: number) {
  if (!value) {
    return "-";
  }
  return new Intl.DateTimeFormat(undefined, {
    month: "short",
    day: "numeric",
    hour: "2-digit",
    minute: "2-digit",
  }).format(new Date(value * 1000));
}
