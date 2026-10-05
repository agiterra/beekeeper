import * as React from "react";
import { Maximize2, Minimize2, PanelRightClose, Plus, X } from "lucide-react";

import { cn } from "@/shared/lib/cn";
import {
  ContextMenu,
  ContextMenuContent,
  ContextMenuItem,
  ContextMenuSeparator,
  ContextMenuTrigger,
} from "@/shared/ui/context-menu";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuShortcut,
  DropdownMenuTrigger,
} from "@/shared/ui/dropdown-menu";

import {
  CodingSessionSurfaceIconWithBadge,
  codingSessionSurfaceForShortcut,
} from "./CodingSessionSurfaceLauncher";
import type { CodingSessionSurfaceCtx } from "./surfaces/codingSessionSurfaceContext";
import type {
  CodingSessionResolvedSurface,
  CodingSessionSurfaceDefinition,
} from "./surfaces/codingSessionSurfaceRegistry";

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

/** What a tab's context menu can close, after T3's tab menu. */
export type CodingSessionSurfaceTabCloseScope =
  | "close"
  | "close-others"
  | "close-right"
  | "close-all";

/**
 * The tab ids a context-menu action closes, from the strip's tabs in order.
 * Empty when the action would close nothing (the item is then disabled).
 */
export function codingSessionSurfaceTabsToClose(
  tabIds: readonly string[],
  id: string,
  scope: CodingSessionSurfaceTabCloseScope,
): string[] {
  const index = tabIds.indexOf(id);
  if (index === -1) return [];
  switch (scope) {
    case "close":
      return [id];
    case "close-others":
      return tabIds.filter((tabId) => tabId !== id);
    case "close-right":
      return tabIds.slice(index + 1);
    case "close-all":
      return [...tabIds];
  }
}

const TAB_CONTEXT_ITEMS: ReadonlyArray<{
  scope: CodingSessionSurfaceTabCloseScope;
  label: string;
}> = [
  { scope: "close", label: "Close" },
  { scope: "close-others", label: "Close others" },
  { scope: "close-right", label: "Close to the right" },
  { scope: "close-all", label: "Close all" },
];

/**
 * The right panel's top bar (SV-21), after T3 Code's tab bar
 * (`components/RightPanelTabs.tsx:1088-1250`): one tab per opened surface
 * whose icon slot turns into its close button on hover or focus (T3's
 * `PanelTabCloseButton`), middle-click to close, a context menu that closes
 * this, the others, those to the right or all; a "+" menu listing the same
 * entries as the launcher (dimmed rows keep their reason inline), then expand
 * and close at the top right.
 */
