import * as React from "react";
import { createPortal } from "react-dom";
import { ChevronDown, ChevronUp } from "lucide-react";

import { codingSessionParticipantAccent } from "@/features/coding-sessions/lib/codingSessionParticipantAccent";
import {
  advanceCodingSessionLastSeen,
  CODING_SESSION_LAST_SEEN_LABEL,
  readCodingSessionLastSeen,
  resolveCodingSessionLastSeenIndex,
  writeCodingSessionLastSeen,
  type CodingSessionLastSeenScope,
} from "@/features/coding-sessions/lib/codingSessionLastSeen";
import {
  CODING_SESSION_MINIMAP_MIN_ITEMS,
  codingSessionMinimapBoundsInView,
  codingSessionMinimapCurrentIndex,
  codingSessionMinimapHasPersistentGutter,
  codingSessionMinimapHeightStyle,
  codingSessionMinimapHitStripWidth,
  codingSessionMinimapIndexFromPointer,
  codingSessionMinimapInteractiveWidth,
  codingSessionMinimapNavigationInteractive,
  codingSessionMinimapPromptLine,
  codingSessionMinimapTopPercent,
  type CodingSessionMinimapItem,
} from "@/features/coding-sessions/lib/codingSessionTranscriptMinimapItems";
import type { CodingSessionMinimapMarks } from "@/features/coding-sessions/lib/codingSessionTranscriptMinimapMarks";
import { cn } from "@/shared/lib/cn";
import { truncatePubkey } from "@/shared/lib/pubkey";
import { CodingSessionTranscriptMinimapCard } from "./CodingSessionTranscriptMinimapCard";
import {
  type CodingSessionMinimapFacts,
  useCodingSessionTranscriptMinimapFacts,
} from "./CodingSessionTranscriptMinimapData";
import {
  type CodingSessionSurfaceCtx,
  useCodingSessionSurfaceCtx,
} from "./surfaces/codingSessionSurfaceContext";

/**
 * The transcript minimap (SV-26, SV-27): one dash per prompted turn at the
 * pane's left edge, portaled into B0's minimap slot
 * (`coding-session-minimap-slot`). A port of T3 Code's `TimelineMinimap`
 * (`MessagesTimeline.tsx:1462-1700`): the same 8 px spacing, the 48 px
 * persistent-gutter rule, the gutter-capped 40 px hit strip, the hover
 * fisheye, prev/next buttons and the keyboard (arrows, Home, End, Enter).
 *
 * Conversation lens only: in Mission the route rail is the map, and two
 * navigators on one edge would collide. Shown only from two items and only
 * on `pointer: fine`, as T3 does.
 *
 * Beekeeper differences, each recorded in the brief: dashes take the
 * prompter's identity accent when it is somebody else's prompt (DB12), turns
 * carry state marks (SV-27), the card says more than T3's, and a
 * "since you were here" rule marks where this device stopped reading.
 */
export function CodingSessionTranscriptMinimap({
  items,
  onSelect,
  resolveElement,
  scrollRef,
}: {
  items: readonly CodingSessionMinimapItem[];
  /** Bring the turn into view (scroll, or reveal then scroll). */
  onSelect: (item: CodingSessionMinimapItem) => void;
  /** The turn's rendered element, or null when it is not mounted. */
  resolveElement: (item: CodingSessionMinimapItem) => HTMLElement | null;
  scrollRef: React.RefObject<HTMLElement | null> | undefined;
}) {
  const ctx = useCodingSessionSurfaceCtx();
  const slot = useMinimapSlot(ctx);
  const enabled =
    ctx !== null &&
    ctx.lens === "conversation" &&
    items.length >= CODING_SESSION_MINIMAP_MIN_ITEMS;
  const facts = useCodingSessionTranscriptMinimapFacts(
    enabled ? ctx : null,
    items,
  );
  const viewport = useMinimapViewport({
    enabled,
    items,
    resolveElement,
    scrollRef,
  });
  const ruleIndex = useMinimapLastSeen(ctx, items, viewport.inView);
  if (!enabled || slot === null) return null;
  return createPortal(
    <CodingSessionMinimapRail
      currentIndex={viewport.currentIndex}
      facts={facts}
      hasPersistentGutter={viewport.hasPersistentGutter}
      hitStripWidth={viewport.hitStripWidth}
      inView={viewport.inView}
      items={items}
      onSelect={onSelect}
      resolveAuthorLabel={(item) => minimapAuthorLabel(ctx, item)}
      ruleIndex={ruleIndex}
    />,
    slot,
  );
}

