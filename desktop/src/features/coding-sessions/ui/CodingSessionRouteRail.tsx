import * as React from "react";
import {
  Check,
  CheckCheck,
  CircleCheckBig,
  CircleQuestionMark,
  Clock3,
  ClipboardList,
  FileText,
  Flag,
  Gavel,
  GitBranch,
  PanelLeftClose,
  LifeBuoy,
  OctagonAlert,
  Scale,
  Signpost,
  StickyNote,
  TriangleAlert,
  type LucideIcon,
} from "lucide-react";

import type {
  CodingSessionRoute,
  CodingSessionRouteRoad,
  CodingSessionRouteSign,
  CodingSessionRouteSignKind,
} from "@/features/coding-sessions/lib/codingSessionRouteModel";
import { codingSessionOpenHoldStatusLine } from "@/features/coding-sessions/lib/codingSessionMissionOpenHolds";
import { cn } from "@/shared/lib/cn";

import {
  CODING_SESSION_ROUTE_DEFAULT_WIDTH_PX,
  CODING_SESSION_ROUTE_MAX_WIDTH_PX,
  CODING_SESSION_ROUTE_MIN_WIDTH_PX,
} from "./useCodingSessionRouteWidth";

/**
 * The dom id the header's Route control points at (F7).
 *
 * Carried by the expanded rail **and** by the scrubber, because the control
 * governs whichever of the two is mounted and an `aria-controls` that resolves
 * to nothing is a promise to a screen reader that the app does not keep.
 */
export const CODING_SESSION_ROUTE_RAIL_ID = "coding-session-route-rail";

/** Left padding before the first lane. */
const MAP_PAD_X = 16;
/** The connector run between the last drawn lane and the first sign glyph. */
const LANE_TAIL_PX = 10;

/**
 * Where the sign column starts, measured from the map's left edge.
 *
 * `MAP_PAD_X` plus exactly the lanes that are drawn — not a constant. The
 * fixed 84 px this replaced reserved room for seven roads whether seven or
 * two were on the map, so a two-seat session paid 36 px of empty gutter out
 * of a 224 px rail and its signs truncated to pay for it (critique B5).
 *
 * REVIEW-L4 F14: the first version of this still over-reserved by one whole
 * lane. Lanes are centred at `MAP_PAD_X + lane * pitch` for lanes
 * `0 … roads.length - 1`, so the rightmost ink is at `(roads.length - 1) *
 * pitch`, not `roads.length * pitch`; charging the extra pitch gave the signs
 * one lane less than the map actually uses. What the column needs after the
 * last lane is the dotted connector, and that is {@link LANE_TAIL_PX}.
 */
export function codingSessionRouteSignColumnX(
  route: CodingSessionRoute,
): number {
  const drawn = Math.max(1, route.roads.length);
  return MAP_PAD_X + (drawn - 1) * route.lanePitchPx + LANE_TAIL_PX;
}
/** Breathing room under the newest sign so Now never sits on top of one. */
const MAP_TAIL_PX = 16;

/**
 * One glyph per sign kind. A glyph means one thing on this surface, always —
 * which is why `decision.answer` gets its own `CheckCheck` rather than reusing
 * the gavel a disposition already owns, or the single check an acknowledgement
 * does. Exported so a test can hold it against the wire's own type list.
 */
export const CODING_SESSION_ROUTE_SIGN_ICON: Readonly<
  Record<CodingSessionRouteSignKind, LucideIcon>
> = {
  assignment: ClipboardList,
  report: FileText,
  refutation: TriangleAlert,
  disposition: Gavel,
  acknowledgement: Check,
  "mission.completed": CircleCheckBig,
  "mission.blocked": OctagonAlert,
  note: StickyNote,
  "decision.request": CircleQuestionMark,
  "decision.answer": CheckCheck,
  hire: GitBranch,
  delivery: Clock3,
  "seat-ungranted": Flag,
};

/**
 * The glyph for a sign whose kind this build has never heard of.
 *
 * A relay one version ahead can sign a verb this table has no entry for, and
 * an unlisted entry used to resolve to `undefined` — which React renders by
 * throwing `Element type is invalid`, taking the whole Mission tab down over a
 * single unknown sign. A generic signpost says "something happened here that
 * this build cannot name", which is the honest reading and costs nothing else.
 */
const UNKNOWN_SIGN_ICON: LucideIcon = Signpost;

