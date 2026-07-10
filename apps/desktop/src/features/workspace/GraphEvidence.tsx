import { fileNameFromPath } from "./pathUtils";
import type { WorkspaceEvidenceRef } from "./types";

export function EvidenceCard(props: { evidence: WorkspaceEvidenceRef }) {
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

export function formatEvidenceSnippet(value: string): string {
  return (
    value
      .replace(/!\[[^\]]*\]\([^)]+\)/g, "")
      .replace(/\s+/g, " ")
      .trim() || "No text evidence is available for this page yet."
  );
}

function extractMarkdownImageLabel(value: string): string | null {
  return value.match(/!\[([^\]]*)\]\(([^)]+)\)/)?.[1]?.trim() || null;
}