function minimapAuthorLabel(
  ctx: CodingSessionSurfaceCtx | null,
  item: CodingSessionMinimapItem,
): string {
  switch (item.authorKind) {
    case "you":
      return "you";
    case "automatic":
      return "an automatic team wake";
    case "unrecorded":
      return "an operator the record does not name";
    case "other": {
      const pubkey = item.authorPubkey ?? "";
      return ctx?.resolveActorName(pubkey) ?? truncatePubkey(pubkey);
    }
  }
}

/** The slot element, once B0's workspace has mounted it. */
function useMinimapSlot(
  ctx: CodingSessionSurfaceCtx | null,
): HTMLDivElement | null {
  const slotRef = ctx?.minimapSlotRef ?? null;
  const [slot, setSlot] = React.useState<HTMLDivElement | null>(null);
  React.useLayoutEffect(() => {
    const next = slotRef?.current ?? null;
    setSlot((current) => (current === next ? current : next));
  });
  return slot;
}

type MinimapViewport = {
  inView: ReadonlySet<string>;
  currentIndex: number | null;
  hasPersistentGutter: boolean;
  hitStripWidth: number;
};

const EMPTY_IN_VIEW: ReadonlySet<string> = new Set();

/**
 * Which dashes are in view, the reader's current turn, and the gutter, kept
 * current on scroll (one measure per frame) and on resize — T3's
 * `handleScroll` and gutter `measure`, over this pane's own scroller. A turn
 * the virtualizer has not mounted is not in view.
 */
function useMinimapViewport(input: {
  enabled: boolean;
  items: readonly CodingSessionMinimapItem[];
  resolveElement: (item: CodingSessionMinimapItem) => HTMLElement | null;
  scrollRef: React.RefObject<HTMLElement | null> | undefined;
}): MinimapViewport {
  const { enabled, items, scrollRef } = input;
  const resolveRef = React.useRef(input.resolveElement);
  resolveRef.current = input.resolveElement;
  const [state, setState] = React.useState<MinimapViewport>({
    inView: EMPTY_IN_VIEW,
    currentIndex: null,
    hasPersistentGutter: false,
    hitStripWidth: 0,
  });
  React.useEffect(() => {
    const scroller = scrollRef?.current ?? null;
    if (!enabled || scroller === null) return;
    let frame = 0;
    const measure = () => {
      frame = 0;
      const scrollerRect = scroller.getBoundingClientRect();
      const scrollTop = scroller.scrollTop;
      const view = {
        scrollTop,
        scrollBottom: scrollTop + scroller.clientHeight,
      };
      const inView = new Set<string>();
      const itemBounds = items.map((item) => {
        const element = resolveRef.current(item);
        if (element === null) return { top: null, height: null };
        const rect = element.getBoundingClientRect();
        const bounds = {
          top: rect.top - scrollerRect.top + scrollTop,
          height: rect.height,
        };
        if (codingSessionMinimapBoundsInView(bounds, view)) inView.add(item.id);
        return bounds;
      });
      const currentIndex = codingSessionMinimapCurrentIndex({
        ...view,
        itemBounds,
      });
      const content = scroller.firstElementChild;
      const viewportWidth = scrollerRect.width;
      const contentWidth =
        content instanceof HTMLElement
          ? content.getBoundingClientRect().width
          : viewportWidth;
      const hasPersistentGutter = codingSessionMinimapHasPersistentGutter(
        viewportWidth,
        contentWidth,
      );
      const hitStripWidth = codingSessionMinimapHitStripWidth(
        viewportWidth,
        contentWidth,
      );
      setState((current) =>
        current.currentIndex === currentIndex &&
        current.hasPersistentGutter === hasPersistentGutter &&
        current.hitStripWidth === hitStripWidth &&
        sameSet(current.inView, inView)
          ? current
          : { inView, currentIndex, hasPersistentGutter, hitStripWidth },
      );
    };
    const schedule = () => {
      if (frame === 0) frame = requestAnimationFrame(measure);
    };
    schedule();
    scroller.addEventListener("scroll", schedule, { passive: true });
    const observer =
      typeof ResizeObserver === "undefined"
        ? null
        : new ResizeObserver(schedule);
    observer?.observe(scroller);
    if (scroller.firstElementChild) {
      observer?.observe(scroller.firstElementChild);
    }
    return () => {
      if (frame !== 0) cancelAnimationFrame(frame);
      scroller.removeEventListener("scroll", schedule);
      observer?.disconnect();
    };
  }, [enabled, items, scrollRef]);
  return state;
}

