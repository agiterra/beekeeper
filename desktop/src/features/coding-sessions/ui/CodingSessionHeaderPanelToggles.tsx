import { PanelBottom, PanelRight } from "lucide-react";
import * as React from "react";

import { isMacPlatform } from "@/shared/lib/platform";
import { cn } from "@/shared/lib/cn";
import { Button } from "@/shared/ui/button";
import {
  Tooltip,
  TooltipContent,
  TooltipProvider,
  TooltipTrigger,
} from "@/shared/ui/tooltip";

import {
  type CodingSessionSurfaceBadgeTone,
  strongestCodingSessionSurfaceBadgeTone,
} from "@/features/coding-sessions/lib/codingSessionSurfaceBadgeModel";

import type { CodingSessionSurfaceCtx } from "./surfaces/codingSessionSurfaceContext";
import type {
  CodingSessionResolvedSurface,
  CodingSessionSurfacePlacement,
} from "./surfaces/codingSessionSurfaceRegistry";
import type {
  CodingSessionHeaderPanels,
  CodingSessionSurfacePanelState,
} from "./surfaces/useCodingSessionSurfacePanels";

/**
 * The header's two layout toggles (SV-20): the bottom panel (⌘J) and the right
 * panel (⌘⌥B).
 *
 * Ported from T3 Code's `PanelLayoutControls` (`components/chat/PanelLayoutControls.tsx`):
 * ghost toggles with a pressed state, the bottom one dimmed with a reason when
 * it cannot open, and an attention dot on the corner. Two Beekeeper rules on
 * top of it:
 *
 * - **A live badge must not hide behind a click.** Each toggle carries one dot
 *   in the strongest tone among the badges of its own placement's surfaces
 *   that are not on screen (the right panel's for ⌘⌥B, the drawer's for ⌘J),
 *   and its tooltip and accessible name list them. T3's dot means one thing
 *   (thread attention); here it means "something you cannot see is live".
 * - **The tone comes from the surface's own Badge.** Each off-screen surface's
 *   registered `Badge` is rendered once, hidden, in the `header` slot, and the
 *   toggle reads the `data-tone` it draws. The header never recomputes what a
 *   badge means, so the dot and the badge cannot disagree.
 */

/** The badge tones, weakest first (DB5); the ranking lives in B3's model. */
export const CODING_SESSION_BADGE_TONES = [
  "neutral",
  "activity",
  "waiting",
  "attention",
] as const satisfies readonly CodingSessionSurfaceBadgeTone[];

export type CodingSessionBadgeTone = CodingSessionSurfaceBadgeTone;

/** One off-screen surface whose badge drew something. */
export type CodingSessionOffscreenBadge = {
  id: string;
  label: string;
  tone: CodingSessionBadgeTone;
  /** The badge's own accessible sentence, when it gave one. */
  description: string | null;
};

/**
 * The strongest tone among badges, or null when there are none. One source
 * for the DB5 order: this defers to the surface badge model's ranking.
 */
export function strongestCodingSessionBadgeTone(
  badges: readonly Pick<CodingSessionOffscreenBadge, "tone">[],
): CodingSessionBadgeTone | null {
  return strongestCodingSessionSurfaceBadgeTone(badges);
}

/**
 * Is this surface's badge already on screen?
 *
 * A right-panel surface's badge shows when the panel is open and either the
 * launcher is up (every row has its badge) or the surface is one of the tabs
 * (every tab has its badge). A drawer surface shows when the drawer is open.
 */
export function codingSessionSurfaceBadgeOnScreen(
  placement: CodingSessionSurfacePlacement,
  id: string,
  state: Pick<
    CodingSessionSurfacePanelState,
    "active" | "bottomOpen" | "rightOpen" | "tabs"
  >,
): boolean {
  if (placement === "drawer") return state.bottomOpen;
  if (!state.rightOpen) return false;
  return state.active === null || state.tabs.includes(id);
}

function parseTone(value: string | null): CodingSessionBadgeTone | null {
  return (CODING_SESSION_BADGE_TONES as readonly string[]).includes(value ?? "")
    ? (value as CodingSessionBadgeTone)
    : null;
}

/**
 * Read what each probe wrapper's badge drew. A badge that drew an element
 * with no `data-tone` reads as `neutral`: something is there, and the header
 * claims no more about it than that.
 */
