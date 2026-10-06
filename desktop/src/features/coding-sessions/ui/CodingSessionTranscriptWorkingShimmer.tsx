import * as React from "react";
import { useReducedMotion } from "motion/react";

import {
  codingSessionLiveShimmerActive,
  codingSessionMsUntilQuiet,
} from "@/features/coding-sessions/lib/codingSessionWaitingLiveness";
import { Shimmer } from "@/shared/ui/Shimmer";

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
 * `text` with the shared {@link Shimmer} while `active`, plain otherwise.
 * `data-live-shimmer` says which, for tests and the E2E spec.
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
    <span className={className} data-live-shimmer="on">
      {/* Bounded, so a long command still truncates inside its row. */}
      <Shimmer className="max-w-full truncate align-bottom">{text}</Shimmer>
    </span>
  );
}
