import * as React from "react";

import { useCodingSessionGateStartClock } from "@/features/coding-sessions/hooks/useCodingSessionGateStartClock";
import { describeDuration } from "@/features/coding-sessions/lib/codingSessionObservationView";
import type { CodingSessionObservationNotLive } from "@/features/coding-sessions/lib/codingSessionObservationLiveness";
import type { CodingSessionObservationGateStartRow } from "@/features/coding-sessions/lib/codingSessionObservationWire";

/**
 * What one gate run reads as, in words.
 *
 * `ended` is a close the provider measured: it saw the call's result.
 * `unwatched` is a close it signed without a span (`durationMs: null`) because
 * it stopped watching — the turn ended, the call left its 32-call window, or
 * the session exited — so the command may still have been running.
 * `clock-backwards` is a close whose end the provider clamped to its start
 * because its clock read earlier at the close; nothing was measured either.
 */
export type CodingSessionGateRunState =
  | "ended"
  | "unwatched"
  | "clock-backwards"
  | "running"
  | "no-result";

/** A closed state: the provider will sign nothing more about this run. */
export function isCodingSessionGateRunClosed(
  state: CodingSessionGateRunState,
): boolean {
  return (
    state === "ended" || state === "unwatched" || state === "clock-backwards"
  );
}

/**
 * One signed gate run's state against `nowMs` (SV-41).
 *
 * A paired close is `ended` only when the provider measured a span; a
 * durationless close is `unwatched` (or `clock-backwards` when its end was
 * clamped to its start — a start is published only after the provider's
 * publish delay, so a stop-watching close always ends later than it began).
 * An open start is `running` until the core's stale threshold, then
 * `no-result` — never "failed", because the provider signed nothing that says
 * so. A start dated after now is not stale (the core's `gate_start_is_stale`).
 */
export function codingSessionGateRunState(
  start: CodingSessionObservationGateStartRow,
  staleAfterMs: number,
  nowMs: number,
): CodingSessionGateRunState {
  if (start.closeEventId !== null) {
    if (start.durationMs !== null) return "ended";
    return start.endedAtMs !== null && start.endedAtMs <= start.startedAtMs
      ? "clock-backwards"
      : "unwatched";
  }
  return nowMs >= start.startedAtMs + staleAfterMs ? "no-result" : "running";
}

/** The run's line, e.g. `cargo test · started 14:02 · ended 14:05 (3m)`. */
export function describeCodingSessionGateRun(
  start: CodingSessionObservationGateStartRow,
  state: CodingSessionGateRunState,
  formatTime: (ms: number) => string = formatClock,
): string {
  const started = formatTime(start.startedAtMs);
  if (state === "running") return `${start.gate} · running since ${started}`;
  if (state === "no-result") {
    return `${start.gate} · started ${started} · no result observed`;
  }
  if (state === "clock-backwards") {
    return `${start.gate} · started ${started} · end not observed (the provider's clock went backwards)`;
  }
  if (state === "unwatched") {
    const stopped =
      start.endedAtMs === null
        ? "stopped watching"
        : `stopped watching ${formatTime(start.endedAtMs)}`;
    return `${start.gate} · started ${started} · ${stopped} · end not observed`;
  }
  const ended =
    start.endedAtMs === null ? "ended" : `ended ${formatTime(start.endedAtMs)}`;
  const span = describeDuration(start.durationMs);
  return `${start.gate} · started ${started} · ${ended}${
    span === null ? "" : ` (${span})`
  }`;
}

/**
 * The run's hover sentence. `hasGateRow` is whether the same block lists a
 * gate row for this gate: the outcome is pointed at only when one exists.
 */
