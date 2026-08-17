import * as React from "react";
import { PanelRightClose } from "lucide-react";

import { cn } from "@/shared/lib/cn";
import { Sheet, SheetContent, SheetTitle } from "@/shared/ui/sheet";
import {
  CODING_SESSION_RAIL_MAX_WIDTH_PX,
  CODING_SESSION_RAIL_MIN_WIDTH_PX,
  useCodingSessionRailWidth,
} from "./useCodingSessionRailWidth";

/**
 * One entry in the workspace's surface registry. This is the extensibility
 * seam for future surfaces (Browser, Terminal, Files): a surface contributes
 * an id, a tab label, an optional count badge, and its content — nothing
 * else. The host owns every piece of panel chrome: the tab strip, the close
 * control, width/resize/persistence, and narrow-window sheet behavior.
 */
export type CodingSessionSurfaceDescriptor = {
  id: string;
  label: string;
  /** Small tabular count rendered on the tab; hidden when absent or zero. */
  count?: number | null;
  content: React.ReactNode;
};

/**
 * How the host should present itself. `null` means the workspace width is
 * not measured yet — the host renders nothing rather than briefly mounting
 * the inline panel and replacing it with an animated sheet.
 */
export type CodingSessionSurfaceHostLayout = "inline" | "sheet" | null;

/**
 * Resolve which surface tab is effectively active. The state contract is
 * "closed or {tab}": when the requested tab's surface disappears from the
 * registry, the selection reconciles to the last remembered tab if still
 * offered, else the first offered surface, else closed — never content with
 * no selected tab.
 */
export function reconcileCodingSessionSurfaceTab({
  availableIds,
  lastTab,
  requestedTab,
}: {
  availableIds: readonly string[];
  lastTab: string | null;
  requestedTab: string | null;
}): string | null {
  if (requestedTab === null) return null;
  if (availableIds.includes(requestedTab)) return requestedTab;
  if (lastTab !== null && availableIds.includes(lastTab)) return lastTab;
  return availableIds[0] ?? null;
}

/** Roving-tabindex keyboard order for the surface tab strip. */
export function nextCodingSessionSurfaceTabIndex(
  key: string,
  currentIndex: number,
  tabCount: number,
): number | null {
  if (tabCount <= 0) return null;
  if (key === "ArrowRight") return (currentIndex + 1) % tabCount;
  if (key === "ArrowLeft") return (currentIndex - 1 + tabCount) % tabCount;
  if (key === "Home") return 0;
  if (key === "End") return tabCount - 1;
  return null;
}

/**
 * The single workspace-owned surface state: closed (`activeTab === null`) or
 * open on one tab. Closing remembers the last active tab so reopening (via a
 * tab affordance or stale-tab fallback) restores it when still available.
 */
export function useCodingSessionSurfaceHostState(
  availableIds: readonly string[],
  options?: { initialTab?: string | null },
): {
  activeTab: string | null;
  close: () => void;
  select: (id: string) => void;
  toggle: (id: string) => void;
} {
  const [requestedTab, setRequestedTab] = React.useState<string | null>(
    options?.initialTab ?? null,
  );
  const lastTabRef = React.useRef<string | null>(options?.initialTab ?? null);
  const activeTab = reconcileCodingSessionSurfaceTab({
    availableIds,
    lastTab: lastTabRef.current,
    requestedTab,
  });
  // Render-phase reconciliation: when the selected surface disappears the
  // stored request is rewritten immediately, so no stale tab id survives a
  // registry change even for one committed frame.
  if (requestedTab !== activeTab) setRequestedTab(activeTab);
  if (activeTab !== null) lastTabRef.current = activeTab;

  const select = React.useCallback((id: string) => setRequestedTab(id), []);
  const close = React.useCallback(() => setRequestedTab(null), []);
  const toggle = React.useCallback(
    (id: string) => setRequestedTab((current) => (current === id ? null : id)),
    [],
  );

  return { activeTab, close, select, toggle };
}

/**
 * The shared right-side surface host. Renders one collapsible, resizable
 * panel (or one sheet on narrow layouts) whose sibling tabs are the offered
 * surfaces. Mounted only while open — the workspace owns open/closed state
 * through {@link useCodingSessionSurfaceHostState}.
 */
