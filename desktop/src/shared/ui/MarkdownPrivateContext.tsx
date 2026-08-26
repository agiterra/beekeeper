import { LockKeyhole } from "lucide-react";
import type * as React from "react";

/** Compact visible stand-in for context intentionally removed before signing. */
export function MarkdownPrivateContext({
  children,
  node: _node,
  ...props
}: React.ComponentPropsWithoutRef<"span"> & { node?: unknown }) {
  return (
    <span
      {...props}
      className="inline-flex items-center gap-1 rounded-md border border-border/60 bg-muted/55 px-1.5 py-0.5 align-baseline text-xs leading-none text-muted-foreground"
    >
      <LockKeyhole aria-hidden className="size-3" />
      {children}
    </span>
  );
}