export function readCodingSessionBadgeProbe(
  root: ParentNode,
  labels: ReadonlyMap<string, string>,
): CodingSessionOffscreenBadge[] {
  const badges: CodingSessionOffscreenBadge[] = [];
  for (const wrapper of Array.from(
    root.querySelectorAll<HTMLElement>("[data-badge-probe]"),
  )) {
    if (wrapper.childElementCount === 0) continue;
    const id = wrapper.dataset.badgeProbe ?? "";
    const toned = wrapper.querySelector<HTMLElement>("[data-tone]");
    const tone = parseTone(toned?.getAttribute("data-tone") ?? null);
    const described =
      toned ?? wrapper.querySelector<HTMLElement>("[aria-label]");
    badges.push({
      id,
      label: labels.get(id) ?? id,
      tone: tone ?? "neutral",
      description: described?.getAttribute("aria-label") ?? null,
    });
  }
  return badges;
}

function sameBadges(
  left: readonly CodingSessionOffscreenBadge[],
  right: readonly CodingSessionOffscreenBadge[],
): boolean {
  return (
    left.length === right.length &&
    left.every(
      (badge, index) =>
        badge.id === right[index]?.id &&
        badge.tone === right[index]?.tone &&
        badge.description === right[index]?.description,
    )
  );
}

/**
 * Render the given surfaces' badges, hidden, and report what they drew. One
 * instance per surface, in the `header` slot; re-read after every render and
 * whenever a badge changes its own DOM (badges hold their own state).
 */
function useOffscreenBadges(
  surfaces: readonly CodingSessionResolvedSurface[],
  ctx: CodingSessionSurfaceCtx | null,
): {
  probe: React.ReactNode;
  badges: CodingSessionOffscreenBadge[];
} {
  const rootRef = React.useRef<HTMLSpanElement | null>(null);
  const [badges, setBadges] = React.useState<CodingSessionOffscreenBadge[]>([]);
  const labels = React.useMemo(
    () =>
      new Map(
        surfaces.map(({ definition }) => [definition.id, definition.label]),
      ),
    [surfaces],
  );
  const read = React.useCallback(() => {
    const root = rootRef.current;
    const next = root ? readCodingSessionBadgeProbe(root, labels) : [];
    setBadges((previous) => (sameBadges(previous, next) ? previous : next));
  }, [labels]);
  React.useLayoutEffect(read);
  React.useEffect(() => {
    const root = rootRef.current;
    if (!root || typeof MutationObserver === "undefined") return;
    const observer = new MutationObserver(read);
    observer.observe(root, {
      attributes: true,
      characterData: true,
      childList: true,
      subtree: true,
    });
    return () => observer.disconnect();
  }, [read]);
  const withBadge = surfaces.filter(({ definition }) => definition.Badge);
  const probe =
    ctx && withBadge.length > 0 ? (
      <span
        className="hidden"
        data-testid="coding-session-header-badge-probe"
        hidden
        ref={rootRef}
      >
        {withBadge.map(({ definition }) => {
          const Badge = definition.Badge;
          return Badge ? (
            <span data-badge-probe={definition.id} key={definition.id}>
              <Badge ctx={ctx} slot="header" />
            </span>
          ) : null;
        })}
      </span>
    ) : null;
  return { probe, badges: ctx ? badges : [] };
}

const DOT_TONE_CLASS: Record<CodingSessionBadgeTone, string> = {
  attention: "bg-destructive",
  waiting: "bg-amber-500",
  activity: "bg-primary",
  neutral: "bg-muted-foreground/60",
};

const TONE_WORD: Record<CodingSessionBadgeTone, string> = {
  attention: "needs attention",
  waiting: "waiting",
  activity: "active",
  neutral: "has something new",
};

/** "Landing needs attention", or the badge's own sentence. */
function offscreenSentence(badge: CodingSessionOffscreenBadge): string {
  return badge.description
    ? `${badge.label}: ${badge.description}`
    : `${badge.label} ${TONE_WORD[badge.tone]}`;
}

function shortcutLabel(key: "J" | "B", alt: boolean): string {
  if (isMacPlatform()) return `⌘${alt ? "⌥" : ""}${key}`;
  return `Ctrl+${alt ? "Alt+" : ""}${key}`;
}

