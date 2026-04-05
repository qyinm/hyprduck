import { type HTMLAttributes } from "react";

import { cn } from "@/lib/utils";

type BadgeVariant = "default" | "secondary" | "outline";

interface BadgeProps extends HTMLAttributes<HTMLSpanElement> {
  variant?: BadgeVariant;
}

export function Badge({ className, variant = "default", ...props }: BadgeProps) {
  const variantClass = {
    default: "ui-badge-default",
    secondary: "ui-badge-secondary",
    outline: "ui-badge-outline",
  }[variant];

  return <span className={cn("ui-badge", variantClass, className)} {...props} />;
}
