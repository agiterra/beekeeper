import { ChevronRight } from "lucide-react";

import {
  summarizeCodingSessionMissionExecution,
  type CodingSessionMissionExecutionSummary,
} from "@/features/coding-sessions/lib/codingSessionMissionExecutionBundle";
import type { TranscriptItem } from "@/features/agents/ui/agentSessionTypes";
import { cn } from "@/shared/lib/cn";

/**
 * `▸ 11 execution events · Terminal 4 · Read 2 · Edit 3`.
 *
 * One disclosure row standing for a turn's signed tool items, per SURFACES C2.
 * The count is the reversibility contract: expanding reveals exactly `count`
 * rows, the existing per-item ones, unchanged. The repeated verb the design doc
 * drops ("Ran Terminal") never appears — the breakdown is the classifier's, and
 * a verb with no events is omitted rather than printed as zero.
 *
 * Mission-only. Conversation and Trace list the items as they always have.
 */
export function CodingSessionMissionExecutionBundle({
  children,
  expanded,
  items,
  onToggle,
}: {
  /** The expanded per-item rows, rendered by the caller's transcript. */
  children?: React.ReactNode;
  expanded: boolean;
  items: readonly TranscriptItem[];
  onToggle: () => void;
}) {
  const summary = summarizeCodingSessionMissionExecution(items);
  if (summary.count === 0) return null;
  return (
    <div className="mt-2" data-testid="coding-session-mission-execution-bundle">
      <button
        aria-expanded={expanded}
        className="flex w-full items-center gap-2 rounded-lg border border-border/45 bg-muted/20 px-2.5 py-1.5 text-left text-xs text-muted-foreground transition-colors hover:bg-muted/35 hover:text-foreground focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring motion-reduce:transition-none"
        data-count={summary.count}
        data-testid="coding-session-mission-execution-bundle-toggle"
        onClick={onToggle}
        type="button"
      >
        <ChevronRight
          aria-hidden
          className={cn(
            "size-3.5 shrink-0 transition-transform motion-reduce:transition-none",
            expanded && "rotate-90",
          )}
        />
        <span className="shrink-0 font-medium text-foreground/80">
          {summary.label}
        </span>
        <Breakdown summary={summary} />
      </button>
      {expanded ? children : null}
    </div>
  );
}

function Breakdown({
  summary,
}: {
  summary: CodingSessionMissionExecutionSummary;
}) {
  if (summary.breakdown.length === 0) return null;
  return (
    <span
      className="ml-auto min-w-0 truncate text-2xs text-muted-foreground"
      data-testid="coding-session-mission-execution-breakdown"
    >
      {summary.breakdown
        .map((entry) => `${entry.verb} ${entry.count}`)
        .join(" · ")}
    </span>
  );
}