function PanelToggle({
  ariaControls,
  dimmedReason,
  icon,
  label,
  offscreen,
  onToggle,
  pressed,
  shortcut,
  testId,
}: {
  ariaControls?: string;
  dimmedReason: string | null;
  icon: React.ReactNode;
  label: string;
  offscreen: readonly CodingSessionOffscreenBadge[];
  onToggle: () => void;
  pressed: boolean;
  shortcut: string;
  testId: string;
}) {
  const tone = strongestCodingSessionBadgeTone(offscreen);
  // Dimmed while what it opens cannot work here, but never refused: the click
  // opens the panel just as the shortcut does, and the panel shows the reason
  // (it is also where a teammate's shared terminal appears). Once open, the
  // toggle closes it again.
  const dimmed = dimmedReason !== null && !pressed;
  const sentences = offscreen.map(offscreenSentence);
  const accessibleName = [
    label,
    dimmed ? `unavailable: ${dimmedReason}` : null,
    sentences.length > 0 ? `not on screen: ${sentences.join("; ")}` : null,
  ]
    .filter(Boolean)
    .join("; ");
  return (
    <Tooltip>
      <TooltipTrigger asChild>
        <Button
          aria-controls={ariaControls}
          aria-label={accessibleName}
          aria-pressed={pressed}
          className={cn("relative shrink-0", dimmed && "opacity-50")}
          data-testid={testId}
          data-unavailable={dimmed ? "true" : undefined}
          onClick={onToggle}
          size="icon"
          type="button"
          variant={pressed ? "secondary" : "ghost"}
        >
          {icon}
          {tone ? (
            <span
              aria-hidden
              className={cn(
                "absolute top-1 right-1 size-1.5 rounded-full ring-2 ring-background",
                DOT_TONE_CLASS[tone],
              )}
              data-testid={`${testId}-dot`}
              data-tone={tone}
            />
          ) : null}
        </Button>
      </TooltipTrigger>
      <TooltipContent
        className="max-w-72"
        data-testid={`${testId}-tooltip`}
        side="bottom"
      >
        <p>
          {dimmed ? dimmedReason : `${label} (${shortcut})`}
          {dimmed ? <span className="block opacity-80">{shortcut}</span> : null}
        </p>
        {sentences.length > 0 ? (
          <ul className="mt-1 space-y-0.5 opacity-90">
            {sentences.map((sentence) => (
              <li key={sentence}>{sentence}</li>
            ))}
          </ul>
        ) : null}
      </TooltipContent>
    </Tooltip>
  );
}

/** What the header needs from the workspace's surface shell. */
export type CodingSessionHeaderSurfaceShell = {
  ctx: CodingSessionSurfaceCtx;
  headerPanels: CodingSessionHeaderPanels;
  surfaces: readonly CodingSessionResolvedSurface[];
};

export function CodingSessionHeaderPanelToggles({
  shell,
  surfaceHostId,
}: {
  shell: CodingSessionHeaderSurfaceShell;
  /** DOM id of the right panel, for `aria-controls`. */
  surfaceHostId?: string;
}) {
  const { ctx, headerPanels, surfaces } = shell;
  const offscreenSurfaces = React.useMemo(
    () =>
      surfaces.filter(
        ({ definition }) =>
          !codingSessionSurfaceBadgeOnScreen(
            definition.placement,
            definition.id,
            ctx.panelState,
          ),
      ),
    [ctx.panelState, surfaces],
  );
  const { badges, probe } = useOffscreenBadges(offscreenSurfaces, ctx);
  const placementOf = React.useMemo(
    () =>
      new Map(
        surfaces.map(({ definition }) => [definition.id, definition.placement]),
      ),
    [surfaces],
  );
  const drawerBadges = badges.filter(
    (badge) => placementOf.get(badge.id) === "drawer",
  );
  const rightBadges = badges.filter(
    (badge) => placementOf.get(badge.id) === "right",
  );
  return (
    <TooltipProvider delayDuration={300}>
      <div
        className="flex shrink-0 items-center gap-0.5"
        data-testid="coding-session-panel-toggles"
      >
        <PanelToggle
          dimmedReason={headerPanels.bottomUnavailableReason}
          icon={<PanelBottom />}
          label="Toggle bottom panel"
          offscreen={drawerBadges}
          onToggle={headerPanels.onToggleBottom}
          pressed={headerPanels.bottomOpen}
          shortcut={shortcutLabel("J", false)}
          testId="coding-session-panel-toggle-bottom"
        />
        <PanelToggle
          ariaControls={surfaceHostId}
          dimmedReason={null}
          icon={<PanelRight />}
          label="Toggle right panel"
          offscreen={rightBadges}
          onToggle={headerPanels.onToggleRight}
          pressed={headerPanels.rightOpen}
          shortcut={shortcutLabel("B", true)}
          testId="coding-session-panel-toggle-right"
        />
        {probe}
      </div>
    </TooltipProvider>
  );
}
