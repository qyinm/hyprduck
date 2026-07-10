import { useEffect, useMemo, useRef, useState } from "react";
import {
  ChevronLeft,
  ChevronRight,
  ExternalLink,
  Maximize2,
  Minimize2,
  Minus,
  Plus,
  RotateCcw,
} from "lucide-react";
import { Document, Page, pdfjs } from "react-pdf";

import { cn } from "@/lib/utils";

import {
  IconAction,
  SourceDetailLoading,
  UnsupportedOriginalPreview,
  clamp,
} from "./sourceDetailShared";

pdfjs.GlobalWorkerOptions.workerSrc = new URL(
  "pdfjs-dist/build/pdf.worker.min.mjs",
  import.meta.url,
).toString();

const PDF_PAGE_GAP = 20;
const PDF_PAGE_OVERSCAN = 1;
const DEFAULT_PDF_PAGE_ASPECT_RATIO = 1.294;

export function PdfOriginalPreview({
  onOpenOriginal,
  previewUrl,
}: {
  onOpenOriginal: () => void;
  previewUrl: string;
}) {
  const [currentPage, setCurrentPage] = useState(1);
  const [isFullscreen, setIsFullscreen] = useState(false);
  const [numPages, setNumPages] = useState(0);
  const [pageAspectRatio, setPageAspectRatio] = useState(DEFAULT_PDF_PAGE_ASPECT_RATIO);
  const [pageWidth, setPageWidth] = useState(720);
  const [previewError, setPreviewError] = useState<string | null>(null);
  const [visiblePageRange, setVisiblePageRange] = useState({ start: 1, end: 1 });
  const [zoomPercent, setZoomPercent] = useState(100);
  const previewContainerRef = useRef<HTMLDivElement | null>(null);
  const zoomScale = zoomPercent / 100;
  const pageDisplayWidth = Math.ceil(pageWidth * zoomScale);
  const pageDisplayHeight = Math.ceil(pageWidth * pageAspectRatio * zoomScale);
  const pageItemHeight = pageDisplayHeight + PDF_PAGE_GAP;
  const virtualHeight = numPages > 0 ? numPages * pageItemHeight : 0;
  const visiblePageNumbers = useMemo(() => {
    const pages: number[] = [];
    for (let page = visiblePageRange.start; page <= visiblePageRange.end; page += 1) {
      pages.push(page);
    }
    return pages;
  }, [visiblePageRange.end, visiblePageRange.start]);

  const updateVisiblePages = () => {
    const container = previewContainerRef.current;
    if (!container || numPages === 0 || pageItemHeight <= 0) {
      return;
    }

    const scrollTop = container.scrollTop;
    const viewportHeight = container.clientHeight;
    const nextStart = clamp(
      Math.floor(scrollTop / pageItemHeight) + 1 - PDF_PAGE_OVERSCAN,
      1,
      numPages,
    );
    const nextEnd = clamp(
      Math.ceil((scrollTop + viewportHeight) / pageItemHeight) + PDF_PAGE_OVERSCAN,
      nextStart,
      numPages,
    );
    setVisiblePageRange((range) =>
      range.start === nextStart && range.end === nextEnd
        ? range
        : { start: nextStart, end: nextEnd },
    );

    const nextCurrentPage = clamp(
      Math.floor((scrollTop + viewportHeight / 2) / pageItemHeight) + 1,
      1,
      numPages,
    );
    setCurrentPage((page) => (page === nextCurrentPage ? page : nextCurrentPage));
  };

  useEffect(() => {
    const container = previewContainerRef.current;
    if (!container) {
      return;
    }

    const updateWidth = () => {
      const nextWidth = Math.max(320, Math.min(760, container.clientWidth - 56));
      setPageWidth(nextWidth);
    };
    updateWidth();
    const resizeObserver = new ResizeObserver(updateWidth);
    resizeObserver.observe(container);
    return () => resizeObserver.disconnect();
  }, [isFullscreen]);

  useEffect(() => {
    if (!isFullscreen) {
      return;
    }
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key === "Escape") {
        setIsFullscreen(false);
      }
    };
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [isFullscreen]);

  useEffect(() => {
    updateVisiblePages();
  }, [isFullscreen, numPages, pageItemHeight]);

  const goToPage = (page: number) => {
    if (numPages === 0) {
      return;
    }
    const targetPage = clamp(page, 1, numPages);
    setCurrentPage(targetPage);
    previewContainerRef.current?.scrollTo({
      behavior: "smooth",
      top: (targetPage - 1) * pageItemHeight,
    });
  };

  const setZoom = (nextZoom: number) => {
    const nextZoomPercent = clamp(nextZoom, 50, 180);
    setZoomPercent(nextZoomPercent);
    const container = previewContainerRef.current;
    if (container && numPages > 0) {
      const nextScale = nextZoomPercent / 100;
      const nextPageItemHeight =
        Math.ceil(pageWidth * pageAspectRatio * nextScale) + PDF_PAGE_GAP;
      window.requestAnimationFrame(() => {
        container.scrollTo({ top: (currentPage - 1) * nextPageItemHeight });
      });
    }
  };

  if (previewError) {
    return <UnsupportedOriginalPreview message={previewError} onOpenOriginal={onOpenOriginal} />;
  }

  return (
    <div
      data-electron-no-drag
      className={cn(
        "flex h-full min-h-0 flex-col bg-muted/20",
        isFullscreen &&
          "fixed inset-x-4 bottom-4 top-12 z-[60] overflow-hidden rounded-xl border border-border bg-background shadow-2xl",
      )}
    >
      <div
        className="flex min-h-11 flex-wrap items-center justify-between gap-2 border-b border-border bg-background px-3 py-2"
        data-electron-no-drag
      >
        <div className="flex items-center gap-1">
          <IconAction
            disabled={numPages === 0 || currentPage <= 1}
            label="Previous page"
            onClick={() => goToPage(currentPage - 1)}
          >
            <ChevronLeft size={15} />
          </IconAction>
          <label className="flex items-center gap-1 text-xs text-muted-foreground">
            <span className="sr-only">Current page</span>
            <input
              aria-label="Current page"
              className="h-7 w-12 rounded-md border border-border bg-background px-2 text-center text-xs text-foreground"
              disabled={numPages === 0}
              max={numPages || 1}
              min={1}
              onChange={(event) => {
                const nextPage = Number.parseInt(event.currentTarget.value, 10);
                if (Number.isFinite(nextPage)) {
                  goToPage(nextPage);
                }
              }}
              type="number"
              value={currentPage}
            />
            <span>/ {numPages || "..."}</span>
          </label>
          <IconAction
            disabled={numPages === 0 || currentPage >= numPages}
            label="Next page"
            onClick={() => goToPage(currentPage + 1)}
          >
            <ChevronRight size={15} />
          </IconAction>
        </div>

        <div className="flex items-center gap-1">
          <IconAction label="Zoom out" onClick={() => setZoom(zoomPercent - 10)}>
            <Minus size={15} />
          </IconAction>
          <button
            className="h-7 min-w-12 rounded-md px-2 text-xs font-medium text-muted-foreground hover:bg-muted"
            onClick={() => setZoom(100)}
            title="Reset zoom"
            type="button"
          >
            {zoomPercent}%
          </button>
          <IconAction label="Zoom in" onClick={() => setZoom(zoomPercent + 10)}>
            <Plus size={15} />
          </IconAction>
          <IconAction label="Reset zoom" onClick={() => setZoom(100)}>
            <RotateCcw size={15} />
          </IconAction>
          <IconAction label="Open original" onClick={onOpenOriginal}>
            <ExternalLink size={15} />
          </IconAction>
          <IconAction
            label={isFullscreen ? "Exit fullscreen" : "Fullscreen preview"}
            onClick={() => setIsFullscreen((value) => !value)}
          >
            {isFullscreen ? <Minimize2 size={15} /> : <Maximize2 size={15} />}
          </IconAction>
        </div>
      </div>

      <div
        className="min-h-0 flex-1 overflow-auto bg-muted/40 px-6 py-5"
        onScroll={updateVisiblePages}
        ref={previewContainerRef}
      >
        <Document
          className="min-h-full"
          file={previewUrl}
          loading={<SourceDetailLoading label="Loading PDF pages..." />}
          onLoadError={(error) => setPreviewError(error.message || "PDF preview could not be loaded.")}
          onLoadSuccess={(pdf) => {
            const loadedPageCount = pdf.numPages;
            setPreviewError(null);
            setNumPages(loadedPageCount);
            setCurrentPage((page) => clamp(page, 1, loadedPageCount));
            void pdf
              .getPage(1)
              .then((page) => {
                const viewport = page.getViewport({ scale: 1 });
                if (viewport.width > 0) {
                  setPageAspectRatio(viewport.height / viewport.width);
                }
              })
              .catch(() => undefined);
          }}
        >
          {numPages > 0 ? (
            <div
              className="relative mx-auto"
              style={{
                height: virtualHeight,
                width: pageDisplayWidth,
              }}
            >
              {visiblePageNumbers.map((pageNumber) => (
                <div
                  className="absolute left-1/2 rounded-sm bg-background shadow-sm"
                  key={`${pageNumber}-${pageWidth}-${zoomPercent}`}
                  style={{
                    top: (pageNumber - 1) * pageItemHeight,
                    transform: "translateX(-50%)",
                  }}
                >
                  <Page
                    className="overflow-hidden"
                    loading={
                      <div className="flex h-64 items-center justify-center text-xs text-muted-foreground">
                        Loading page...
                      </div>
                    }
                    pageNumber={pageNumber}
                    renderAnnotationLayer={false}
                    renderTextLayer={false}
                    scale={zoomScale}
                    width={pageWidth}
                  />
                </div>
              ))}
            </div>
          ) : null}
        </Document>
      </div>
    </div>
  );
}
