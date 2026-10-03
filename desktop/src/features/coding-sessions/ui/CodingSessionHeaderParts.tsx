import {
  GitCompare,
  ListChecks,
  PanelLeft,
  PanelRight,
  Users,
} from "lucide-react";

import type { CodingSessionWorkspaceStatus } from "@/features/coding-sessions/lib/codingSessionTypes";
import { Badge } from "@/shared/ui/badge";
import { Button } from "@/shared/ui/button";
import { cn } from "@/shared/lib/cn";

import { CODING_SESSION_ROUTE_RAIL_ID } from "./CodingSessionRouteRail";
import { CODING_SESSION_TASK_RAIL_ID } from "./CodingSessionTaskRail";

/**
 * Pieces of the session header, split out of `CodingSessionHeader.tsx` so the
 * header itself reads as a layout: title, status, a few primary controls and
 * the `⋯` menu. Each part renders exactly what it rendered inline before;
 * only where it sits changed.
 */

/**
 * A compact direct affordance for one surface tab of the shared right-side
 * surface host. Clicking an inactive affordance opens the host on that tab;
 * clicking the active one closes the host.
 */
export type CodingSessionHeaderSurfaceTab = {
  id: string;
  label: string;
  icon: "agents" | "changes" | "inspector";
  count?: number;
  active: boolean;
};

/** The session's one status word, always visible — never folded away. */
export function CodingSessionHeaderStatusBadge({
  compact,
  sessionClosed,
  status,
  statusText,
  testId = "coding-session-status-badge",
}: {
  compact: boolean;
  sessionClosed: boolean;
  status: CodingSessionWorkspaceStatus;
  statusText: string;
  /** A second mount (the narrow fallback) carries its own id. */
  testId?: string;
}) {
  return (
    <Badge
      aria-label={`Session status: ${sessionClosed ? "Closed" : statusText}`}
      className={cn("gap-1.5", compact && "px-2")}
      data-testid={testId}
      title={sessionClosed ? "Closed" : statusText}
      variant="outline"
    >
      <span
        aria-hidden
        className={cn(
          "h-2 w-2 rounded-full",
          !sessionClosed && status.kind === "working"
            ? "bg-emerald-500"
            : status.kind === "idle" || status.kind === "ended"
              ? "bg-muted-foreground/50"
              : // Founded, never started: hollow, like every other
                // founded surface — a filled colour would claim a state
                // nobody has published.
                status.kind === "founded"
                ? "bg-transparent ring-1 ring-inset ring-muted-foreground/50"
                : // A lifecycle-signed "Disconnected" or "Needs attention"
                  // reads like the execution rail's own attention state, not
                  // like an unread status.
                  status.kind === "unknown" && status.attention
                  ? "bg-destructive"
                  : "bg-amber-500",
        )}
      />
      {compact ? (
        <span className="sr-only">{sessionClosed ? "Closed" : statusText}</span>
      ) : sessionClosed ? (
        "Closed"
      ) : (
        statusText
      )}
    </Badge>
  );
}

/**
 * The header's way back to the plan, shown only while the plan is not already
 * on screen.
 *
 * The plan's one persistent home is the dock rail above the composer,
 * collapsed to a single line. This control exists for the two cases where
 * that rail is not mounted: a narrow window, where the plan opens as a sheet,
 * and a wide one where the person dismissed the rail for this turn. While the
 * rail (or the sheet) is showing, a second "Plan" control would be the plan
 * appearing twice, so the header stands it down. The task count rides along
 * because it is the one live number the rail would otherwise have shown.
 */
export function CodingSessionHeaderPlanToggle({
  compact,
  onToggle,
  open,
  taskCount,
}: {
  compact: boolean;
  onToggle: () => void;
  open: boolean;
  taskCount: number;
}) {
  return (
    <Button
      aria-controls={CODING_SESSION_TASK_RAIL_ID}
      aria-expanded={open}
      aria-label={open ? "Hide session plan" : "Show session plan"}
      data-testid="coding-session-task-rail-toggle"
      onClick={onToggle}
      size={compact ? "icon" : "sm"}
      type="button"
      variant={open ? "secondary" : "ghost"}
    >
      <ListChecks />
      <span className={compact ? "sr-only" : undefined}>Plan</span>
      {taskCount > 0 ? (
        <>
          <span
            aria-hidden
            className={cn(
              "rounded-full bg-background/70 px-1.5 text-xs",
              compact && "sr-only",
            )}
          >
            {taskCount}
          </span>
          <span className="sr-only">{taskCount} tasks</span>
        </>
      ) : null}
    </Button>
  );
}

