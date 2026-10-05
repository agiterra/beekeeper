/**
 * The "now" a gate-start line is judged against (SV-41).
 *
 * An open start reads "running" until `buzz-core`'s stale threshold passes,
 * then "no result observed". That flip is a question about the time, not
 * about the wire, so nothing new arrives to re-render it: this clock re-reads
 * `Date.now()` once a minute for the "since" text, plus once more exactly when
 * the earliest open start crosses the stale threshold, so the flip to "no
 * result observed" lands on time rather than up to a minute late — and only
 * while some open start has not gone stale yet. With nothing running it holds
 * still and costs nothing.
 *
 * The live 44246 subscription in `useCodingSessionObservations` is what
 * clears a start when its close is signed; this clock only turns a start the
 * provider never closed into "no result observed" on time.
 */
import * as React from "react";

import {
  type CodingSessionRunningGate,
  selectCodingSessionRunningGates,
} from "@/features/coding-sessions/lib/codingSessionObservationView";
import type { CodingSessionObservationNotLive } from "@/features/coding-sessions/lib/codingSessionObservationLiveness";
import type {
  CodingSessionObservationFold,
  CodingSessionObservationGateStartRow,
} from "@/features/coding-sessions/lib/codingSessionObservationWire";

/** How often an open, not-yet-stale start re-reads the time. */
export const CODING_SESSION_GATE_START_CLOCK_MS = 60_000;

/** True when some start is open and still inside the stale threshold. */
export function hasLiveGateStart(
  starts: readonly CodingSessionObservationGateStartRow[],
  staleAfterMs: number | null,
  nowMs: number,
): boolean {
  if (staleAfterMs === null) return false;
  return starts.some(
    (start) =>
      start.closeEventId === null && nowMs < start.startedAtMs + staleAfterMs,
  );
}

/** The largest delay a browser timer honours (2^31 - 1 ms). */
const MAX_TIMER_DELAY_MS = 2_147_483_647;

/**
 * Milliseconds from `nowMs` until the earliest open, not-yet-stale start goes
 * stale, or null when no start can still flip. What the one-shot timer waits.
 */
export function nextGateStartStaleFlipDelayMs(
  starts: readonly CodingSessionObservationGateStartRow[],
  staleAfterMs: number | null,
  nowMs: number,
): number | null {
  if (staleAfterMs === null) return null;
  let earliest: number | null = null;
  for (const start of starts) {
    if (start.closeEventId !== null) continue;
    const staleAtMs = start.startedAtMs + staleAfterMs;
    if (nowMs >= staleAtMs) continue;
    if (earliest === null || staleAtMs < earliest) earliest = staleAtMs;
  }
  return earliest === null
    ? null
    : Math.min(earliest - nowMs, MAX_TIMER_DELAY_MS);
}

/**
 * `Date.now()`, re-read every minute while a start could still flip from
 * running to stale, and once at the earliest flip itself; frozen otherwise.
 */
export function useCodingSessionGateStartClock(
  starts: readonly CodingSessionObservationGateStartRow[],
  staleAfterMs: number | null,
): number {
  const [nowMs, setNowMs] = React.useState(() => Date.now());
  const live = hasLiveGateStart(starts, staleAfterMs, nowMs);
  React.useEffect(() => {
    if (!live) return;
    const timer = window.setInterval(
      () => setNowMs(Date.now()),
      CODING_SESSION_GATE_START_CLOCK_MS,
    );
    return () => window.clearInterval(timer);
  }, [live]);
  // The minute tick alone would leave a start reading "running" for up to a
  // minute past the stale rule; this fires at the flip. Re-armed on every
  // new `nowMs`, so a timer that fires a hair early simply re-arms.
  const flipDelayMs = nextGateStartStaleFlipDelayMs(
    starts,
    staleAfterMs,
    nowMs,
  );
  React.useEffect(() => {
    if (flipDelayMs === null) return;
    const timer = window.setTimeout(() => setNowMs(Date.now()), flipDelayMs);
    return () => window.clearTimeout(timer);
  }, [flipDelayMs]);
  // A new read can carry a start the frozen clock predates; re-read once when
  // the set of starts changes so its first render is judged against now.
  React.useEffect(() => {
    if (starts.length > 0) setNowMs(Date.now());
  }, [starts]);
  return nowMs;
}

/**
 * Every open gate start in `fold`, judged against a clock that ticks once a
 * minute while one could still go stale. What Landing and its badge read.
 * `notLive` (the read is a snapshot) rides on every gate so each surface can
 * say "not live — read at HH:MM" on the line.
 */
export function useCodingSessionRunningGates(
  fold: CodingSessionObservationFold | null,
  notLive: CodingSessionObservationNotLive | null = null,
): readonly CodingSessionRunningGate[] {
  const starts = fold?.gateStarts ?? EMPTY_STARTS;
  const nowMs = useCodingSessionGateStartClock(
    starts,
    fold?.gateStartStaleAfterMs ?? null,
  );
  // Keyed on the note's text: callers derive it inline each render.
  const notLiveShort = notLive?.short ?? null;
  const notLiveSentence = notLive?.sentence ?? null;
  return React.useMemo(
    () =>
      selectCodingSessionRunningGates(fold, {
        nowMs,
        notLive:
          notLiveShort === null || notLiveSentence === null
            ? null
            : { short: notLiveShort, sentence: notLiveSentence },
      }),
    [fold, notLiveSentence, notLiveShort, nowMs],
  );
}

const EMPTY_STARTS: readonly CodingSessionObservationGateStartRow[] =
  Object.freeze([]);