function sameSet(left: ReadonlySet<string>, right: ReadonlySet<string>) {
  if (left.size !== right.size) return false;
  for (const value of left) if (!right.has(value)) return false;
  return true;
}

/**
 * The "since you were here" rule's position (after which item), read once
 * per session view, and the device marker advanced as turns come into view.
 */
function useMinimapLastSeen(
  ctx: CodingSessionSurfaceCtx | null,
  items: readonly CodingSessionMinimapItem[],
  inView: ReadonlySet<string>,
): number | null {
  const relayUrl = ctx?.communityScope ?? null;
  const channelId = ctx?.channelId ?? null;
  const sessionKey = ctx?.sessionKey ?? null;
  const scope = React.useMemo<CodingSessionLastSeenScope | null>(
    () =>
      relayUrl && channelId && sessionKey
        ? { relayUrl, channelId, sessionKey }
        : null,
    [channelId, relayUrl, sessionKey],
  );
  // Read once per scope: the rule shows where this device stopped *before*
  // this visit, and stays put while this visit advances the marker.
  const snapshot = React.useMemo(
    () => (scope === null ? null : readCodingSessionLastSeen(scope)),
    [scope],
  );
  const itemIds = React.useMemo(() => items.map((item) => item.id), [items]);
  React.useEffect(() => {
    if (scope === null || inView.size === 0) return;
    const next = advanceCodingSessionLastSeen(
      itemIds,
      readCodingSessionLastSeen(scope),
      inView,
    );
    if (next !== null) writeCodingSessionLastSeen(scope, next);
  }, [inView, itemIds, scope]);
  return resolveCodingSessionLastSeenIndex(itemIds, snapshot);
}

/** DB12: your own prompt is neutral; anyone else's is their identity hue. */
function dashHue(item: CodingSessionMinimapItem): {
  base: string;
  bright: string;
} {
  if (item.authorKind === "other" && item.authorPubkey !== null) {
    const dot = codingSessionParticipantAccent(item.authorPubkey).dot;
    return { base: cn(dot, "opacity-50"), bright: dot };
  }
  return { base: "bg-muted-foreground/35", bright: "bg-foreground/90" };
}

function eventTargetsCard(target: EventTarget): boolean {
  return (
    target instanceof Element &&
    target.closest("[data-minimap-preview]") !== null
  );
}

