import { cn } from "@/shared/lib/cn";

import type { AgentProgressLane } from "../lib/agentProgressFold";
import {
  agentLaneCoordinationText,
  agentLaneCoordinationTitle,
  agentLaneDotClass,
  agentLaneReportedText,
} from "../lib/agentProgressFormat";

/**
 * One session lane: dot, name, the two-axis status gutter, activity line.
 *
 * **Fixed height, always.** The row is `h-14` whether or not it has an
 * activity line, a runtime chip, an execution count or a Closed marker, so a
 * lane that starts streaming does not reflow the list under the reader's
 * cursor.
 *
 * **The gutter is two axes and shows both.** The left half is the coordination
 * word — `Reachable`, `Unverified`, `Closed` — which is the row's only claim
 * about liveness and comes only from a current lease. The right half is what
 * the provider last *reported*, phrased as a report on any row whose provider
 * is not proven reachable. Neither may stand in for the other, which is why
 * they are rendered as separate elements rather than one string.
 */
export function AgentProgressLaneRow({
  lane,
  onOpen,
}: {
  lane: AgentProgressLane;
  onOpen?: (lane: AgentProgressLane) => void;
}) {
  const openable = Boolean(onOpen && lane.openTarget);
  return (
    <li
      className="h-14 border-b border-border/40 last:border-b-0"
      data-lane-coordination={lane.coordination}
      data-lane-executions={lane.executionCount}
      data-testid="agent-progress-lane"
    >
      <button
        className="flex h-full w-full flex-col justify-center gap-0.5 px-3 text-left hover:bg-muted/40 disabled:cursor-default disabled:hover:bg-transparent"
        disabled={!openable}
        onClick={openable ? () => onOpen?.(lane) : undefined}
        title={
          openable
            ? undefined
            : "This session was proven by signed events, but this client holds no local transcript for it, so there is nowhere to open."
        }
        type="button"
      >
        <span className="flex items-center gap-2">
          <span
            aria-hidden
            className={cn(
              "size-1.5 shrink-0 rounded-full",
              agentLaneDotClass(lane.coordination),
            )}
          />
          <span className="truncate text-sm text-foreground">{lane.label}</span>
          {lane.executionCount > 1 ? (
            <span
              className="shrink-0 rounded bg-muted px-1 text-3xs uppercase tracking-wide text-muted-foreground"
              data-testid="agent-progress-lane-executions"
              title="Distinct provider executions nested inside this one durable session."
            >
              {lane.executionCount} exec
            </span>
          ) : null}
          <span
            className={cn(
              "ml-auto shrink-0 text-2xs",
              lane.coordination === "provider_reachable"
                ? "text-emerald-500"
                : "text-muted-foreground",
            )}
            data-testid="agent-progress-lane-coordination"
            title={agentLaneCoordinationTitle(lane)}
          >
            {agentLaneCoordinationText(lane)}
          </span>
          <span
            className="shrink-0 text-2xs text-muted-foreground"
            data-testid="agent-progress-lane-reported"
          >
            · {agentLaneReportedText(lane)}
          </span>
        </span>
        <span className="flex items-center gap-2 pl-3.5">
          <span
            className={cn(
              "truncate text-2xs",
              lane.activityIsError
                ? "text-destructive"
                : "text-muted-foreground",
            )}
            data-testid="agent-progress-lane-activity"
          >
            {lane.activity ?? "No activity in what this read returned"}
          </span>
          {lane.runtimeLabel ? (
            <span className="ml-auto shrink-0 text-2xs text-muted-foreground">
              {lane.runtimeLabel}
            </span>
          ) : null}
        </span>
      </button>
    </li>
  );
}
