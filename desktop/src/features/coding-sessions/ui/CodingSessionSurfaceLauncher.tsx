import * as React from "react";

import { cn } from "@/shared/lib/cn";
import {
  Tooltip,
  TooltipContent,
  TooltipProvider,
  TooltipTrigger,
} from "@/shared/ui/tooltip";

import type { CodingSessionSurfaceCtx } from "./surfaces/codingSessionSurfaceContext";
import type {
  CodingSessionResolvedSurface,
  CodingSessionSurfaceBadgeSlot,
  CodingSessionSurfaceDefinition,
} from "./surfaces/codingSessionSurfaceRegistry";

/**
 * "Open a surface" (SV-21), after T3 Code's `RightPanelEmptyState`
 * (`components/RightPanelTabs.tsx:307-575`).
 *
 * Shown while the right panel is open with no active tab. One row per
 * surface the lens lists: icon (with its badge slot on the corner), label and
 * letter. A surface that cannot open stays in the list, dimmed and
 * `aria-disabled`, with its reason in a tooltip and in the row's accessible
 * description (SV-23): dimmed, never removed.
 *
 * Keys follow T3: a bare letter opens its surface from anywhere while the
 * launcher is visible (capture phase, so an app handler cannot swallow it),
 * except in a typing context or under an overlay; arrows move a highlight
 * and Enter opens it while the launcher has focus.
 */

/** Overlays that win over the launcher's letters (T3 `:163-173`). */
const LAUNCHER_BLOCKING_LAYERS =
  '[role="dialog"], [role="alertdialog"], [role="menu"], [role="listbox"]';

type ShortcutEvent = Pick<
  KeyboardEvent,
  "altKey" | "ctrlKey" | "defaultPrevented" | "isComposing" | "key" | "metaKey"
>;

/** The available surface a bare letter opens, or null (T3 `:246-257`). */
export function codingSessionSurfaceForShortcut(
  surfaces: readonly CodingSessionResolvedSurface[],
  event: ShortcutEvent,
): CodingSessionSurfaceDefinition | null {
  if (event.defaultPrevented || event.isComposing) return null;
  if (event.metaKey || event.ctrlKey || event.altKey) return null;
  if (event.key.length !== 1) return null;
  const letter = event.key.toUpperCase();
  return (
    surfaces.find(
      (surface) =>
        surface.availability.available &&
        surface.definition.shortcut === letter,
    )?.definition ?? null
  );
}

/**
 * A focused editable is a typing context whether or not it holds text: an
 * empty composer is still where the next keystroke belongs (T3 `:259-273`).
 */
export function codingSessionSurfaceTargetIsTyping(
  target: { closest(selectors: string): unknown } | null,
): boolean {
  return (
    target?.closest(
      'input, textarea, select, [contenteditable]:not([contenteditable="false"])',
    ) != null
  );
}

/** Whether an overlay other than the one holding `launcher` is open. */
function overlayBlocks(launcher: HTMLElement | null): boolean {
  for (const layer of document.querySelectorAll(LAUNCHER_BLOCKING_LAYERS)) {
    if (launcher === null || !layer.contains(launcher)) return true;
  }
  return false;
}

/** An icon with the surface's badge slot on its top-right corner (ref sv22). */
export function CodingSessionSurfaceIconWithBadge({
  ctx,
  definition,
  slot,
}: {
  ctx: CodingSessionSurfaceCtx;
  definition: CodingSessionSurfaceDefinition;
  slot: CodingSessionSurfaceBadgeSlot;
}) {
  const Icon = definition.icon;
  const Badge = definition.Badge;
  return (
    <span className="relative inline-flex shrink-0">
      <Icon aria-hidden className={slot === "tab" ? "size-3.5" : "size-4"} />
      {Badge ? (
        <span
          className="pointer-events-none absolute -top-1.5 -right-2 flex"
          data-testid={`coding-session-surface-badge-slot-${definition.id}`}
        >
          <Badge ctx={ctx} slot={slot} />
        </span>
      ) : null}
    </span>
  );
}

function LauncherKbd({ letter }: { letter: string }) {
  return (
    <kbd className="inline-flex h-5 min-w-5 items-center justify-center rounded border border-border/70 bg-muted/40 px-1 font-sans text-2xs font-medium text-muted-foreground">
      {letter}
    </kbd>
  );
}