/** Delivery badges that are not the plain queued clock. */
const DELIVERY_ICON: Readonly<Record<string, LucideIcon>> = {
  fallback: LifeBuoy,
  failed: OctagonAlert,
};

function signIcon(sign: CodingSessionRouteSign): LucideIcon {
  if (sign.kind === "delivery") {
    return DELIVERY_ICON[sign.word] ?? Clock3;
  }
  return CODING_SESSION_ROUTE_SIGN_ICON[sign.kind] ?? UNKNOWN_SIGN_ICON;
}

/**
 * The Route rail — DESIGN-SPEC §9.
 *
 * A map of the session in the left gutter: a road per participant, a junction
 * where a seat was hired, a bridge for every signed handoff, a sign for every
 * row the stream already renders, and dashed stretches where nothing happened
 * or a wake sat queued. It is a **second projection**, never a second source:
 * every word on it is the word the stream's own row uses, and clicking a sign
 * reveals that row rather than restating it.
 *
 * Mission-only. Conversation never mounts this, which is what keeps the
 * one-seat lens byte-identical.
 */
export function CodingSessionRouteRail({
  className,
  onCollapse,
  onExpandRoad,
  onFocusRoad,
  onResizeKeyDown,
  onResizeStart,
  onRevealRow,
  route,
  variant = "gutter",
  widthPx = null,
}: {
  className?: string;
  /** Fold the rail to the 40 px scrubber. Absent in the sheet variant. */
  onCollapse?: () => void;
  /** Arrow-key resize, from `useCodingSessionRouteWidth`. */
  onResizeKeyDown?: (event: React.KeyboardEvent<HTMLButtonElement>) => void;
  /** Pointer drag, from `useCodingSessionRouteWidth`. */
  onResizeStart?: (event: React.PointerEvent<HTMLButtonElement>) => void;
  /**
   * The viewer's width for this rail, in px. `null` keeps the historical
   * fixed column — the sheet variant, and any caller that has not adopted the
   * handle yet.
   */
  widthPx?: number | null;
  /** Clicking a lane focuses that participant — the existing chip focus. */
  onFocusRoad?: (executionKey: string | null) => void;
  /** Clicking a sign reveals its stream row: scroll plus the 2.4 s ring. */
  onRevealRow?: (rowKey: string) => void;
  /** Lift one road's `+N earlier` bound, in place (§9.5). */
  onExpandRoad?: (executionKey: string) => void;
  route: CodingSessionRoute;
  /** `gutter` is the 224 px column; `sheet` is the scrubber's tap-through. */
  variant?: "gutter" | "sheet";
}) {
  const mapHeightPx = route.heightPx + MAP_TAIL_PX;
  const scrollRef = React.useRef<HTMLDivElement>(null);
  const signRefs = React.useRef(new Map<string, HTMLButtonElement>());
  const [focusedSignKey, setFocusedSignKey] = React.useState<string | null>(
    null,
  );
  // The sign whose row the rail last revealed. This is what `aria-pressed`
  // reports; a hard-coded `false` on every sign is a toggle that cannot
  // toggle, which is worse than no `aria-pressed` at all (REVIEW-A4 F6).
  const [selectedSignKey, setSelectedSignKey] = React.useState<string | null>(
    null,
  );
  // §9.6: "Tab enters the rail as one stop". Every focusable thing inside the
  // rail — lanes, signs, road heads, the `+N earlier` markers — shares one
  // roving tabindex, so the rail is a single stop in the page's tab order
  // instead of the seven it was (REVIEW-A4 F6).
  const [activeStop, setActiveStop] = React.useState<string | null>(null);
  // The map is bottom-anchored: Now is the rule at the foot of it, so a rail
  // taller than its column opens showing the newest end of the session rather
  // than the oldest. No timer and no animation — one scroll on mount.
  React.useEffect(() => {
    const node = scrollRef.current;
    if (node !== null) node.scrollTop = node.scrollHeight;
  }, []);

  const orderedSignKeys = React.useMemo(
    () => route.signs.map((sign) => sign.key),
    [route.signs],
  );
  const focusSignAt = React.useCallback(
    (index: number) => {
      const key = orderedSignKeys[index];
      if (key === undefined) return;
      setFocusedSignKey(key);
      setActiveStop(key);
      setSelectedSignKey(key);
      const node = signRefs.current.get(key);
      node?.focus();
      const sign = route.signs[index];
      if (sign?.revealKey != null) onRevealRow?.(sign.revealKey);
    },
    [orderedSignKeys, onRevealRow, route.signs],
  );
  // §9.6 — the keyboard map binds on the rail's own element, never globally,
  // so `j` in the composer is still a `j`.
  const onKeyDown = React.useCallback(
    (event: React.KeyboardEvent<HTMLElement>) => {
      const current =
        focusedSignKey === null ? -1 : orderedSignKeys.indexOf(focusedSignKey);
      if (event.key === "j" || event.key === "ArrowDown") {
        event.preventDefault();
        focusSignAt(Math.min(orderedSignKeys.length - 1, current + 1));
        return;
      }
      if (event.key === "k" || event.key === "ArrowUp") {
        event.preventDefault();
        focusSignAt(current <= 0 ? 0 : current - 1);
        return;
      }
      if (event.key === "Escape") {
        setFocusedSignKey(null);
        setSelectedSignKey(null);
      }
    },
    [focusSignAt, focusedSignKey, orderedSignKeys],
  );
  const rovingKey =
    activeStop ??
    orderedSignKeys[0] ??
    (route.roads[0] ? `road:${route.roads[0].key}` : null);
  const stopProps = (stopId: string) => ({
    onFocus: () => setActiveStop(stopId),
    tabIndex: stopId === rovingKey ? 0 : -1,
  });
  // A sign additionally becomes the cursor `j`/`k` move from, so focusing one
  // by any route — Tab, a click, a screen reader — leaves the keyboard walk
  // starting where the reader actually is.
  const signStopProps = (signKey: string) => ({
    ...stopProps(signKey),
    onFocus: () => {
      setActiveStop(signKey);
      setFocusedSignKey(signKey);
    },
  });
  // F15: two wakes on one lane put their duration labels within a few pixels
  // of each other at `text-2xs`. Labels are laid out top-down with a minimum
  // gap so a second measurement is never illegible next to the first.
  const stretchLabels = layOutRouteStretchLabels(route);

  const resizable = variant === "gutter" && onResizeStart !== undefined;
  const signColumnX = codingSessionRouteSignColumnX(route);
  return (
    <nav
      aria-label="Route"
      className={cn(
        "relative flex min-h-0 shrink-0 flex-col bg-background",
        variant === "gutter" &&
          cn("border-r border-border/50", widthPx === null && "w-56"),
        variant === "sheet" && "w-full",
        className,
      )}
      data-testid="coding-session-route-rail"
      // The gutter rail only: the scrubber's own sheet mounts this component
      // again, and two nodes sharing one id is a different broken IDREF.
      id={variant === "gutter" ? CODING_SESSION_ROUTE_RAIL_ID : undefined}
      data-variant={variant}
      onKeyDown={onKeyDown}
      style={
        variant === "gutter" && widthPx !== null
          ? { width: `${widthPx}px` }
          : undefined
      }
    >
      {resizable ? (
        // B1: the Inspector has had a real separator since §19; this rail had
        // a hard `w-56` and a width gate the viewer could not influence. Same
        // idiom, same value semantics, mirrored because this rail is on the
        // left: ArrowRight widens it.
        // biome-ignore lint/a11y/useSemanticElements: this separator is interactive (pointer drag + arrow-key resizing with value semantics); an <hr> cannot take focus or carry aria-valuenow.
        <button
          aria-label="Resize route rail"
          aria-orientation="vertical"
          aria-valuemax={CODING_SESSION_ROUTE_MAX_WIDTH_PX}
          aria-valuemin={CODING_SESSION_ROUTE_MIN_WIDTH_PX}
          aria-valuenow={Math.round(
            widthPx ?? CODING_SESSION_ROUTE_DEFAULT_WIDTH_PX,
          )}
          className="group absolute inset-y-0 -right-1 z-30 w-2 cursor-col-resize touch-none outline-none"
          data-testid="coding-session-route-resize"
          onKeyDown={onResizeKeyDown}
          onPointerDown={onResizeStart}
          role="separator"
          type="button"
        >
          <span className="absolute inset-y-0 left-1/2 w-px bg-transparent transition-colors group-hover:bg-primary/70 group-focus-visible:bg-primary" />
        </button>
      ) : null}
      {onCollapse ? (
        <div className="flex h-7 shrink-0 items-center justify-end pr-1">
          <button
            aria-expanded
            aria-label="Collapse route rail"
            className="flex size-6 items-center justify-center rounded-md text-muted-foreground hover:bg-muted hover:text-foreground focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring"
            data-testid="coding-session-route-collapse"
            onClick={onCollapse}
            title="Collapse the route rail to its scrubber"
            type="button"
          >
            <PanelLeftClose aria-hidden className="size-4" />
          </button>
        </div>
      ) : null}
      {/* §9.2 still governs a map *taller* than the rail: the mount effect
          above scrolls the scroller to its foot, so a long session opens at
          its newest end. A map *shorter* than the rail no longer hangs from
          the bottom — B5 measured ~350 px of blank sitting above the first
          sign, which reads as a rail that lost its top. The blank belongs
          under the last sign, where there is nothing to look for. */}
      <div
        className="flex min-h-0 flex-1 flex-col justify-start overflow-y-auto overscroll-contain pl-4"
        ref={scrollRef}
      >
        <div
          className="relative w-full shrink-0"
          style={{ height: `${mapHeightPx}px` }}
        >
          <RouteMap
            route={route}
            onFocusRoad={onFocusRoad}
            stopProps={stopProps}
          />
          <RouteBand route={route} />
          {route.signs.map((sign, index) => (
            <RouteSignButton
              index={index}
              key={sign.key}
              onReveal={(rowKey) => {
                setFocusedSignKey(sign.key);
                setSelectedSignKey(sign.key);
                onRevealRow?.(rowKey);
              }}
              registerRef={(node) => {
                if (node) signRefs.current.set(sign.key, node);
                else signRefs.current.delete(sign.key);
              }}
              roadAccentText={
                route.roads.find((road) => road.key === sign.road)?.accent
                  .text ?? "text-muted-foreground"
              }
              selected={sign.key === selectedSignKey}
              sign={sign}
              signColumnX={signColumnX}
              stopProps={signStopProps(sign.key)}
            />
          ))}
          {stretchLabels.map((stretch) => (
            <span
              className="absolute text-2xs tabular-nums text-muted-foreground/60"
              data-kind={stretch.kind}
              data-testid="coding-session-route-stretch"
              key={stretch.key}
              style={{
                left: `${signColumnX}px`,
                top: `${stretch.labelTopPx}px`,
              }}
            >
              · {stretch.label} ·
            </span>
          ))}
        </div>
      </div>
      <RouteNow
        route={route}
        onExpandRoad={onExpandRoad}
        onFocusRoad={onFocusRoad}
        stopProps={stopProps}
      />
      {/* R9, plus the two clauses a sighted reader gets from a cap and a
          tilde: which clock a sign is on, and where a road's start came
          from (REVIEW-A4 F1, F7). */}
      <ol aria-label="Route signs, oldest first" className="sr-only">
        {route.signs.map((sign) => (
          <li key={`sr-${sign.key}`}>
            {sign.title}
            {sign.timeSource === "local" ? " — local time, not signed" : ""}
          </li>
        ))}
      </ol>
      <ul aria-label="Route roads" className="sr-only">
        {route.roads.map((road) => (
          <li key={`sr-road-${road.key}`}>
            {road.label} — {codingSessionRouteRoadStartClause(road)}
          </li>
        ))}
      </ul>
    </nav>
  );
}