function CodingSessionMinimapRail({
  currentIndex,
  facts,
  hasPersistentGutter,
  hitStripWidth,
  inView,
  items,
  onSelect,
  resolveAuthorLabel,
  ruleIndex,
}: {
  currentIndex: number | null;
  facts: CodingSessionMinimapFacts;
  hasPersistentGutter: boolean;
  hitStripWidth: number;
  inView: ReadonlySet<string>;
  items: readonly CodingSessionMinimapItem[];
  onSelect: (item: CodingSessionMinimapItem) => void;
  resolveAuthorLabel: (item: CodingSessionMinimapItem) => string;
  ruleIndex: number | null;
}) {
  const [activeIndex, setActiveIndex] = React.useState<number | null>(null);
  const active =
    activeIndex !== null && activeIndex < items.length ? activeIndex : null;
  const activeItem = active === null ? null : (items[active] ?? null);
  const current =
    currentIndex !== null && currentIndex >= 0 && currentIndex < items.length
      ? currentIndex
      : null;
  const previousItem = current === null ? null : (items[current - 1] ?? null);
  const nextItem = current === null ? null : (items[current + 1] ?? null);
  const navigationInteractive =
    codingSessionMinimapNavigationInteractive(hitStripWidth);
  const activeTop =
    active === null ? 0 : codingSessionMinimapTopPercent(active, items.length);
  const activeTranslate =
    active === null || (active > 0 && active < items.length - 1)
      ? "-50%"
      : active === 0
        ? "0%"
        : "-100%";

  const indexFromPointer = (event: React.MouseEvent<HTMLElement>) => {
    const rect = event.currentTarget.getBoundingClientRect();
    return codingSessionMinimapIndexFromPointer({
      itemCount: items.length,
      railTop: rect.top,
      railHeight: rect.height,
      pointerY: event.clientY,
    });
  };
  const move = (delta: number) =>
    setActiveIndex((value) =>
      Math.max(0, Math.min(items.length - 1, (value ?? 0) + delta)),
    );

  return (
    <div
      className={cn(
        "group/minimap absolute inset-y-0 left-0 hidden w-18 [@media(pointer:fine)]:block",
        hasPersistentGutter
          ? "opacity-100"
          : "opacity-0 transition-opacity duration-150 focus-within:opacity-100 hover:opacity-100",
      )}
      data-dash-count={items.length}
      data-persistent-gutter={hasPersistentGutter ? "true" : "false"}
      data-testid="coding-session-minimap"
      // The slot makes its children pointer-active; the rail itself must not
      // be, or it would swallow clicks on the transcript beside it.
      style={{ pointerEvents: "none" }}
    >
      <div className="relative h-full w-full select-none">
        <div
          className="absolute top-1/2 left-3 -translate-y-1/2"
          style={{
            height: codingSessionMinimapHeightStyle(items.length),
            pointerEvents: hitStripWidth > 0 ? "auto" : "none",
            width: codingSessionMinimapInteractiveWidth(
              hitStripWidth,
              activeItem !== null,
            ),
          }}
        >
          <MinimapNavigationButton
            direction="previous"
            disabled={previousItem === null}
            interactive={navigationInteractive}
            onClick={() => previousItem && onSelect(previousItem)}
          />
          <button
            aria-label={`Jump to turn: ${codingSessionMinimapPromptLine(activeItem?.userText) ?? "prompt"}`}
            className="absolute inset-y-0 left-0 w-full cursor-pointer bg-transparent focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring/70"
            data-testid="coding-session-minimap-strip"
            onBlur={() => setActiveIndex(null)}
            onClick={(event) => {
              if (eventTargetsCard(event.target)) return;
              const index = indexFromPointer(event);
              const selected = index === null ? null : (items[index] ?? null);
              if (selected) onSelect(selected);
              event.currentTarget.blur();
            }}
            onFocus={() => setActiveIndex((value) => value ?? current ?? 0)}
            onKeyDown={(event) => {
              if (event.key === "ArrowDown") {
                event.preventDefault();
                move(1);
              } else if (event.key === "ArrowUp") {
                event.preventDefault();
                move(-1);
              } else if (event.key === "Home") {
                event.preventDefault();
                setActiveIndex(0);
              } else if (event.key === "End") {
                event.preventDefault();
                setActiveIndex(items.length - 1);
              } else if (event.key === "Enter" || event.key === " ") {
                event.preventDefault();
                if (activeItem) onSelect(activeItem);
              }
            }}
            onMouseDown={(event) => {
              if (!eventTargetsCard(event.target)) event.preventDefault();
            }}
            onMouseLeave={() => setActiveIndex(null)}
            onMouseMove={(event) => setActiveIndex(indexFromPointer(event))}
            type="button"
          >
            <span className="absolute top-0 left-3 h-full w-px bg-border/15" />
            {items.map((item, index) => (
              <MinimapDash
                activeDistance={
                  active === null ? null : Math.abs(index - active)
                }
                index={index}
                inView={inView.has(item.id)}
                item={item}
                itemCount={items.length}
                key={item.id}
                marks={facts.marks.get(item.id) ?? null}
              />
            ))}
            {ruleIndex !== null ? (
              <span
                className="pointer-events-none absolute left-0 flex -translate-y-1/2 items-center gap-1"
                data-after-item={items[ruleIndex]?.id}
                data-testid="coding-session-minimap-since"
                style={{
                  top: `${codingSessionMinimapTopPercent(ruleIndex + 0.5, items.length)}%`,
                }}
                title={`Since you were here (${CODING_SESSION_LAST_SEEN_LABEL})`}
              >
                <span className="h-px w-7 bg-primary" />
                <span className="sr-only">New since you were here,</span>
                <span
                  className={cn(
                    "whitespace-nowrap text-3xs text-primary",
                    !hasPersistentGutter && "sr-only",
                  )}
                >
                  {CODING_SESSION_LAST_SEEN_LABEL}
                </span>
              </span>
            ) : null}
            {activeItem ? (
              // biome-ignore lint/a11y/noStaticElementInteractions: as in T3, moving over the open card must not re-aim the fisheye; it is not a control
              <span
                className="pointer-events-auto absolute left-8 w-80 cursor-text select-text"
                data-minimap-preview
                onMouseMove={(event) => event.stopPropagation()}
                style={{
                  top: `${activeTop}%`,
                  transform: `translateY(${activeTranslate})`,
                }}
              >
                <CodingSessionTranscriptMinimapCard
                  authorLabel={resolveAuthorLabel(activeItem)}
                  facts={facts}
                  firstSinceLastSeen={
                    ruleIndex !== null && active === ruleIndex + 1
                  }
                  item={activeItem}
                  marks={facts.marks.get(activeItem.id) ?? null}
                />
              </span>
            ) : null}
          </button>
          <MinimapNavigationButton
            direction="next"
            disabled={nextItem === null}
            interactive={navigationInteractive}
            onClick={() => nextItem && onSelect(nextItem)}
          />
        </div>
      </div>
    </div>
  );
}

