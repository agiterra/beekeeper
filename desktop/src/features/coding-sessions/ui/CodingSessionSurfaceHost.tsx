import * as React from "react";

import { cn } from "@/shared/lib/cn";
import { Sheet, SheetContent, SheetTitle } from "@/shared/ui/sheet";
import { CodingSessionSurfaceLauncher } from "./CodingSessionSurfaceLauncher";
import { CodingSessionSurfaceTabStrip } from "./CodingSessionSurfaceTabStrip";
import type { CodingSessionSurfaceCtx } from "./surfaces/codingSessionSurfaceContext";
import type { CodingSessionResolvedSurface } from "./surfaces/codingSessionSurfaceRegistry";
import type {
  CodingSessionSurfacePanelActions,
  CodingSessionSurfacePanelState,
} from "./surfaces/useCodingSessionSurfacePanels";
import {
  CODING_SESSION_RAIL_MAX_WIDTH_PX,
  CODING_SESSION_RAIL_MIN_WIDTH_PX,
  useCodingSessionRailWidth,
} from "./useCodingSessionRailWidth";

export { nextCodingSessionSurfaceTabIndex } from "./CodingSessionSurfaceTabStrip";

/**
 * How the host should present itself. `null` means the workspace width is
 * not measured yet — the host renders nothing rather than briefly mounting
 * the inline panel and replacing it with an animated sheet.
 */
export type CodingSessionSurfaceHostLayout = "inline" | "sheet" | null;

/**
 * The session's right panel (SV-21): the tab strip, then the active
 * surface's panel or — with no active tab — the "Open a surface" launcher.
 *
 * Surfaces come from the registry (SV-38) as definitions; the host owns all
 * chrome: tabs, "+", expand, close, width/resize/persistence, and the sheet
 * on narrow windows. Mounted only while the panel is open; the workspace owns
 * the state through `useCodingSessionSurfacePanels`.
 */
export function CodingSessionSurfaceHost({
  ctx,
  hostId,
  layout,
  panels,
  surfaces,
  widthContainerRef,
}: {
  ctx: CodingSessionSurfaceCtx;
  /** DOM id the header's surface affordances point `aria-controls` at. */
  hostId?: string;
  layout: CodingSessionSurfaceHostLayout;
  panels: {
    state: CodingSessionSurfacePanelState;
    actions: CodingSessionSurfacePanelActions;
  };
  /** The right-placement surfaces the lens lists, availability resolved. */
  surfaces: readonly CodingSessionResolvedSurface[];
  /** Element whose width bounds the inline panel's resizable width. */
  widthContainerRef: React.RefObject<HTMLElement | null>;
}) {
  const { cancelDrag, onResizeKeyDown, onResizeStart, width } =
    useCodingSessionRailWidth(widthContainerRef);
  const expanded = panels.state.expanded && layout === "inline";

  // A drag interrupted by a breakpoint transition must still restore
  // document.body cursor/user-select; unmount teardown is in the hook.
  React.useEffect(() => {
    if (layout !== "inline") cancelDrag();
  }, [cancelDrag, layout]);

  if (layout === null) return null;

  if (layout === "sheet") {
    return (
      <Sheet
        onOpenChange={(open) => {
          if (!open) panels.actions.closeRight();
        }}
        open
      >
        <SheetContent
          aria-describedby={undefined}
          className="flex w-[min(94vw,30rem)] max-w-none flex-col p-0"
          side="right"
        >
          <SheetTitle className="sr-only">Session surfaces</SheetTitle>
          {/* SheetContent renders the sheet's single close control; the host
              must not add a second one in the same corner. */}
          <SurfaceHostBody
            ctx={ctx}
            expanded={false}
            panels={panels}
            showPanelControls={false}
            surfaces={surfaces}
          />
        </SheetContent>
      </Sheet>
    );
  }

  return (
    <aside
      aria-label="Session surfaces"
      className={cn(
        "relative flex min-h-0 flex-col border-l border-border/60 bg-background",
        expanded ? "min-w-0 flex-1" : "shrink-0",
      )}
      data-expanded={expanded ? "true" : undefined}
      data-testid="coding-session-surface-host"
      id={hostId}
      style={expanded ? undefined : { width }}
    >
      {expanded ? null : (
        // biome-ignore lint/a11y/useSemanticElements: this separator is interactive (pointer drag + arrow-key resizing with value semantics); an <hr> cannot take focus or carry aria-valuenow.
        <button
          aria-label="Resize session surface"
          aria-orientation="vertical"
          aria-valuemax={CODING_SESSION_RAIL_MAX_WIDTH_PX}
          aria-valuemin={CODING_SESSION_RAIL_MIN_WIDTH_PX}
          aria-valuenow={Math.round(width)}
          className="group absolute inset-y-0 -left-1 z-30 w-2 cursor-col-resize touch-none outline-none"
          data-testid="coding-session-surface-resize"
          onKeyDown={onResizeKeyDown}
          onPointerDown={onResizeStart}
          role="separator"
          type="button"
        >
          <span className="absolute inset-y-0 left-1/2 w-px bg-transparent transition-colors group-hover:bg-primary/70 group-focus-visible:bg-primary" />
        </button>
      )}
      <SurfaceHostBody
        ctx={ctx}
        expanded={expanded}
        panels={panels}
        showPanelControls
        surfaces={surfaces}
      />
    </aside>
  );
}

function SurfaceHostBody({
  ctx,
  expanded,
  panels,
  showPanelControls,
  surfaces,
}: {
  ctx: CodingSessionSurfaceCtx;
  expanded: boolean;
  panels: {
    state: CodingSessionSurfacePanelState;
    actions: CodingSessionSurfacePanelActions;
  };
  showPanelControls: boolean;
  surfaces: readonly CodingSessionResolvedSurface[];
}) {
  // Instance-scoped tab/panel ids: popouts or multiple mounted workspaces
  // must never mint colliding DOM ids.
  const idBase = React.useId();
  const byId = new Map(
    surfaces.map((surface) => [surface.definition.id, surface.definition]),
  );
  const tabs = panels.state.tabs.flatMap((id) => {
    const definition = byId.get(id);
    return definition ? [definition] : [];
  });
  const active =
    panels.state.active === null
      ? null
      : (byId.get(panels.state.active) ?? null);
  const Panel = active?.Panel ?? null;

  return (
    <div className="flex h-full min-h-0 flex-1 flex-col">
      <CodingSessionSurfaceTabStrip
        activeId={active?.id ?? null}
        ctx={ctx}
        expanded={expanded}
        idBase={idBase}
        onActivate={panels.actions.activate}
        onClosePanel={panels.actions.closeRight}
        onCloseTab={panels.actions.close}
        onCloseTabs={panels.actions.closeMany}
        onOpen={panels.actions.open}
        onToggleExpanded={panels.actions.toggleExpanded}
        showPanelControls={showPanelControls}
        surfaces={surfaces}
        tabs={tabs}
      />
      {active && Panel ? (
        <div
          aria-labelledby={`${idBase}-tab-${active.id}`}
          className="flex min-h-0 flex-1 flex-col"
          id={`${idBase}-panel-${active.id}`}
          role="tabpanel"
        >
          <Panel ctx={ctx} />
        </div>
      ) : (
        <CodingSessionSurfaceLauncher
          ctx={ctx}
          onOpen={panels.actions.open}
          surfaces={surfaces}
        />
      )}
    </div>
  );
}
