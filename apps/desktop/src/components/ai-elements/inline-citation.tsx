"use client";

import type { ComponentProps } from "react";

import { cn } from "@/lib/utils";

export type InlineCitationProps = ComponentProps<"span">;

export const InlineCitation = ({ className, ...props }: InlineCitationProps) => (
  <span
    className={cn("group/citation relative inline-flex items-baseline", className)}
    {...props}
  />
);

export type InlineCitationTextProps = ComponentProps<"span">;

export const InlineCitationText = ({ className, ...props }: InlineCitationTextProps) => (
  <span className={cn("transition-colors group-hover/citation:bg-accent", className)} {...props} />
);

export type InlineCitationCardProps = ComponentProps<"span">;

export const InlineCitationCard = ({ className, ...props }: InlineCitationCardProps) => (
  <span className={cn("relative inline-flex", className)} {...props} />
);

export type InlineCitationCardTriggerProps = ComponentProps<"button"> & {
  sources: string[];
};

export const InlineCitationCardTrigger = ({
  sources,
  className,
  children,
  ...props
}: InlineCitationCardTriggerProps) => (
  <button
    className={cn(
      "ml-1 inline-flex h-5 min-w-5 items-center justify-center rounded-full border border-border bg-muted px-1.5 align-baseline text-[11px] font-medium leading-none text-foreground shadow-sm transition-colors hover:bg-background focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring",
      className,
    )}
    type="button"
    {...props}
  >
    {children ?? (sources.length > 1 ? `${sources[0]} +${sources.length - 1}` : sources[0] ?? "source")}
  </button>
);

export type InlineCitationCardBodyProps = ComponentProps<"span">;

export const InlineCitationCardBody = ({ className, ...props }: InlineCitationCardBodyProps) => (
  <span
    className={cn(
      "invisible absolute bottom-full left-1/2 z-50 mb-2 block max-h-72 w-96 max-w-[calc(100vw-2rem)] -translate-x-1/2 overflow-y-auto rounded-xl border border-border bg-background p-3 text-left text-foreground opacity-0 shadow-lg transition-opacity group-hover/citation:visible group-hover/citation:opacity-100 group-focus-within/citation:visible group-focus-within/citation:opacity-100",
      className,
    )}
    {...props}
  />
);

export type InlineCitationSourceProps = ComponentProps<"div"> & {
  title?: string;
  url?: string;
  description?: string;
};

export const InlineCitationSource = ({
  title,
  url,
  description,
  className,
  children,
  ...props
}: InlineCitationSourceProps) => (
  <div className={cn("space-y-1", className)} {...props}>
    {title ? <h4 className="truncate text-sm font-medium leading-tight">{title}</h4> : null}
    {url ? <p className="truncate break-all text-xs text-muted-foreground">{url}</p> : null}
    {description ? (
      <p className="line-clamp-3 text-sm leading-relaxed text-muted-foreground">{description}</p>
    ) : null}
    {children}
  </div>
);

export type InlineCitationQuoteProps = ComponentProps<"blockquote">;

export const InlineCitationQuote = ({ children, className, ...props }: InlineCitationQuoteProps) => (
  <blockquote
    className={cn(
      "whitespace-pre-wrap break-words border-l-2 border-muted pl-3 text-sm leading-6 text-muted-foreground",
      className,
    )}
    {...props}
  >
    {children}
  </blockquote>
);