export function CodingSessionSurfaceHost({
  activeSurfaceId,
  hostId,
  layout,
  onClose,
  onSelectSurface,
  surfaces,
  widthContainerRef,
}: {
  activeSurfaceId: string;
  /** DOM id the header's surface affordances point `aria-controls` at. */
  hostId?: string;
  layout: CodingSessionSurfaceHostLayout;
  onClose: () => void;
  onSelectSurface: (id: string) => void;
  surfaces: readonly CodingSessionSurfaceDescriptor[];
  /** Element whose width bounds the inline panel's resizable width. */
  widthContainerRef: React.RefObject<HTMLElement | null>;
}) {
  const { cancelDrag, onResizeKeyDown, onResizeStart, width } =
    useCodingSessionRailWidth(widthContainerRef);

  // A drag interrupted by a breakpoint transition must still restore
  // document.body cursor/user-select; unmount teardown is handled inside the
  // hook itself.
  React.useEffect(() => {
    if (layout !== "inline") cancelDrag();
  }, [cancelDrag, layout]);

  if (layout === null) return null;

  if (layout === "sheet") {
    return (
      <Sheet
        onOpenChange={(open) => {
          if (!open) onClose();
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
            activeSurfaceId={activeSurfaceId}
            onClose={onClose}
            onSelectSurface={onSelectSurface}
            showClose={false}
            surfaces={surfaces}
          />
        </SheetContent>
      </Sheet>
    );
  }

  return (
    <aside
      aria-label="Session surfaces"
      className="relative flex min-h-0 shrink-0 flex-col border-l border-border/60 bg-background"
      data-testid="coding-session-surface-host"
      id={hostId}
      style={{ width }}
    >
      {/* biome-ignore lint/a11y/useSemanticElements: this separator is interactive (pointer drag + arrow-key resizing with value semantics); an <hr> cannot take focus or carry aria-valuenow. */}
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
      <SurfaceHostBody
        activeSurfaceId={activeSurfaceId}
        onClose={onClose}
        onSelectSurface={onSelectSurface}
        showClose
        surfaces={surfaces}
      />
    </aside>
  );
}

function SurfaceHostBody({
  activeSurfaceId,
  onClose,
  onSelectSurface,
  showClose,
  surfaces,
}: {
  activeSurfaceId: string;
  onClose: () => void;
  onSelectSurface: (id: string) => void;
  showClose: boolean;
  surfaces: readonly CodingSessionSurfaceDescriptor[];
}) {
  // Instance-scoped tab/panel ids: popouts or multiple mounted workspaces
  // must never mint colliding DOM ids.
  const idBase = React.useId();
  const tabRefs = React.useRef<Array<HTMLButtonElement | null>>([]);
  const activeIndex = surfaces.findIndex(
    (surface) => surface.id === activeSurfaceId,
  );
  const activeSurface = surfaces[activeIndex] ?? null;
  const tabId = (id: string) => `${idBase}-tab-${id}`;
  const panelId = (id: string) => `${idBase}-panel-${id}`;

  return (
    <div className="flex h-full min-h-0 flex-1 flex-col">
      <div
        className={cn(
          "flex h-12 shrink-0 items-center border-b border-border/60 px-3",
          // In sheet mode the strip leaves room for the sheet's own close
          // control in the top-right corner.
          !showClose && "pr-12",
        )}
      >
        <div
          aria-label="Session surface tabs"
          className="flex min-w-0 flex-1 items-center gap-1 overflow-x-auto"
          onKeyDown={(event) => {
            const next = nextCodingSessionSurfaceTabIndex(
              event.key,
              activeIndex,
              surfaces.length,
            );
            if (next === null) return;
            event.preventDefault();
            const surface = surfaces[next];
            if (!surface) return;
            onSelectSurface(surface.id);
            tabRefs.current[next]?.focus();
          }}
          role="tablist"
        >
          {surfaces.map((surface, index) => (
            <button
              aria-controls={panelId(surface.id)}
              aria-selected={index === activeIndex}
              className={cn(
                "flex shrink-0 items-center gap-1.5 rounded-md px-2.5 py-1.5 text-xs font-medium text-muted-foreground outline-none transition-colors hover:text-foreground focus-visible:ring-2 focus-visible:ring-ring",
                index === activeIndex && "bg-muted text-foreground",
              )}
              data-testid={`coding-session-surface-tab-${surface.id}`}
              id={tabId(surface.id)}
              key={surface.id}
              onClick={() => onSelectSurface(surface.id)}
              ref={(node) => {
                tabRefs.current[index] = node;
              }}
              role="tab"
              tabIndex={index === activeIndex ? 0 : -1}
              type="button"
            >
              {surface.label}
              {typeof surface.count === "number" && surface.count > 0 ? (
                <span className="rounded-full bg-background/70 px-1.5 text-2xs tabular-nums">
                  {surface.count}
                </span>
              ) : null}
            </button>
          ))}
        </div>
        {showClose ? (
          <button
            aria-label="Close session surface"
            className="ml-2 flex size-8 shrink-0 items-center justify-center rounded-md text-muted-foreground hover:bg-muted hover:text-foreground"
            data-testid="coding-session-surface-close"
            onClick={onClose}
            type="button"
          >
            <PanelRightClose className="size-4" />
          </button>
        ) : null}
      </div>
      {activeSurface ? (
        <div
          aria-labelledby={tabId(activeSurface.id)}
          className="flex min-h-0 flex-1 flex-col"
          id={panelId(activeSurface.id)}
          role="tabpanel"
        >
          {activeSurface.content}
        </div>
      ) : null}
    </div>
  );
}