/**
 * One dash. Like T3's, it animates only transform and opacity: width tiers
 * are a scale-x on a fixed box and the in-view highlight is an opacity-faded
 * bright layer, so scroll ticks never force layout.
 */
const MinimapDash = React.memo(function MinimapDash({
  activeDistance,
  index,
  inView,
  item,
  itemCount,
  marks,
}: {
  activeDistance: number | null;
  index: number;
  inView: boolean;
  item: CodingSessionMinimapItem;
  itemCount: number;
  marks: CodingSessionMinimapMarks | null;
}) {
  const hue = dashHue(item);
  const top = `${codingSessionMinimapTopPercent(index, itemCount)}%`;
  return (
    <>
      <span
        aria-hidden="true"
        className={cn(
          "group/strip pointer-events-none absolute left-0 h-0.5 w-6 origin-left -translate-y-1/2 rounded-full transition-transform duration-150",
          activeDistance === 0
            ? "scale-x-100"
            : activeDistance === 1
              ? "scale-x-[0.667]"
              : activeDistance === 2
                ? "scale-x-[0.417]"
                : "scale-x-[0.333]",
        )}
        data-author-kind={item.authorKind}
        data-in-view={inView ? "true" : "false"}
        data-item-id={item.id}
        data-testid="coding-session-minimap-dash"
        style={{ top }}
      >
        <span
          className={cn("absolute inset-0 rounded-full", hue.base)}
          data-testid="coding-session-minimap-dash-hue"
        />
        <span
          className={cn(
            "absolute inset-0 rounded-full opacity-0 transition-opacity duration-150 group-data-[in-view=true]/strip:opacity-100",
            hue.bright,
          )}
        />
      </span>
      <MinimapMarks itemId={item.id} marks={marks} top={top} />
    </>
  );
});

