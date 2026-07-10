import { type ReactNode } from "react";
import { FileText, Loader2 } from "lucide-react";

import type { SourceDetailResult } from "@/appTypes";
import { Button } from "@/components/ui/button";
import { cn } from "@/lib/utils";

export type SourceDetailLoadState =
  | { status: "idle"; data: null; error: null }
  | { status: "loading"; data: null; error: null }
  | { status: "ready"; data: SourceDetailResult; error: null }
  | { status: "error"; data: null; error: string };

export function SourceDetailPanel({
  actions,
  children,
  className,
  title,
}: {
  actions?: ReactNode;
  children: ReactNode;
  className?: string;
  title: string;
}) {
  return (
    <section
      className={cn(
        "flex min-h-0 flex-col overflow-hidden rounded-lg border border-border bg-background",
        className,
      )}
    >
      <div className="flex min-h-11 items-center justify-between gap-3 border-b border-border px-4 py-2">
        <h2 className="text-sm font-semibold text-foreground">{title}</h2>
        {actions}
      </div>
      <div className="min-h-0 flex-1 overflow-hidden">{children}</div>
    </section>
  );
}

export function SourceDetailLoading({ label }: { label: string }) {
  return (
    <div className="flex h-full min-h-[20rem] items-center justify-center gap-2 text-sm text-muted-foreground">
      <Loader2 className="size-4 animate-spin" />
      <span>{label}</span>
    </div>
  );
}

export function SourceDetailError({
  actionLabel,
  message,
  onAction,
}: {
  actionLabel?: string;
  message: string;
  onAction?: () => void;
}) {
  return (
    <div className="flex h-full min-h-[20rem] flex-col items-center justify-center gap-3 px-6 text-center">
      <p className="max-w-md text-sm text-muted-foreground">{message}</p>
      {actionLabel && onAction ? (
        <Button onClick={onAction} size="sm" type="button" variant="outline">
          {actionLabel}
        </Button>
      ) : null}
    </div>
  );
}

export function UnsupportedOriginalPreview({
  message,
  onOpenOriginal,
}: {
  message: string;
  onOpenOriginal: () => void;
}) {
  return (
    <div className="flex h-full min-h-[20rem] flex-col items-center justify-center gap-3 px-6 text-center">
      <FileText className="size-8 text-muted-foreground" />
      <p className="max-w-md text-sm text-muted-foreground">{message}</p>
      <Button onClick={onOpenOriginal} size="sm" type="button" variant="outline">
        Open original
      </Button>
    </div>
  );
}

export function IconAction({
  children,
  disabled,
  label,
  onClick,
}: {
  children: ReactNode;
  disabled?: boolean;
  label: string;
  onClick: () => void;
}) {
  return (
    <Button
      aria-label={label}
      disabled={disabled}
      onClick={onClick}
      size="icon"
      title={label}
      type="button"
      variant="ghost"
    >
      {children}
    </Button>
  );
}

export function SegmentButton({
  active,
  children,
  onClick,
}: {
  active: boolean;
  children: ReactNode;
  onClick: () => void;
}) {
  return (
    <button
      className={cn(
        "rounded px-2.5 py-1 text-xs font-medium text-muted-foreground transition",
        active
          ? "bg-background text-foreground shadow-xs"
          : "hover:bg-background/70 hover:text-foreground",
      )}
      onClick={onClick}
      type="button"
    >
      {children}
    </button>
  );
}

export function clamp(value: number, min: number, max: number) {
  return Math.max(min, Math.min(max, value));
}
