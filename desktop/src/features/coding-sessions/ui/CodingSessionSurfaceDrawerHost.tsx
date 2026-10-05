import { X } from "lucide-react";

import type { CodingSessionSurfaceCtx } from "./surfaces/codingSessionSurfaceContext";
import type { CodingSessionResolvedSurface } from "./surfaces/codingSessionSurfaceRegistry";

/**
 * The bottom drawer under the composer (SV-21 mount point; SV-25 fills it).
 *
 * Renders every drawer-placement surface the lens lists while the drawer is
 * open (⌘J). Each surface gets a toolbar first, as T3 Code's
 * `ThreadTerminalDrawer` always has one: the surface's icon and name, and a
 * control that hides the drawer (⌘J). It says "Hide", not "Close": it closes
 * no terminal, and the Terminal panel's own "Close terminal" does. Beneath
 * it is the panel, and a surface that cannot open here still renders its
 * panel, which says why — so the drawer always names
 * what it is, and never shows a bare sentence that reads as an orphan error.
 */
export function CodingSessionSurfaceDrawerHost({
  ctx,
  open,
  surfaces,
}: {
  ctx: CodingSessionSurfaceCtx;
  open: boolean;
  /** Drawer-placement surfaces for the lens, availability resolved. */
  surfaces: readonly CodingSessionResolvedSurface[];
}) {
  if (!open || surfaces.length === 0) return null;
  return (
    <section
      aria-label="Session drawer"
      className="relative z-20 flex min-h-0 shrink-0 flex-col border-t border-border/60 bg-background"
      data-testid="coding-session-drawer-host"
    >
      {surfaces.map(({ definition }) => {
        const Panel = definition.Panel;
        const Icon = definition.icon;
        const headingId = `coding-session-drawer-${definition.id}-heading`;
        return (
          <section
            aria-labelledby={headingId}
            className="flex min-h-0 flex-1 flex-col"
            data-surface={definition.id}
            key={definition.id}
          >
            <div
              className="flex h-7 shrink-0 items-center gap-1.5 border-b border-border/60 pr-1 pl-3 text-xs"
              data-testid={`coding-session-drawer-toolbar-${definition.id}`}
            >
              <Icon aria-hidden className="size-3.5 text-muted-foreground" />
              <h2
                className="min-w-0 flex-1 truncate font-medium"
                id={headingId}
              >
                {definition.label}
              </h2>
              <button
                aria-label={`Hide ${definition.label}`}
                className="flex size-5 shrink-0 items-center justify-center rounded-md text-muted-foreground hover:bg-muted hover:text-foreground focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring"
                data-testid={`coding-session-drawer-close-${definition.id}`}
                onClick={ctx.panels.toggleBottom}
                title={`Hide ${definition.label} (⌘J)`}
                type="button"
              >
                <X aria-hidden className="size-3.5" />
              </button>
            </div>
            <div className="flex min-h-0 flex-1 flex-col">
              <Panel ctx={ctx} />
            </div>
          </section>
        );
      })}
    </section>
  );
}