/** Roads, junction curves, bridges and dashed stretches, in one SVG layer. */
function RouteMap({
  onFocusRoad,
  route,
  stopProps,
}: {
  onFocusRoad?: (executionKey: string | null) => void;
  route: CodingSessionRoute;
  stopProps: (stopId: string) => { onFocus: () => void; tabIndex: number };
}) {
  const laneX = (lane: number) => MAP_PAD_X + lane * route.lanePitchPx;
  const signColumnX = codingSessionRouteSignColumnX(route);
  const laneOf = (key: string | null) =>
    route.roads.find((road) => road.key === key)?.lane ?? null;
  return (
    <>
      <svg
        aria-hidden
        className="pointer-events-none absolute inset-y-0 left-0 overflow-visible"
        width={signColumnX}
        height="100%"
      >
        <title>Route map</title>
        {route.roads.map((road) => (
          <g key={`road-${road.key}`}>
            <line
              className={cn(
                "stroke-current",
                road.founder ? "text-muted-foreground/50" : road.accent.text,
              )}
              strokeWidth={2}
              strokeLinecap="round"
              x1={laneX(road.lane)}
              x2={laneX(road.lane)}
              y1={road.startOffsetPx}
              y2={road.endOffsetPx}
            />
            {road.startedAt === null ? null : (
              // R1's start cap, and F1's honesty clause: a **filled** cap means
              // the road starts at the seat's own accepted create. A road whose
              // create was never observed starts at its first provider-authored
              // transcript time, which no signature covers — so it wears an
              // **open** cap and says which it is in the sr-only road list.
              <circle
                className={cn(
                  road.startedAtSource === "hire"
                    ? "fill-current"
                    : "fill-none stroke-current",
                  road.founder ? "text-muted-foreground/50" : road.accent.text,
                )}
                cx={laneX(road.lane)}
                cy={road.startOffsetPx}
                r={3}
                strokeWidth={1.5}
              />
            )}
            {/* R1's end cap: live is filled with a pulsing ring, idle is a
                hollow ring, released is a flat cap where the road stopped. */}
            {road.endedAt !== null ? (
              <line
                className={cn(
                  "stroke-current",
                  road.founder ? "text-muted-foreground/50" : road.accent.text,
                )}
                strokeWidth={2}
                x1={laneX(road.lane) - 3}
                x2={laneX(road.lane) + 3}
                y1={road.endOffsetPx}
                y2={road.endOffsetPx}
              />
            ) : road.live ? (
              <circle
                className={cn(
                  "fill-current coding-session-agent-breathe",
                  road.founder ? "text-muted-foreground/50" : road.accent.text,
                )}
                cx={laneX(road.lane)}
                cy={road.endOffsetPx}
                r={4}
              />
            ) : (
              <circle
                className={cn(
                  "fill-none stroke-current",
                  road.founder ? "text-muted-foreground/50" : road.accent.text,
                )}
                cx={laneX(road.lane)}
                cy={road.endOffsetPx}
                r={4}
                strokeWidth={1.5}
              />
            )}
          </g>
        ))}
        {/* R4's dotted tick from the road to the sign, plus the filled anchor
            an attention sign puts on its own lane. Without these the signs
            floated free of the lanes and the map read as a legend. */}
        {route.signs.map((sign) => {
          const lane = laneOf(sign.road);
          if (lane === null) return null;
          const road = route.roads[lane];
          if (road === undefined) return null;
          const y = sign.offsetPx + 10;
          const accent = road.founder
            ? "text-muted-foreground/50"
            : road.accent.text;
          return (
            <g key={`tick-${sign.key}`}>
              <line
                className={cn("stroke-current", accent)}
                strokeDasharray="1 2"
                strokeWidth={1}
                x1={laneX(lane) + 3}
                x2={signColumnX - 2}
                y1={y}
                y2={y}
              />
              {sign.weight === "attention" ? (
                <circle
                  className={cn(
                    "fill-current",
                    sign.tone === "critical"
                      ? "text-destructive"
                      : "text-amber-700 dark:text-amber-300",
                  )}
                  cx={laneX(lane)}
                  cy={y}
                  r={2.5}
                />
              ) : null}
            </g>
          );
        })}
        {route.stretches.map((stretch) => {
          const lanes =
            stretch.road === null
              ? route.roads.filter(
                  (road) =>
                    road.startOffsetPx <= stretch.offsetPx &&
                    road.endOffsetPx >= stretch.offsetPx,
                )
              : route.roads.filter((road) => road.key === stretch.road);
          return lanes.map((road) => (
            <line
              className={cn(
                "stroke-current",
                road.founder ? "text-muted-foreground/50" : road.accent.text,
              )}
              key={`${stretch.key}-${road.key}`}
              strokeDasharray={stretch.kind === "queued" ? "2 3" : "2 5"}
              strokeWidth={2}
              x1={laneX(road.lane)}
              x2={laneX(road.lane)}
              y1={stretch.offsetPx}
              y2={stretch.offsetPx + stretch.heightPx}
            />
          ));
        })}
        {route.bridges.map((bridge) => {
          const from = laneOf(bridge.from);
          const to = laneOf(bridge.to);
          if (from === null || to === null) return null;
          const x1 = laneX(from);
          const x2 = laneX(to);
          const thin =
            bridge.kind === "disposition" || bridge.kind === "acknowledgement";
          const accent =
            route.roads.find((road) => road.key === bridge.from)?.accent.text ??
            "text-muted-foreground";
          const drop = bridge.kind === "hire" ? 20 : 8;
          const tip = bridge.offsetPx + drop;
          const back = x2 > x1 ? -3.5 : 3.5;
          return (
            <g key={bridge.key}>
              <path
                className={cn("fill-none stroke-current", accent)}
                d={`M ${x1} ${bridge.offsetPx} C ${x1} ${tip}, ${x2} ${bridge.offsetPx}, ${x2} ${tip}`}
                strokeWidth={thin ? 1 : 1.5}
              />
              {/* R3's 7 px arrowhead, at the counterparty's lane: a handoff has
                  a direction, and a line without one states half the fact. */}
              <path
                className={cn("fill-current stroke-none", accent)}
                d={`M ${x2} ${tip + 3.5} L ${x2 + back} ${tip - 3.5} L ${x2 - back} ${tip - 3.5} Z`}
              />
            </g>
          );
        })}
      </svg>
      {route.roads.map((road) => (
        <button
          aria-label={`Focus ${road.label} — ${codingSessionRouteRoadStartClause(road)}`}
          className="absolute w-3 cursor-pointer focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring"
          data-testid="coding-session-route-road"
          key={`hit-${road.key}`}
          onClick={() =>
            onFocusRoad?.(road.founder ? null : (road.key as string))
          }
          title={codingSessionRouteRoadStartClause(road)}
          {...stopProps(`road:${road.key}`)}
          style={{
            left: `${laneX(road.lane) - 6}px`,
            top: `${road.startOffsetPx}px`,
            height: `${Math.max(12, road.endOffsetPx - road.startOffsetPx)}px`,
          }}
          type="button"
        />
      ))}
    </>
  );
}

