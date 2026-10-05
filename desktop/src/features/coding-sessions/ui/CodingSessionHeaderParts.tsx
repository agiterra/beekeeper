import type * as React from "react";
import { PanelLeft } from "lucide-react";

import type { CodingSessionWorkspaceStatus } from "@/features/coding-sessions/lib/codingSessionTypes";
import { Button } from "@/shared/ui/button";
import { cn } from "@/shared/lib/cn";

import { CODING_SESSION_ROUTE_RAIL_ID } from "./CodingSessionRouteRail";

/**
 * Pieces of the session header, split out of `CodingSessionHeader.tsx` so the
 * header itself reads as a layout: breadcrumb, status, a few primary controls,
 * the `⋯` menu and the two panel toggles (SV-20).
 */

/**
 * The shape the header's surface toggles had before SV-20 moved surfaces to
 * the launcher. Nothing renders or builds it any more (B1's
 * `CodingSessionHeaderPanelToggles` replaced the toggles, and the registry
 * shell's legacy builder is gone); it survives only as a re-exported type.
 */
export type CodingSessionHeaderSurfaceTab = {
  id: string;
  label: string;
  /** A named legacy glyph, or the surface's own icon from the registry. */
  icon:
    | "agents"
    | "changes"
    | "inspector"
    | React.ComponentType<{ className?: string }>;
  count?: number;
  active: boolean;
};

/**
 * The colour of the status dot. Working is green; resting is muted; founded
 * and never started is hollow (a filled colour would claim a state nobody
 * published); a lifecycle-signed attention state is destructive, as the
 * execution rail draws it; everything else (waiting, unknown) is amber.
 *
 * Working pulses, as T3 Code's status pill does (`Sidebar.logic.ts`
 * `resolveThreadStatusPill`, `pulse: true`), and only where the person has
 * not asked for reduced motion.
 */
export function codingSessionHeaderStatusDotClass(
  status: CodingSessionWorkspaceStatus,
  sessionClosed: boolean,
): string {
  if (!sessionClosed && status.kind === "working") {
    return "bg-emerald-500 motion-safe:animate-pulse";
  }
  if (sessionClosed || status.kind === "idle" || status.kind === "ended") {
    return "bg-muted-foreground/50";
  }
  if (status.kind === "founded") {
    return "bg-transparent ring-1 ring-inset ring-muted-foreground/50";
  }
  if (status.kind === "unknown" && status.attention) return "bg-destructive";
  return "bg-amber-500";
}

/**
 * The session's status: a dot and its word, beside the title (SV-20).
 *
 * T3 Code shows only a dot in its header; Beekeeper keeps the word on screen,
 * because the status is a fact a person acts on and a colour alone does not
 * say which one. A demoted status keeps its history clause ("last reported
 * Idle 2h ago") in the tooltip and the accessible name, and in the text a
 * screen reader or a search reads — it is detail one hover away, never gone.
 * The badge never shrinks: in a narrow window the title truncates first, so
 * the word cannot be squeezed to "Wo…" beside a long title.
 */
export function CodingSessionHeaderStatusBadge({
  detail,
  sessionClosed,
  status,
  word,
  testId = "coding-session-status-badge",
}: {
  /** The history clause of a demoted status, or null. */
  detail: string | null;
  sessionClosed: boolean;
  status: CodingSessionWorkspaceStatus;
  /** The word on screen: the status label, or the caller's aggregate. */
  word: string;
  testId?: string;
}) {
  const shownWord = sessionClosed ? "Closed" : word;
  const fullText =
    sessionClosed || detail === null ? shownWord : `${shownWord} · ${detail}`;
  return (
    <span
      aria-label={`Session status: ${fullText}`}
      className="inline-flex shrink-0 items-center gap-1.5 text-xs text-muted-foreground"
      data-status-kind={sessionClosed ? "closed" : status.kind}
      data-testid={testId}
      role="img"
      title={fullText}
    >
      <span
        aria-hidden
        className={cn(
          "size-2 shrink-0 rounded-full",
          codingSessionHeaderStatusDotClass(status, sessionClosed),
        )}
      />
      <span aria-hidden className="whitespace-nowrap">
        {shownWord}
      </span>
      {!sessionClosed && detail !== null ? (
        <span aria-hidden className="sr-only">
          {` · ${detail}`}
        </span>
      ) : null}
    </span>
  );
}

/**
 * Mission's Route rail collapse control. It stays in the header in Mission
 * only (SV-20): the rail is Mission's left-edge navigator, not a surface, so
 * it has no launcher row.
 */
export function CodingSessionHeaderRouteToggle({
  compact,
  displacesInspector,
  expanded,
  onToggle,
}: {
  compact: boolean;
  displacesInspector: boolean;
  expanded: boolean;
  onToggle: () => void;
}) {
  return (
    <Button
      // F7: the IDREF has to land on something. Both the expanded rail and
      // the 40 px scrubber carry this id, because the control governs
      // whichever of the two is mounted.
      aria-controls={CODING_SESSION_ROUTE_RAIL_ID}
      aria-expanded={expanded}
      aria-label={expanded ? "Collapse route rail" : "Expand route rail"}
      aria-pressed={expanded}
      data-testid="coding-session-route-toggle"
      onClick={onToggle}
      size={compact ? "icon" : "sm"}
      title={
        expanded
          ? "Collapse the route rail to its scrubber"
          : displacesInspector
            ? "Expand the route rail — closes the Inspector, which this width cannot hold beside it"
            : "Expand the route rail"
      }
      type="button"
      variant={expanded ? "secondary" : "ghost"}
    >
      <PanelLeft />
      <span className={compact ? "sr-only" : undefined}>Route</span>
    </Button>
  );
}
