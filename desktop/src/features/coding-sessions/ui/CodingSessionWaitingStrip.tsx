import * as React from "react";
import { Info } from "lucide-react";
import { useReducedMotion } from "motion/react";

import { useCodingSessionRunningGates } from "@/features/coding-sessions/hooks/useCodingSessionGateStartClock";
import type { CodingSessionParticipantPresence } from "@/features/coding-sessions/lib/codingSessionStreamPresence";
import { codingSessionObservationNotLiveOf } from "@/features/coding-sessions/lib/codingSessionObservationLiveness";
import { formatCodingSessionDuration } from "@/features/coding-sessions/lib/codingSessionTranscriptModel";
import type {
  CodingSessionCatalogRecord,
  CodingSessionUmbrellaRecord,
  CodingSessionWorkspaceStatus,
} from "@/features/coding-sessions/lib/codingSessionTypes";
import {
  type CodingSessionWaiting,
  type CodingSessionWaitingExecution,
  codingSessionWaitingMsUntilQuiet,
  collectCodingSessionWaitingLines,
  describeCodingSessionWaitingLine,
  summarizeCodingSessionWaiting,
} from "@/features/coding-sessions/lib/codingSessionWaiting";
import { cn } from "@/shared/lib/cn";
import { Popover, PopoverContent, PopoverTrigger } from "@/shared/ui/popover";

import type { CodingSessionSurfaceObservations } from "./surfaces/codingSessionSurfaceContext";

/** How often a quiet strip re-reads the time for its "no update for Nm". */
const QUIET_TICK_MS = 60_000;

const NOT_READ: CodingSessionSurfaceObservations = {
  state: "not-read",
  reason: "The dock was not handed this session's observations.",
};

function formatClock(ms: number): string {
  return new Date(ms).toLocaleTimeString([], {
    hour: "2-digit",
    minute: "2-digit",
  });
}

/**
 * The status strip docked above the composer while the agent waits on its
 * own work (SV-99): a dot, "Waiting on N subagents" / "Waiting on a
 * background task" / "Gate running", one line of what, and an info button
 * listing every outstanding line with the evidence behind it.
 *
 * Derived only from live evidence (`deriveCodingSessionWaiting`); hidden
 * when nothing is outstanding. The dot pulses only while some line is fresh
 * and motion is not reduced; a quiet provider reads "no update for Nm".
 *
 * No Stop here: the composer below already carries the session's own Stop
 * while a turn runs, and this strip offers no action the session does not.
 */
export function CodingSessionUmbrellaWaitingStrip({
  observations,
  participants,
  umbrella,
}: {
  observations?: CodingSessionSurfaceObservations;
  participants: readonly CodingSessionParticipantPresence[];
  umbrella: CodingSessionUmbrellaRecord;
}) {
  const executions = React.useMemo<CodingSessionWaitingExecution[]>(() => {
    const byKey = new Map(
      participants.map((participant) => [
        participant.executionKey,
        participant,
      ]),
    );
    const list: CodingSessionWaitingExecution[] = [];
    for (const execution of umbrella.executions) {
      const participant = byKey.get(execution.executionKey);
      // No roster status, no vouching for anything it runs.
      if (!participant) continue;
      list.push({
        executionKey: execution.executionKey,
        label: participant.label,
        status: participant.status,
        generation: execution.activeGeneration,
      });
    }
    return list;
  }, [participants, umbrella.executions]);
  return (
    <CodingSessionWaitingStrip
      executions={executions}
      observations={observations}
    />
  );
}

/** The single-session workspace's strip: one execution, its own status. */
export function CodingSessionSingleWaitingStrip({
  observations,
  session,
  status,
}: {
  observations?: CodingSessionSurfaceObservations;
  session: CodingSessionCatalogRecord;
  /** The workspace's status after reachability demotion. */
  status: CodingSessionWorkspaceStatus;
}) {
  const executions = React.useMemo<CodingSessionWaitingExecution[]>(
    () => [
      {
        executionKey: session.generationId,
        label: session.label || "This session",
        status,
        generation: session,
      },
    ],
    [session, status],
  );
  return (
    <CodingSessionWaitingStrip
      executions={executions}
      observations={observations}
    />
  );
}

