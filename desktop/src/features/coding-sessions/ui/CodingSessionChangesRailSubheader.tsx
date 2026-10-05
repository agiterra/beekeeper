import type * as React from "react";

import { cn } from "@/shared/lib/cn";

/**
 * The 40px bar at the top of a surface's content, under the host's tab strip
 * — T3 Code's `data-surface-subheader` row (`DiffPanelShell.tsx`,
 * `FileBrowserPanel.tsx`: `h-10 min-h-10 border-b border-border/60`). Shared
 * by Diff, Files, Plan, Landing, People and Pulse so every surface opens on
 * the same line: what it is, and the one sentence that says where its
 * content comes from.
 */
export function CodingSessionSurfaceSubheader({
  actions,
  className,
  meta,
  testId,
  title,
}: {
  /** Controls at the right edge (refresh, open elsewhere). */
  actions?: React.ReactNode;
  className?: string;
  /** Muted text beside the title: the provenance in a few words. */
  meta?: React.ReactNode;
  testId?: string;
  title: React.ReactNode;
}) {
  return (
    <div
      className={cn(
        "flex h-10 min-h-10 shrink-0 items-center gap-2 border-b border-border/60 bg-background px-3",
        className,
      )}
      data-surface-subheader
      data-testid={testId}
    >
      <h2 className="shrink-0 text-xs font-semibold">{title}</h2>
      {meta ? (
        <span className="min-w-0 flex-1 truncate text-2xs text-muted-foreground">
          {meta}
        </span>
      ) : (
        <span className="flex-1" />
      )}
      {actions ? (
        <div className="flex shrink-0 items-center gap-1">{actions}</div>
      ) : null}
    </div>
  );
}
