import { type LabelHTMLAttributes, forwardRef } from "react";

import { cn } from "@/lib/utils";

export const Label = forwardRef<HTMLLabelElement, LabelHTMLAttributes<HTMLLabelElement>>(
  ({ className, ...props }, ref) => {
    return <label ref={ref} className={cn("ui-label", className)} {...props} />;
  },
);

Label.displayName = "Label";