export function describeCodingSessionGateRunTitle(
  state: CodingSessionGateRunState,
  staleAfterMs: number,
  hasGateRow: boolean,
): string {
  if (state === "ended") {
    return `The provider signed that this gate started and that it ended, and measured the span. Times are the provider's clock. ${
      hasGateRow
        ? "The outcome is the gate row above, not this line."
        : "No gate row for this gate is in this block, so no outcome is shown."
    }`;
  }
  if (state === "unwatched" || state === "clock-backwards") {
    const why =
      state === "unwatched"
        ? "The provider signed that this gate started, then stopped watching it before it saw it end: the turn ended, the call left the provider's 32-call window, or the session exited."
        : "The provider signed that this gate started and closed it, but its clock read earlier at the close than at the start, so the end time was clamped to the start.";
    return `${why} No span was measured, and the command may still have been running. Times are the provider's clock.${
      hasGateRow
        ? " If the command finished later, its result is a gate row above."
        : ""
    }`;
  }
  if (state === "running") {
    return "The provider signed that this gate started and has not signed its end. Times are the provider's clock. A provider restart is not observed: this line can outlive the process until it goes stale.";
  }
  return `The provider signed that this gate started and has signed nothing since. After ${
    describeDuration(staleAfterMs) ?? "the stale threshold"
  } that reads as no result rather than running.`;
}

/**
 * Under a watcher's gate rows: every gate run it signed as started — ended,
 * no longer watched, running, or with no result observed. These left the phase timings when
 * SV-41 routed `gate:` phases out of `phases`, and stay one click away here.
 *
 * Each line names its start event (copyable), so a reader can find it on the
 * wire. Nothing here is coloured as a failure: a run with no close is not a
 * failed gate.
 */
export function CodingSessionGateStartRows({
  gateRows = EMPTY_GATE_ROWS,
  notLive = null,
  starts,
  staleAfterMs,
}: {
  /** The same block's gate rows: a line points at an outcome only if listed. */
  gateRows?: readonly { readonly gate: string }[];
  /** Set while the read is not kept live: an open run says so on its line. */
  notLive?: CodingSessionObservationNotLive | null;
  starts: readonly CodingSessionObservationGateStartRow[];
  staleAfterMs: number | null;
}) {
  const nowMs = useCodingSessionGateStartClock(starts, staleAfterMs);
  if (starts.length === 0 || staleAfterMs === null) return null;
  return (
    <ul
      aria-label="Signed gate runs"
      className="mt-1.5 space-y-1"
      data-testid="coding-session-gate-starts"
    >
      {starts.map((start) => {
        const state = codingSessionGateRunState(start, staleAfterMs, nowMs);
        // A closed run is settled; an open one read from a snapshot says so.
        const snapshot = isCodingSessionGateRunClosed(state) ? null : notLive;
        const hasGateRow = gateRows.some((row) => row.gate === start.gate);
        return (
          <li
            className="flex items-baseline justify-between gap-2 text-2xs"
            data-gate-run-state={state}
            data-testid="coding-session-gate-start-row"
            key={start.eventId}
            title={`${describeCodingSessionGateRunTitle(
              state,
              staleAfterMs,
              hasGateRow,
            )}${snapshot === null ? "" : ` ${snapshot.sentence}`}`}
          >
            <span
              className={
                state === "running"
                  ? "min-w-0 truncate text-foreground"
                  : "min-w-0 truncate text-muted-foreground"
              }
            >
              {describeCodingSessionGateRun(start, state)}
              {snapshot === null ? null : ` · ${snapshot.short}`}
            </span>
            <StartEventId eventId={start.eventId} />
          </li>
        );
      })}
    </ul>
  );
}

function StartEventId({ eventId }: { eventId: string }) {
  const [copied, setCopied] = React.useState(false);
  return (
    <button
      aria-label={`Copy start event id ${eventId}`}
      className="shrink-0 rounded-sm font-mono text-muted-foreground underline-offset-2 hover:underline focus-visible:ring-2 focus-visible:ring-ring focus-visible:outline-none"
      data-event-id={eventId}
      data-testid="coding-session-gate-start-event"
      onClick={() => {
        void navigator.clipboard?.writeText(eventId).then(
          () => setCopied(true),
          () => setCopied(false),
        );
      }}
      title={eventId}
      type="button"
    >
      {copied ? "copied" : `start ${eventId.slice(0, 8)}`}
    </button>
  );
}

const EMPTY_GATE_ROWS: readonly { readonly gate: string }[] = Object.freeze([]);

function formatClock(ms: number): string {
  return new Date(ms).toLocaleTimeString([], {
    hour: "2-digit",
    minute: "2-digit",
  });
}