/**
 * R6 — the slice of the map the reader is currently looking at.
 *
 * The band is the tint alone. Its sentence lives above the Now rule
 * ({@link RouteNow}) rather than floating inside the map: a band over rows
 * signed seconds apart is a few pixels tall, and a label pinned to it landed
 * on top of the very signs it was meant to locate. Deviation from §9's board,
 * where the sample band spans twelve minutes and has room for its own words.
 */
function RouteBand({ route }: { route: CodingSessionRoute }) {
  if (route.here === null) return null;
  return (
    <div
      className="pointer-events-none absolute inset-x-0 border-primary/60 border-y bg-primary/5"
      data-testid="coding-session-route-band"
      style={{
        top: `${route.here.offsetPx}px`,
        height: `${route.here.heightPx}px`,
      }}
    />
  );
}

/** R7 and R8 — the Now rule, and what every road head is holding under it. */
function RouteNow({
  onExpandRoad,
  onFocusRoad,
  route,
  stopProps,
}: {
  onExpandRoad?: (executionKey: string) => void;
  onFocusRoad?: (executionKey: string | null) => void;
  route: CodingSessionRoute;
  stopProps: (stopId: string) => { onFocus: () => void; tabIndex: number };
}) {
  const nowLabel = new Date(route.nowAt * 1_000).toLocaleTimeString([], {
    hour: "numeric",
    minute: "2-digit",
  });
  return (
    <div
      className="shrink-0 border-t border-border pt-1.5 pb-2 pl-4"
      data-testid="coding-session-route-now"
    >
      {route.here === null ? null : (
        <p
          className="mb-1 text-2xs font-semibold text-primary"
          data-testid="coding-session-route-here-label"
        >
          You are here
          {route.here.toNowLabel === null ? null : (
            <span className="ml-1 font-normal text-muted-foreground">
              · {route.here.toNowLabel}
            </span>
          )}
        </p>
      )}
      <p className="text-2xs font-semibold text-foreground">Now · {nowLabel}</p>
      <ul className="mt-1.5 grid gap-1.5">
        {route.roads.map((road) => (
          <RouteHead
            key={`head-${road.key}`}
            onExpand={onExpandRoad}
            onFocus={onFocusRoad}
            road={road}
            stopProps={stopProps}
          />
        ))}
        {route.hiddenRoadCount > 0 ? (
          <li className="text-2xs text-muted-foreground">
            {route.hiddenRoadCount} more participants not drawn
          </li>
        ) : null}
      </ul>
    </div>
  );
}

