import { type Dispatch, useEffect, useState } from "react";
import {
  ExternalLink,
  FileText,
  FolderOpen,
  Trash2,
} from "lucide-react";

import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { useI18n } from "@/i18n/I18nProvider";

import { EvidenceCard } from "./GraphEvidence";
import { fileNameFromPath } from "./pathUtils";
import type { WorkspaceUiAction } from "./state";
import type {
  WorkspaceApplyCorrectionRequest,
  WorkspaceEvidenceRef,
  WorkspaceProject,
} from "./types";

interface GraphNodeInspectorProps {
  selectedNode: NonNullable<WorkspaceProject["detailsByNodeId"][string]>;
  sourceNodeBySourceId: Record<string, string>;
  dispatch: Dispatch<WorkspaceUiAction>;
  onOpenArtifact: (path: string, reveal: boolean) => Promise<void>;
  onApplyCorrection: (request: WorkspaceApplyCorrectionRequest) => Promise<void>;
  projectId: string;
}

export function GraphNodeInspector(props: GraphNodeInspectorProps) {
  const {
    selectedNode,
    sourceNodeBySourceId,
    dispatch,
    onOpenArtifact,
    onApplyCorrection,
    projectId,
  } = props;
  const { t } = useI18n();
  const [pendingCorrectionKind, setPendingCorrectionKind] = useState<
    WorkspaceApplyCorrectionRequest["kind"] | null
  >(null);
  const [correctionError, setCorrectionError] = useState<string | null>(null);
  const [deleteConfirmNodeId, setDeleteConfirmNodeId] = useState<string | null>(null);

  const selectedSourcePath =
    selectedNode.source?.sourcePath ?? selectedNode.evidence[0]?.sourcePath ?? null;
  const selectedMarkdownPath = selectedNode.source?.markdownPath ?? null;
  const selectedDeleteAction =
    selectedNode.actions.find((action) => action.kind === "delete") ?? null;
  const deleteArmed = deleteConfirmNodeId === selectedNode.node.id;
  const deleteDisabled =
    Boolean(selectedDeleteAction?.disabledReason) || pendingCorrectionKind !== null;
  const isSourceDelete =
    selectedNode.node.kind === "source" || selectedNode.node.kind === "document";

  useEffect(() => {
    setPendingCorrectionKind(null);
    setCorrectionError(null);
    setDeleteConfirmNodeId(null);
  }, [selectedNode.node.id]);

  async function handleApplyCorrection(
    request: Omit<WorkspaceApplyCorrectionRequest, "projectId" | "nodeId">,
  ) {
    setPendingCorrectionKind(request.kind);
    setCorrectionError(null);
    try {
      await onApplyCorrection({
        projectId,
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
    } catch {
      // Open failures are non-fatal for inspector actions.
    }
  }

  return (
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
                onClick={() => void handleOpenArtifact(selectedSourcePath, false)}
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
                onClick={() => void handleOpenArtifact(selectedMarkdownPath, false)}
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
                onClick={() => void handleOpenArtifact(selectedSourcePath, true)}
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
            <EvidenceCard evidence={evidence} key={evidence.id} />
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
  );
}

export function workspaceSelectionKindLabel(
  kind: WorkspaceProject["nodes"][number]["kind"],
): string {
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

export function customerVisibleDescription(value: string): string | null {
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
