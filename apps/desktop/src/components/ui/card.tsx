import { type HTMLAttributes, type PropsWithChildren } from "react";

import { cn } from "@/lib/utils";

interface CardProps extends HTMLAttributes<HTMLDivElement> {
  asChild?: never;
}

export function Card({ className, ...props }: CardProps) {
  return <section className={cn("ui-card", className)} {...props} />;
}

export function CardHeader({ className, ...props }: CardProps) {
  return <header className={cn("ui-card-header", className)} {...props} />;
}

export function CardTitle({ className, ...props }: PropsWithChildren<HTMLAttributes<HTMLHeadingElement>>) {
  return <h2 className={cn("ui-card-title", className)} {...props} />;
}

export function CardDescription({ className, ...props }: PropsWithChildren<HTMLAttributes<HTMLParagraphElement>>) {
  return <p className={cn("ui-card-description", className)} {...props} />;
}

export function CardContent({ className, ...props }: PropsWithChildren<HTMLAttributes<HTMLDivElement>>) {
  return <div className={cn("ui-card-content", className)} {...props} />;
}

export function CardFooter({ className, ...props }: PropsWithChildren<HTMLAttributes<HTMLDivElement>>) {
  return <footer className={cn("ui-card-footer", className)} {...props} />;
}