function RouteHead({
  onExpand,
  onFocus,
  road,
  stopProps,
}: {
  onExpand?: (executionKey: string) => void;
  onFocus?: (executionKey: string | null) => void;
  road: CodingSessionRouteRoad;
  stopProps: (stopId: string) => { onFocus: () => void; tabIndex: number };
}) {
  // A5: one string, built once, in `codingSessionMissionOpenHolds` — the rail
  // head and the Inspector's Team row say the same sentence because neither
  // writes it. The clock is the half a reader who looked away could not get
  // from a ticking duration alone.
  const held = codingSessionOpenHoldStatusLine({
    holding: road.head.holding,
    sinceAt: road.head.sinceAt,
    sinceMs: road.head.sinceMs,
  });
  return (
    <li data-testid="coding-session-route-head">
      <button
        className="flex w-full items-center gap-1.5 text-left"
        onClick={() => onFocus?.(road.founder ? null : (road.key as string))}
        title={codingSessionRouteRoadStartClause(road)}
        {...stopProps(`head:${road.key}`)}
        type="button"
      >
        <span
          aria-hidden
          className={cn(
            "size-2 shrink-0 rounded-full",
            road.founder ? "bg-muted-foreground/50" : road.accent.dot,
            // §9.9: the live head pulses at the same tempo as every other
            // liveness mark in the app, and `coding-session.css` turns that
            // animation off under `prefers-reduced-motion`.
            road.live && "coding-session-agent-breathe",
          )}
        />
        <span className="min-w-0 truncate text-xs font-medium text-foreground">
          {road.label}
        </span>
        {road.head.word === null ? null : (
          <span
            className={cn(
              "shrink-0 text-2xs",
              road.live
                ? "text-emerald-600 dark:text-emerald-400"
                : "text-muted-foreground",
            )}
            data-testid="coding-session-route-head-word"
          >
            {road.head.word}
          </span>
        )}
      </button>
      {held === null ? null : (
        <p className="pl-3.5 text-2xs text-muted-foreground">{held}</p>
      )}
      {road.head.seatAuthorityDetail === null ? null : (
        <p
          className="flex items-center gap-1 pl-3.5 text-2xs text-amber-700 dark:text-amber-300"
          data-testid="coding-session-route-head-ungranted"
          title={road.head.seatAuthorityRemedy ?? undefined}
        >
          <Flag aria-hidden className="size-3 shrink-0" />
          {road.head.seatAuthorityDetail}
        </p>
      )}
      {road.undatedSignCount > 0 ? (
        <p
          className="pl-3.5 text-2xs text-muted-foreground"
          data-testid="coding-session-route-head-undated"
          title="A wake whose local observation never landed has no moment to place."
        >
          {road.undatedSignCount} undated
        </p>
      ) : null}
      {road.hiddenSignCount > 0 ? (
        // §9.5's marker, and it does what the spec says: clicking it lifts
        // this road's bound in place rather than stating a number the reader
        // cannot act on.
        <button
          className="pl-3.5 text-2xs text-muted-foreground/70 underline underline-offset-2 hover:text-foreground"
          data-testid="coding-session-route-earlier"
          disabled={road.key === null || onExpand === undefined}
          onClick={() => {
            if (road.key !== null) onExpand?.(road.key);
          }}
          {...stopProps(`earlier:${road.key}`)}
          type="button"
        >
          +{road.hiddenSignCount} earlier
        </button>
      ) : null}
    </li>
  );
}

