import type { ReactNode } from "react";
import { AlertTriangle, ExternalLink, FileText, FolderOpen, Search, Upload, X } from "lucide-react";

import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { cn } from "@/lib/utils";

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
  onRetryFailedPages: () => Promise<void>;
  onViewInGraph: (sourceId: string) => void;
}

export function DocsWorkspace(props: DocsWorkspaceProps) {
  const { importStatus, sources, onChooseFile, onOpenArtifact, onRetryFailedPages, onViewInGraph } =
    props;
  const warnings = sources.filter((source) => source.status === "failed" || source.status === "stale");

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
            <div className="flex min-w-[16rem] items-center gap-2">
              <div className="relative min-w-0 flex-1">
                <Search className="pointer-events-none absolute left-3 top-1/2 size-4 -translate-y-1/2 text-muted-foreground" />
                <Input className="pl-9" placeholder="Search sources" readOnly />
              </div>
              <Button size="sm" type="button" variant="outline">
                Filter
              </Button>
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
            ) : (
              sources.map((source) => (
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
                    <IconAction label="Open file" onClick={() => void onOpenArtifact(source.original_path, false)}>
                      <ExternalLink size={15} />
                    </IconAction>
                    <IconAction
                      label="Open extracted text"
                      onClick={() => void onOpenArtifact(source.markdown_path, false)}
                    >
                      <FileText size={15} />
                    </IconAction>
                    <IconAction
                      label="Reveal in Finder"
                      onClick={() => void onOpenArtifact(source.original_path, true)}
                    >
                      <FolderOpen size={15} />
                    </IconAction>
                    <Button onClick={() => onViewInGraph(source.source_id)} size="sm" type="button" variant="ghost">
                      Graph
                    </Button>
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

function IconAction({
  children,
  label,
  onClick,
}: {
  children: ReactNode;
  label: string;
  onClick: () => void;
}) {
  return (
    <Button aria-label={label} onClick={onClick} size="icon" title={label} type="button" variant="ghost">
      {children}
    </Button>
  );
}

function statusLabel(status: WorkspaceSourceSummary["status"]) {
  return status
    .split("_")
    .map((part) => part.charAt(0).toUpperCase() + part.slice(1))
    .join(" ");
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