/**
 * Navigation the person toggles while reading — surface tabs and the Mission
 * route rail — in one segmented group. The tabs carry live counts (agents,
 * observed changes), which is why they stay in the row rather than in the
 * `⋯` menu: a count you have to open a menu to see does not signal anything.
 * People moved into the Details control (`CodingSessionHeaderDetails`), which
 * keeps its count on the trigger.
 */
export function CodingSessionHeaderDetailsGroup({
  compact,
  onToggleRouteRail,
  onToggleSurface,
  routeRailDisplacesInspector,
  routeRailExpanded,
  surfaceHostId,
  surfaceTabs,
}: {
  compact: boolean;
  onToggleRouteRail?: () => void;
  onToggleSurface?: (id: string) => void;
  routeRailDisplacesInspector: boolean;
  routeRailExpanded: boolean;
  surfaceHostId?: string;
  surfaceTabs?: readonly CodingSessionHeaderSurfaceTab[];
}) {
  const hasTabs = Boolean(onToggleSurface && surfaceTabs?.length);
  if (!hasTabs && !onToggleRouteRail) return null;
  return (
    <fieldset
      aria-label="Session surfaces"
      className="flex shrink-0 items-center gap-0.5 rounded-xl border border-border/55 bg-muted/20 p-0.5"
    >
      {onToggleSurface && surfaceTabs
        ? surfaceTabs.map((tab) => (
            <Button
              aria-controls={surfaceHostId}
              aria-expanded={tab.active}
              aria-label={
                tab.active
                  ? `Hide ${tab.label.toLowerCase()}`
                  : `Show ${tab.label.toLowerCase()}`
              }
              data-testid={`coding-session-surface-toggle-${tab.id}`}
              key={tab.id}
              onClick={() => onToggleSurface(tab.id)}
              size={compact ? "icon" : "sm"}
              type="button"
              variant={tab.active ? "secondary" : "ghost"}
            >
              {tab.icon === "agents" ? (
                <Users />
              ) : tab.icon === "inspector" ? (
                <PanelRight />
              ) : (
                <GitCompare />
              )}
              <span className={compact ? "sr-only" : undefined}>
                {tab.label}
              </span>
              {tab.count !== undefined && tab.count > 0 ? (
                <span
                  className={cn(
                    "rounded-full bg-background/70 px-1.5 text-xs",
                    compact && "sr-only",
                  )}
                >
                  {tab.count}
                </span>
              ) : null}
            </Button>
          ))
        : null}
      {onToggleRouteRail ? (
        <Button
          // F7: the IDREF has to land on something. Both the expanded rail
          // and the 40 px scrubber carry this id, because the control
          // governs whichever of the two is mounted.
          aria-controls={CODING_SESSION_ROUTE_RAIL_ID}
          aria-expanded={routeRailExpanded}
          aria-label={
            routeRailExpanded ? "Collapse route rail" : "Expand route rail"
          }
          aria-pressed={routeRailExpanded}
          data-testid="coding-session-route-toggle"
          onClick={onToggleRouteRail}
          size={compact ? "icon" : "sm"}
          title={
            routeRailExpanded
              ? "Collapse the route rail to its scrubber"
              : routeRailDisplacesInspector
                ? "Expand the route rail — closes the Inspector, which this width cannot hold beside it"
                : "Expand the route rail"
          }
          type="button"
          variant={routeRailExpanded ? "secondary" : "ghost"}
        >
          <PanelLeft />
          <span className={compact ? "sr-only" : undefined}>Route</span>
        </Button>
      ) : null}
    </fieldset>
  );
}