function RouteSignButton({
  index,
  onReveal,
  registerRef,
  roadAccentText,
  selected,
  sign,
  signColumnX,
  stopProps,
}: {
  index: number;
  onReveal?: (rowKey: string) => void;
  registerRef: (node: HTMLButtonElement | null) => void;
  roadAccentText: string;
  selected: boolean;
  sign: CodingSessionRouteSign;
  /** Left edge of the sign column — the lanes actually drawn, not a constant. */
  signColumnX: number;
  stopProps: { onFocus: () => void; tabIndex: number };
}) {
  const Icon = signIcon(sign);
  const critical = sign.tone === "critical";
  const caution = sign.tone === "caution";
  const local = sign.timeSource === "local";
  return (
    <button
      aria-label={local ? `${sign.title} — local time, not signed` : sign.title}
      aria-pressed={selected}
      className={cn(
        // B5: the sign takes whatever the rail leaves right of the lanes,
        // instead of a hard 128 px inside a 224 px rail. A name clipped to
        // `Buil…` costs the same pixels as the whole word and carries none of
        // it — and with the drag handle above, widening the rail now widens
        // the sign, which is why B1 and B5 are one fix.
        "absolute flex h-5 items-center gap-1 rounded px-1 text-left text-2xs font-medium transition-colors",
        "text-muted-foreground hover:bg-muted/25 hover:text-foreground",
        "focus-visible:outline-none focus-visible:bg-primary/5 focus-visible:ring-1 focus-visible:ring-primary/60",
        critical && "font-semibold text-destructive",
        caution && "text-amber-700 dark:text-amber-300",
      )}
      data-index={index}
      data-kind={sign.kind}
      data-testid="coding-session-route-sign"
      data-time-source={sign.timeSource}
      onClick={() => {
        if (sign.revealKey != null) onReveal?.(sign.revealKey);
      }}
      ref={registerRef}
      style={{
        insetInlineStart: `${signColumnX}px`,
        insetInlineEnd: `${MAP_PAD_X / 4}px`,
        top: `${sign.offsetPx}px`,
      }}
      {...stopProps}
      title={local ? `~ ${sign.title} (local time, not signed)` : sign.title}
      type="button"
    >
      {/* F7: a sign placed on Desktop's own observation is drawn hollow and
          prefixed `~`. Every other dot on this map sits on a signed
          `created_at`, and a mixed axis that looks uniform is the map lying
          about its own units. */}
      <span
        aria-hidden
        className={cn(
          "size-1.5 shrink-0 rounded-full",
          local ? "border border-current" : "bg-current",
          sign.tone === null && roadAccentText,
        )}
      />
      {local ? (
        <span aria-hidden className="shrink-0 text-muted-foreground/70">
          ~
        </span>
      ) : null}
      <Icon aria-hidden className="size-3 shrink-0" />
      <span className="min-w-0 truncate">{sign.word}</span>
      {sign.requiresDecision ? (
        <Scale
          aria-hidden
          className="size-3 shrink-0 text-amber-700 dark:text-amber-300"
          data-testid="coding-session-route-decision"
        />
      ) : null}
    </button>
  );
}