export function CodingSessionWaitingStrip({
  executions,
  observations = NOT_READ,
}: {
  executions: readonly CodingSessionWaitingExecution[];
  observations?: CodingSessionSurfaceObservations;
}) {
  const fold =
    observations.state === "read" ? (observations.result?.fold ?? null) : null;
  const runningGates = useCodingSessionRunningGates(
    fold,
    codingSessionObservationNotLiveOf(observations, formatClock),
  );
  const lines = React.useMemo(
    () => collectCodingSessionWaitingLines({ executions, runningGates }),
    [executions, runningGates],
  );

  const [nowMs, setNowMs] = React.useState(() => Date.now());
  // New evidence is judged against the time it arrived.
  React.useEffect(() => {
    if (lines.length > 0) setNowMs(Date.now());
  }, [lines]);
  const waiting = summarizeCodingSessionWaiting({ executions, lines, nowMs });
  const flipDelay = codingSessionWaitingMsUntilQuiet({
    executions,
    lines,
    nowMs,
  });
  React.useEffect(() => {
    if (flipDelay === null) return;
    const timer = window.setTimeout(() => setNowMs(Date.now()), flipDelay);
    return () => window.clearTimeout(timer);
  }, [flipDelay]);
  const quiet = waiting?.quietMs !== null && waiting?.quietMs !== undefined;
  React.useEffect(() => {
    if (!quiet) return;
    const timer = window.setInterval(() => setNowMs(Date.now()), QUIET_TICK_MS);
    return () => window.clearInterval(timer);
  }, [quiet]);

  const reducedMotion = useReducedMotion() ?? false;
  if (!waiting) return null;
  return (
    <CodingSessionWaitingStripView
      pulse={waiting.fresh && !reducedMotion}
      waiting={waiting}
    />
  );
}

/** The strip's markup for one reading; exported for tests. */
export function CodingSessionWaitingStripView({
  pulse,
  waiting,
}: {
  pulse: boolean;
  waiting: CodingSessionWaiting;
}) {
  const quiet =
    waiting.quietMs === null
      ? null
      : `no update for ${formatCodingSessionDuration(waiting.quietMs)}`;
  return (
    <div
      aria-label={waiting.headline}
      className="mx-6 flex h-9 min-w-0 items-center gap-2 rounded-t-2xl border border-b-0 border-border/70 bg-background px-4 text-sm shadow-sm"
      data-pulse={pulse ? "on" : "off"}
      data-quiet={quiet === null ? "false" : "true"}
      data-testid="coding-session-waiting-strip"
      role="status"
    >
      <span
        aria-hidden
        className={cn(
          "size-1.5 shrink-0 rounded-full",
          pulse
            ? "animate-pulse bg-foreground/70 motion-reduce:animate-none"
            : "bg-muted-foreground/50",
        )}
        data-testid="coding-session-waiting-strip-dot"
      />
      <span
        className="shrink-0 font-medium text-foreground"
        data-testid="coding-session-waiting-strip-headline"
      >
        {waiting.headline}
      </span>
      {waiting.brief ? (
        <span
          className="min-w-0 truncate text-muted-foreground"
          data-testid="coding-session-waiting-strip-brief"
          title={waiting.brief}
        >
          {waiting.brief}
        </span>
      ) : null}
      {quiet ? (
        <span
          className="shrink-0 text-xs text-muted-foreground tabular-nums"
          data-testid="coding-session-waiting-strip-quiet"
        >
          · {quiet}
        </span>
      ) : null}
      <Popover>
        <PopoverTrigger asChild>
          <button
            aria-label="What this session is waiting on"
            className="ml-auto inline-flex size-6 shrink-0 items-center justify-center rounded-md text-muted-foreground transition-colors hover:bg-muted hover:text-foreground focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring"
            data-testid="coding-session-waiting-strip-info"
            type="button"
          >
            <Info aria-hidden className="size-4" />
          </button>
        </PopoverTrigger>
        <PopoverContent
          align="end"
          className="w-80 p-3 text-sm"
          data-testid="coding-session-waiting-strip-details"
          side="top"
        >
          <ul className="space-y-2">
            {waiting.lines.map((line) => {
              const description = describeCodingSessionWaitingLine(line);
              return (
                <li data-waiting-kind={line.kind} key={line.key}>
                  <p className="wrap-break-word font-medium text-foreground">
                    {description.title}
                  </p>
                  <p className="text-xs text-muted-foreground">
                    {description.evidence}
                  </p>
                </li>
              );
            })}
          </ul>
          {quiet ? (
            <p className="mt-2 border-t border-border/50 pt-2 text-xs text-muted-foreground">
              The provider has published nothing for{" "}
              {formatCodingSessionDuration(waiting.quietMs ?? 0)}; this is the
              last thing it said, not proof the work is still going.
            </p>
          ) : null}
        </PopoverContent>
      </Popover>
    </div>
  );
}