export function CodingSessionSurfaceLauncher({
  ctx,
  onOpen,
  surfaces,
}: {
  ctx: CodingSessionSurfaceCtx;
  onOpen: (id: string) => void;
  /** The lens's surfaces, in launcher order, with availability resolved. */
  surfaces: readonly CodingSessionResolvedSurface[];
}) {
  const [highlight, setHighlight] = React.useState(-1);
  const rootRef = React.useRef<HTMLElement | null>(null);
  const idBase = React.useId();
  const available = surfaces.filter(
    (surface) => surface.availability.available,
  );
  const highlightIndex =
    available.length === 0 ? -1 : Math.min(highlight, available.length - 1);

  const surfacesRef = React.useRef(surfaces);
  surfacesRef.current = surfaces;
  const onOpenRef = React.useRef(onOpen);
  onOpenRef.current = onOpen;
  React.useEffect(() => {
    const handler = (event: KeyboardEvent) => {
      const definition = codingSessionSurfaceForShortcut(
        surfacesRef.current,
        event,
      );
      if (!definition) return;
      if (overlayBlocks(rootRef.current)) return;
      const target = event.target;
      if (
        target instanceof Element &&
        codingSessionSurfaceTargetIsTyping(target)
      )
        return;
      event.preventDefault();
      event.stopPropagation();
      onOpenRef.current(definition.id);
    };
    window.addEventListener("keydown", handler, true);
    return () => window.removeEventListener("keydown", handler, true);
  }, []);

  const focusOnMount = React.useCallback((node: HTMLElement | null) => {
    rootRef.current = node;
    node?.focus({ preventScroll: true });
  }, []);

  const handleKeyDown = (event: React.KeyboardEvent<HTMLElement>) => {
    if (event.defaultPrevented || event.metaKey || event.ctrlKey) return;
    if (event.altKey || available.length === 0) return;
    if (event.key === "ArrowDown" || event.key === "ArrowRight") {
      event.preventDefault();
      setHighlight((highlightIndex + 1) % available.length);
    } else if (event.key === "ArrowUp" || event.key === "ArrowLeft") {
      event.preventDefault();
      setHighlight(
        highlightIndex === -1
          ? available.length - 1
          : (highlightIndex - 1 + available.length) % available.length,
      );
    } else if (event.key === "Enter" && event.target === event.currentTarget) {
      const surface = available[highlightIndex];
      if (!surface) return;
      event.preventDefault();
      onOpen(surface.definition.id);
    }
  };

  return (
    <TooltipProvider delayDuration={150}>
      <section
        aria-label="Open a surface"
        className="flex min-h-0 flex-1 items-center justify-center overflow-y-auto px-6 outline-none"
        data-surface-launcher-keys={available
          .map((surface) => surface.definition.shortcut)
          .join("")}
        data-testid="coding-session-surface-launcher"
        onKeyDown={handleKeyDown}
        ref={focusOnMount}
        // biome-ignore lint/a11y/noNoninteractiveTabindex: it takes focus on mount so arrows and Enter reach it (T3 `RightPanelTabs.tsx:438-447`).
        tabIndex={0}
      >
        <div className="w-full max-w-xs py-6">
          <h3 className="mb-3 text-center text-sm font-medium text-foreground">
            Open a surface
          </h3>
          <div className="flex flex-col gap-0.5">
            {surfaces.map(({ availability, definition }) => {
              const rowTestId = `coding-session-surface-launcher-row-${definition.id}`;
              const content = (
                <>
                  <CodingSessionSurfaceIconWithBadge
                    ctx={ctx}
                    definition={definition}
                    slot="launcher"
                  />
                  <span className="min-w-0 flex-1 truncate">
                    {definition.label}
                  </span>
                  <LauncherKbd letter={definition.shortcut} />
                </>
              );
              if (availability.available) {
                const index = available.findIndex(
                  (surface) => surface.definition.id === definition.id,
                );
                return (
                  <button
                    aria-keyshortcuts={definition.shortcut}
                    className={cn(
                      "flex h-8 w-full cursor-pointer items-center gap-2.5 rounded-md px-2.5 text-left text-sm text-foreground transition-colors hover:bg-accent/60",
                      index === highlightIndex && "bg-accent/60",
                    )}
                    data-available="true"
                    data-testid={rowTestId}
                    key={definition.id}
                    onClick={() => onOpen(definition.id)}
                    onMouseEnter={() => setHighlight(index)}
                    onMouseLeave={() =>
                      setHighlight((current) =>
                        current === index ? -1 : current,
                      )
                    }
                    type="button"
                  >
                    {content}
                  </button>
                );
              }
              const reasonId = `${idBase}-reason-${definition.id}`;
              return (
                <Tooltip key={definition.id}>
                  <TooltipTrigger asChild>
                    {/* Focusable, so the reason is reachable by keyboard;
                        `aria-disabled`, not `disabled`, so it stays so. */}
                    <button
                      aria-describedby={reasonId}
                      aria-disabled="true"
                      aria-keyshortcuts={definition.shortcut}
                      className="flex h-8 w-full cursor-default items-center gap-2.5 rounded-md px-2.5 text-left text-sm text-foreground opacity-50 outline-none focus-visible:ring-2 focus-visible:ring-ring"
                      data-available="false"
                      data-testid={rowTestId}
                      type="button"
                    >
                      {content}
                      <span className="sr-only" id={reasonId}>
                        {availability.reason}
                      </span>
                    </button>
                  </TooltipTrigger>
                  <TooltipContent
                    data-testid={`coding-session-surface-reason-tooltip-${definition.id}`}
                    side="top"
                  >
                    {availability.reason}
                  </TooltipContent>
                </Tooltip>
              );
            })}
          </div>
        </div>
      </section>
    </TooltipProvider>
  );
}
