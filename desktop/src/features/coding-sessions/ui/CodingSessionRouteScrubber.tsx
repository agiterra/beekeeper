import * as React from "react";
import {
  Clock3,
  Flag,
  OctagonAlert,
  Scale,
  type LucideIcon,
} from "lucide-react";

import {
  codingSessionRouteAttentionSigns,
  type CodingSessionRoute,
  type CodingSessionRouteSign,
} from "@/features/coding-sessions/lib/codingSessionRouteModel";
import { cn } from "@/shared/lib/cn";
import { Sheet, SheetContent, SheetTitle } from "@/shared/ui/sheet";
import { CodingSessionRouteRail } from "./CodingSessionRouteRail";

/** Breathing room under the newest mark so Now never sits on top of one. */
const TRACK_TAIL_PX = 16;

function scrubberIcon(sign: CodingSessionRouteSign): LucideIcon {
  if (sign.kind === "mission.blocked") return OctagonAlert;
  if (sign.kind === "seat-ungranted") return Flag;
  if (sign.kind === "delivery")
    return sign.tone === "critical" ? OctagonAlert : Clock3;
  return Scale;
}

/**
 * The Route rail folded to 40 px — DESIGN-SPEC §9 R10.
 *
 * Below the rail's width gates the map cannot be drawn without squeezing the
 * reading column, so it folds to a track carrying only what a reader is not
 * allowed to miss: a blocker, a decision held on a person, a failed wake, an
 * ungranted seat — plus Now. Tapping it opens the wide rail's own markup in a
 * sheet, so the folded state hides detail and never hides a fact.
 *
 * The `aria-label` names the attention signs in words, because a viewer who
 * cannot see the glyphs must still learn why the track is not empty.
 */
export function CodingSessionRouteScrubber({
  className,
  onExpandRoad,
  onFocusRoad,
  onRevealRow,
  route,
}: {
  className?: string;
  onExpandRoad?: (executionKey: string) => void;
  onFocusRoad?: (executionKey: string | null) => void;
  onRevealRow?: (rowKey: string) => void;
  route: CodingSessionRoute;
}) {
  const [open, setOpen] = React.useState(false);
  const attention = React.useMemo(
    () => codingSessionRouteAttentionSigns(route),
    [route],
  );
  const label =
    attention.length === 0
      ? "Route — tap to expand"
      : `Route — ${attention.map((sign) => sign.word).join(" · ")} · tap to expand`;
  const trackHeightPx = route.heightPx + TRACK_TAIL_PX;
  return (
    <>
      <button
        aria-expanded={open}
        aria-label={label}
        className={cn(
          "relative flex w-10 shrink-0 flex-col items-center border-r border-border/50 bg-background py-2 transition-colors hover:bg-muted/20",
          className,
        )}
        data-testid="coding-session-route-scrubber"
        onClick={() => setOpen(true)}
        title={label}
        type="button"
      >
        <span className="relative min-h-0 w-full flex-1 overflow-hidden">
          <span
            aria-hidden
            className="absolute inset-y-0 left-1/2 w-0.5 -translate-x-1/2 rounded-full bg-border"
          />
          {attention.map((sign) => {
            const Icon = scrubberIcon(sign);
            const ratio =
              trackHeightPx === 0 ? 0 : sign.offsetPx / trackHeightPx;
            return (
              <span
                aria-hidden
                className={cn(
                  "absolute left-1/2 grid size-4 -translate-x-1/2 place-items-center rounded-full bg-background",
                  sign.tone === "critical"
                    ? "text-destructive"
                    : "text-amber-700 dark:text-amber-300",
                )}
                data-testid="coding-session-route-scrubber-sign"
                key={sign.key}
                style={{ top: `${Math.min(92, ratio * 100)}%` }}
              >
                <Icon className="size-3.5" />
              </span>
            );
          })}
          {route.here === null ? null : (
            <span
              aria-hidden
              className="absolute inset-x-1 rounded-sm border-primary/60 border-y bg-primary/5"
              data-testid="coding-session-route-scrubber-band"
              style={{
                top: `${Math.min(90, trackHeightPx === 0 ? 0 : (route.here.offsetPx / trackHeightPx) * 100)}%`,
                height: `${Math.max(6, trackHeightPx === 0 ? 6 : (route.here.heightPx / trackHeightPx) * 100)}%`,
              }}
            />
          )}
        </span>
        <span
          aria-hidden
          className="mt-1 size-1.5 shrink-0 rounded-full bg-foreground"
        />
        <span className="text-3xs text-muted-foreground">Now</span>
      </button>
      <Sheet onOpenChange={setOpen} open={open}>
        <SheetContent
          aria-describedby={undefined}
          className="flex w-[min(90vw,18rem)] max-w-none flex-col p-0"
          side="left"
        >
          <SheetTitle className="px-4 pt-4 text-sm">Route</SheetTitle>
          <CodingSessionRouteRail
            onExpandRoad={onExpandRoad}
            onFocusRoad={onFocusRoad}
            onRevealRow={(rowKey) => {
              onRevealRow?.(rowKey);
              setOpen(false);
            }}
            route={route}
            variant="sheet"
          />
        </SheetContent>
      </Sheet>
    </>
  );
}