/** State marks beside a dash (SV-27): state hues and glyphs only (DB12). */
function MinimapMarks({
  itemId,
  marks,
  top,
}: {
  itemId: string;
  marks: CodingSessionMinimapMarks | null;
  top: string;
}) {
  if (marks === null) return null;
  const glyph = marks.gate?.glyph ?? null;
  if (
    !marks.failed &&
    glyph === null &&
    marks.handovers === 0 &&
    marks.waitingRulings === 0 &&
    !marks.rewound
  ) {
    return null;
  }
  return (
    <span
      aria-hidden="true"
      className="pointer-events-none absolute left-7 flex -translate-y-1/2 items-center gap-0.5 text-3xs leading-none"
      data-item-id={itemId}
      data-testid="coding-session-minimap-marks"
      style={{ top }}
    >
      {marks.rewound ? (
        <span
          className="text-muted-foreground"
          data-testid="coding-session-minimap-mark-rewind"
        >
          ↶
        </span>
      ) : null}
      {marks.failed ? (
        <span
          className="size-1 rounded-full bg-destructive"
          data-testid="coding-session-minimap-mark-failed"
        />
      ) : null}
      {marks.waitingRulings > 0 ? (
        <span
          className="size-1 rounded-full bg-amber-500"
          data-testid="coding-session-minimap-mark-ruling"
        />
      ) : null}
      {glyph !== null ? (
        <span
          className={cn(
            "font-semibold",
            glyph === "pass" ? "text-muted-foreground" : "text-foreground",
          )}
          data-glyph={glyph}
          data-testid="coding-session-minimap-mark-gate"
        >
          {glyph === "pass" ? "✓" : "✗"}
        </span>
      ) : null}
      {marks.handovers > 0 ? (
        <span
          className="text-muted-foreground"
          data-testid="coding-session-minimap-mark-handover"
        >
          ⇄
        </span>
      ) : null}
    </span>
  );
}

function MinimapNavigationButton({
  direction,
  disabled,
  interactive,
  onClick,
}: {
  direction: "previous" | "next";
  disabled: boolean;
  interactive: boolean;
  onClick: () => void;
}) {
  const previous = direction === "previous";
  const label = previous ? "Previous turn" : "Next turn";
  const Icon = previous ? ChevronUp : ChevronDown;
  return (
    <span
      className={cn(
        "absolute left-1 z-10 inline-flex -translate-x-1/2 opacity-0 transition-opacity duration-150 focus-within:opacity-100 hover:opacity-100",
        previous ? "bottom-[calc(100%+2px)]" : "top-[calc(100%+2px)]",
      )}
      style={{ pointerEvents: interactive ? "auto" : "none" }}
    >
      <button
        aria-label={label}
        className="inline-flex size-5 items-center justify-center rounded text-foreground/90 hover:bg-muted disabled:opacity-40"
        data-testid={`coding-session-minimap-${direction}`}
        disabled={disabled}
        onClick={onClick}
        title={label}
        type="button"
      >
        <Icon className="size-4" />
      </button>
    </span>
  );
}
