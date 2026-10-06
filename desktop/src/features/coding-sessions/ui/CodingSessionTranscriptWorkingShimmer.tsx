import * as React from "react";
import { useReducedMotion } from "motion/react";

import {
  codingSessionLiveShimmerActive,
  codingSessionMsUntilQuiet,
} from "@/features/coding-sessions/lib/codingSessionWaitingLiveness";
import { cn } from "@/shared/lib/cn";

/**
 * Whether live action text may shimmer now (SV-104): the item is live, the
 * provider published within the quiet threshold, and motion is not reduced.
 *
 * Re-renders once, exactly when a fresh transcript goes quiet, so the shimmer
 * stops on time without a render per second; a new event moves `lastEventAt`
 * and re-arms it.
 */
export function useCodingSessionLiveShimmer(
  live: boolean,
  lastEventAt: number | null | undefined,
): boolean {
  const reducedMotion = useReducedMotion() ?? false;
  const [now, setNow] = React.useState(() => Date.now());
  // A new event (or a turn going live) is judged against the time it arrived,
  // not the time this row first mounted.
  // biome-ignore lint/correctness/useExhaustiveDependencies: re-read the clock when the evidence moves
  React.useEffect(() => {
    setNow(Date.now());
  }, [lastEventAt, live]);
  const delay = live ? codingSessionMsUntilQuiet(lastEventAt, now) : null;
  React.useEffect(() => {
    if (delay === null) return;
    const timer = window.setTimeout(() => setNow(Date.now()), delay);
    return () => window.clearTimeout(timer);
  }, [delay]);
  return codingSessionLiveShimmerActive({
    live,
    lastEventAt,
    now,
    reducedMotion,
  });
}

/**
 * The lit copy of live action text that sweeps across it (SV-104): T3 Code's
 * `ActivityShimmerOverlay`, on the `coding-session-live-activity-*` classes
 * (`coding-session.css`) the live Thinking row already uses.
 *
 * Not the shared channel `Shimmer`: that one pulses a static band blended
 * 60% toward the background, which on the dark theme is a dimmer patch over
 * muted text that never moves — on a live session it read as no motion at
 * all (2026-10-06). This one moves a band of full foreground colour across
 * the muted label, on transform alone, so it stays on the compositor.
 *
 * The parent must be `relative overflow-hidden`. The innermost span carries
 * `coding-session-live-shimmer-copy`, so a label rewritten outside React (the
 * working timer) can rewrite its copy too. Visual only: aria-hidden.
 */
export function CodingSessionLiveShimmerOverlay({ text }: { text: string }) {
  return (
    <span
      aria-hidden="true"
      className="coding-session-live-activity-focus pointer-events-none absolute inset-y-0 select-none"
      data-testid="coding-session-live-shimmer-overlay"
    >
      <span className="coding-session-live-activity-counter block">
        <span className="coding-session-live-activity-aligned coding-session-live-shimmer-copy block truncate text-foreground">
          {text}
        </span>
      </span>
    </span>
  );
}

/**
 * `text` with {@link CodingSessionLiveShimmerOverlay} while `active`, plain
 * otherwise. `data-live-shimmer` says which, for tests and the E2E spec.
 */
export function CodingSessionLiveShimmerText({
  active,
  className,
  text,
}: {
  active: boolean;
  className?: string;
  text: string;
}) {
  if (!active) {
    return (
      <span className={className} data-live-shimmer="off">
        {text}
      </span>
    );
  }
  return (
    <span
      // Bounded, so a long command still truncates inside its row.
      className={cn(
        "relative inline-block max-w-full overflow-hidden truncate whitespace-nowrap align-bottom",
        className,
      )}
      data-live-shimmer="on"
    >
      {text}
      <CodingSessionLiveShimmerOverlay text={text} />
    </span>
  );
}