export function CodingSessionSurfaceTabStrip({
  activeId,
  ctx,
  expanded,
  idBase,
  onActivate,
  onClosePanel,
  onCloseTab,
  onCloseTabs,
  onOpen,
  onToggleExpanded,
  showPanelControls,
  surfaces,
  tabs,
}: {
  activeId: string | null;
  ctx: CodingSessionSurfaceCtx;
  expanded: boolean;
  /** Instance-scoped prefix for tab/panel DOM ids. */
  idBase: string;
  onActivate: (id: string) => void;
  onClosePanel: () => void;
  onCloseTab: (id: string) => void;
  /** Closes several tabs at once (the context menu's bulk actions). */
  onCloseTabs: (ids: readonly string[]) => void;
  onOpen: (id: string) => void;
  onToggleExpanded: () => void;
  /** False in the sheet, which renders its own close control. */
  showPanelControls: boolean;
  /** Every surface the lens lists, for the "+" menu. */
  surfaces: readonly CodingSessionResolvedSurface[];
  /** The opened tabs, in order. */
  tabs: readonly CodingSessionSurfaceDefinition[];
}) {
  const tabRefs = React.useRef<Array<HTMLButtonElement | null>>([]);
  const [menuOpen, setMenuOpen] = React.useState(false);
  const activeIndex = tabs.findIndex((tab) => tab.id === activeId);
  const tabIds = tabs.map((tab) => tab.id);

  return (
    <div
      className={cn(
        "flex h-12 shrink-0 items-center gap-1 border-b border-border/60 pl-2",
        showPanelControls ? "pr-2" : "pr-12",
      )}
      data-testid="coding-session-surface-tabbar"
    >
      <div
        aria-label="Session surface tabs"
        className="flex min-w-0 flex-1 items-center gap-1 overflow-x-auto [scrollbar-width:none] [&::-webkit-scrollbar]:hidden"
        onKeyDown={(event) => {
          const next = nextCodingSessionSurfaceTabIndex(
            event.key,
            activeIndex,
            tabs.length,
          );
          if (next === null) return;
          const tab = tabs[next];
          if (!tab) return;
          event.preventDefault();
          onActivate(tab.id);
          tabRefs.current[next]?.focus();
        }}
        role="tablist"
      >
        {tabs.map((tab, index) => {
          const active = index === activeIndex;
          return (
            <ContextMenu key={tab.id}>
              <ContextMenuTrigger asChild>
                <div
                  className={cn(
                    "group/tab flex h-7 max-w-40 shrink-0 items-center gap-0.5 rounded-md pr-1 pl-1 text-xs",
                    active
                      ? "bg-muted text-foreground"
                      : "text-muted-foreground hover:bg-muted/60 hover:text-foreground",
                  )}
                  data-testid={`coding-session-surface-tab-item-${tab.id}`}
                  onAuxClick={(event) => {
                    if (event.button !== 1) return;
                    event.preventDefault();
                    onCloseTab(tab.id);
                  }}
                >
                  <button
                    aria-label={`Close ${tab.label}`}
                    className="group/close relative flex size-5 shrink-0 items-center justify-center rounded-sm outline-none hover:bg-background/80 focus-visible:ring-2 focus-visible:ring-ring"
                    data-testid={`coding-session-surface-tab-close-${tab.id}`}
                    onClick={() => onCloseTab(tab.id)}
                    type="button"
                  >
                    <span className="relative flex items-center justify-center group-hover/tab:hidden group-focus-visible/close:hidden">
                      <CodingSessionSurfaceIconWithBadge
                        ctx={ctx}
                        definition={tab}
                        slot="tab"
                      />
                    </span>
                    <X
                      aria-hidden
                      className="hidden size-3 group-hover/tab:block group-focus-visible/close:block"
                    />
                  </button>
                  <button
                    aria-controls={`${idBase}-panel-${tab.id}`}
                    aria-selected={active}
                    className="flex min-w-0 items-center rounded px-1 py-1 font-medium outline-none focus-visible:ring-2 focus-visible:ring-ring"
                    data-testid={`coding-session-surface-tab-${tab.id}`}
                    id={`${idBase}-tab-${tab.id}`}
                    onClick={() => onActivate(tab.id)}
                    ref={(node) => {
                      tabRefs.current[index] = node;
                    }}
                    role="tab"
                    tabIndex={
                      active || (activeIndex === -1 && index === 0) ? 0 : -1
                    }
                    type="button"
                  >
                    <span className="truncate">{tab.label}</span>
                  </button>
                </div>
              </ContextMenuTrigger>
              <ContextMenuContent
                data-testid={`coding-session-surface-tab-menu-${tab.id}`}
              >
                {TAB_CONTEXT_ITEMS.map(({ label, scope }) => {
                  const ids = codingSessionSurfaceTabsToClose(
                    tabIds,
                    tab.id,
                    scope,
                  );
                  return (
                    <React.Fragment key={scope}>
                      {scope === "close-all" ? <ContextMenuSeparator /> : null}
                      <ContextMenuItem
                        data-testid={`coding-session-surface-tab-menu-${scope}`}
                        disabled={ids.length === 0}
                        onSelect={() => onCloseTabs(ids)}
                      >
                        {label}
                      </ContextMenuItem>
                    </React.Fragment>
                  );
                })}
              </ContextMenuContent>
            </ContextMenu>
          );
        })}
        {tabs.length > 0 ? (
          <DropdownMenu onOpenChange={setMenuOpen} open={menuOpen}>
            <DropdownMenuTrigger asChild>
              <button
                aria-label="Add a surface"
                className="flex size-7 shrink-0 items-center justify-center rounded-md text-muted-foreground hover:bg-muted hover:text-foreground"
                data-testid="coding-session-surface-add"
                type="button"
              >
                <Plus aria-hidden className="size-3.5" />
              </button>
            </DropdownMenuTrigger>
            <DropdownMenuContent
              align="start"
              className="min-w-52"
              data-testid="coding-session-surface-add-menu"
              onKeyDownCapture={(event) => {
                const definition = codingSessionSurfaceForShortcut(
                  surfaces,
                  event.nativeEvent,
                );
                if (!definition) return;
                event.preventDefault();
                event.stopPropagation();
                setMenuOpen(false);
                onOpen(definition.id);
              }}
            >
              {surfaces.map(({ availability, definition }) => {
                const Icon = definition.icon;
                return (
                  <DropdownMenuItem
                    aria-keyshortcuts={definition.shortcut}
                    data-testid={`coding-session-surface-add-${definition.id}`}
                    disabled={!availability.available}
                    key={definition.id}
                    onSelect={() => onOpen(definition.id)}
                  >
                    <Icon aria-hidden className="size-4" />
                    <span className="flex min-w-0 flex-col">
                      <span>{definition.label}</span>
                      {availability.available ? null : (
                        <span className="text-2xs text-muted-foreground">
                          {availability.reason}
                        </span>
                      )}
                    </span>
                    <DropdownMenuShortcut>
                      {definition.shortcut}
                    </DropdownMenuShortcut>
                  </DropdownMenuItem>
                );
              })}
            </DropdownMenuContent>
          </DropdownMenu>
        ) : null}
      </div>
      {showPanelControls ? (
        <>
          <button
            aria-label={expanded ? "Restore panel size" : "Expand panel"}
            aria-pressed={expanded}
            className="flex size-8 shrink-0 items-center justify-center rounded-md text-muted-foreground hover:bg-muted hover:text-foreground"
            data-testid="coding-session-surface-expand"
            onClick={onToggleExpanded}
            title={expanded ? "Restore panel size" : "Expand panel"}
            type="button"
          >
            {expanded ? (
              <Minimize2 aria-hidden className="size-4" />
            ) : (
              <Maximize2 aria-hidden className="size-4" />
            )}
          </button>
          <button
            aria-label="Close session surface"
            className="flex size-8 shrink-0 items-center justify-center rounded-md text-muted-foreground hover:bg-muted hover:text-foreground"
            data-testid="coding-session-surface-close"
            onClick={onClosePanel}
            type="button"
          >
            <PanelRightClose aria-hidden className="size-4" />
          </button>
        </>
      ) : null}
    </div>
  );
}
