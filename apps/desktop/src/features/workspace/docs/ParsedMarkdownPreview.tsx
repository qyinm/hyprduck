import { useEffect, useState } from "react";
import { Copy as CopyIcon } from "lucide-react";

import { MessageResponse } from "@/components/ai-elements/message";
import { Button } from "@/components/ui/button";

import {
  SegmentButton,
  SourceDetailError,
  SourceDetailLoading,
  SourceDetailPanel,
  type SourceDetailLoadState,
} from "./sourceDetailShared";

type MarkdownViewMode = "preview" | "raw";

export function ParsedMarkdownPreview({
  detailState,
  onRetry,
}: {
  detailState: SourceDetailLoadState;
  onRetry: () => void;
}) {
  const [viewMode, setViewMode] = useState<MarkdownViewMode>("raw");
  const [copyLabel, setCopyLabel] = useState("Copy");

  useEffect(() => {
    if (copyLabel === "Copy") {
      return;
    }
    const timeoutId = window.setTimeout(() => setCopyLabel("Copy"), 1400);
    return () => window.clearTimeout(timeoutId);
  }, [copyLabel]);

  if (detailState.status === "loading" || detailState.status === "idle") {
    return (
      <SourceDetailPanel title="Parsed Markdown">
        <SourceDetailLoading label="Loading parsed markdown..." />
      </SourceDetailPanel>
    );
  }
  if (detailState.status === "error") {
    return (
      <SourceDetailPanel title="Parsed Markdown">
        <SourceDetailError actionLabel="Retry" message={detailState.error} onAction={onRetry} />
      </SourceDetailPanel>
    );
  }
  const markdown = detailState.data.markdown;
  if (markdown.missing || !markdown.text) {
    return (
      <SourceDetailPanel title="Parsed Markdown">
        <SourceDetailError
          actionLabel="Retry"
          message={markdown.error ?? "Parsed markdown not available yet."}
          onAction={onRetry}
        />
      </SourceDetailPanel>
    );
  }
  const markdownText = markdown.text;

  const copyMarkdown = () => {
    void navigator.clipboard
      .writeText(markdownText)
      .then(() => setCopyLabel("Copied"))
      .catch(() => setCopyLabel("Copy failed"));
  };

  return (
    <SourceDetailPanel
      actions={
        <div className="flex items-center gap-2">
          <Button onClick={copyMarkdown} size="sm" type="button" variant="ghost">
            <CopyIcon className="mr-1.5 size-3.5" />
            {copyLabel}
          </Button>
          <div className="flex rounded-md border border-border bg-muted/20 p-0.5">
            <SegmentButton active={viewMode === "preview"} onClick={() => setViewMode("preview")}>
              Preview
            </SegmentButton>
            <SegmentButton active={viewMode === "raw"} onClick={() => setViewMode("raw")}>
              Raw
            </SegmentButton>
          </div>
        </div>
      }
      title="Parsed Markdown"
    >
      {viewMode === "preview" ? (
        <div className="source-detail-markdown h-full overflow-auto p-5">
          <MessageResponse>{markdownText}</MessageResponse>
        </div>
      ) : (
        <div className="h-full overflow-auto bg-muted/20 p-4">
          <pre className="min-h-full whitespace-pre-wrap break-words rounded-md bg-background p-4 text-xs leading-6 text-foreground shadow-xs">
            {markdownText}
          </pre>
        </div>
      )}
    </SourceDetailPanel>
  );
}

/** @deprecated Prefer ParsedMarkdownPreview */
export const ParsedMarkdownPreviewPanel = ParsedMarkdownPreview;
