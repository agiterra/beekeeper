import type * as React from "react";

import { cn } from "@/shared/lib/cn";

import type { CodingSessionSurfaceIcon } from "./codingSessionSurfaceRegistry";

/**
 * A surface that cannot show its content here, saying why (SV-23).
 *
 * T3 Code's empty panel chrome (`device/DevicePanel.tsx`,
 * `RightPanelTabs.tsx:548-570`): the surface's icon in a muted tile, its
 * name, and one sentence. Device and Browser use it as their whole panel
 * (dimmed: they arrive with SV-33 and SV-34); every other surface uses it
 * when a tab stays open after the surface stopped being available, so the
 * reason a person reads in the launcher's tooltip is the reason the panel
 * shows. The reason is never shortened or reworded here.
 */
export function CodingSessionSurfacePlaceholder({
  children,
  dimmed = false,
  icon: Icon,
  id,
  label,
  reason,
}: {
  children?: React.ReactNode;
  /** Draw the whole panel at reduced emphasis: the surface is not built. */
  dimmed?: boolean;
  icon: CodingSessionSurfaceIcon;
  id: string;
  label: string;
  reason: string;
}) {
  return (
    <div
      className="flex min-h-0 flex-1 flex-col items-center justify-center gap-3 overflow-y-auto px-6 py-10 text-center"
      data-available="false"
      data-testid={`coding-session-surface-panel-${id}`}
    >
      <span
        className={cn(
          "flex size-10 items-center justify-center rounded-lg border border-border/60 bg-muted/30 text-muted-foreground",
          dimmed && "opacity-60",
        )}
      >
        <Icon aria-hidden className="size-5" />
      </span>
      <div className={cn("space-y-1", dimmed && "opacity-70")}>
        <p className="text-sm font-medium">{label}</p>
        <p
          className="max-w-64 text-xs leading-5 text-muted-foreground"
          data-testid={`coding-session-surface-reason-${id}`}
        >
          {reason}
        </p>
      </div>
      {children}
    </div>
  );
}