/**
 * How a road's start was established, as a sentence.
 *
 * The clause behind F1: a filled start cap means the road begins at the seat's
 * own accepted 44221 create, and an open one means it begins at the seat's
 * first provider-authored transcript time, which no signature covers. The rail
 * says which in the sr-only road list and in every road control's tooltip, so
 * the distinction is not carried by a 3 px circle alone.
 */
export function codingSessionRouteRoadStartClause(
  road: CodingSessionRouteRoad,
): string {
  if (road.founder) return "the founder is present throughout";
  if (road.startedAtSource === "hire") return "road starts since its create";
  if (road.startedAtSource === "first-signed") {
    return "road starts since its first signed sign, not a create";
  }
  return "road start not reported";
}

/** One stretch label with the top the layout gave it. */
type RouteStretchLabel = CodingSessionRoute["stretches"][number] & {
  labelTopPx: number;
};

/** Minimum vertical gap between two duration labels, in px. */
const STRETCH_LABEL_GAP_PX = 14;

/**
 * Lay out the duration labels top-down so two never collide.
 *
 * Two queued wakes on one lane over overlapping spans put their labels a few
 * pixels apart at `text-2xs`, legible only by accident (REVIEW-A4 F15). Each
 * label takes its natural mid-stretch position unless that would put it inside
 * {@link STRETCH_LABEL_GAP_PX} of the previous one, in which case it stacks
 * below it. The label text — the measurement — is never changed.
 */
export function layOutRouteStretchLabels(
  route: CodingSessionRoute,
): RouteStretchLabel[] {
  const ordered = [...route.stretches].sort(
    (left, right) => left.offsetPx - right.offsetPx,
  );
  let lastTop = Number.NEGATIVE_INFINITY;
  return ordered.map((stretch) => {
    const natural = stretch.offsetPx + stretch.heightPx / 2 - 8;
    const labelTopPx = Math.max(natural, lastTop + STRETCH_LABEL_GAP_PX);
    lastTop = labelTopPx;
    return { ...stretch, labelTopPx };
  });
}
