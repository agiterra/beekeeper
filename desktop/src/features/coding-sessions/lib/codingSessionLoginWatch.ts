/**
 * Pure scheduling rules for the post-Connect login watch.
 *
 * After a Connect launches a runtime's own sign-in (browser or terminal),
 * nothing tells the app when the person finishes — the only signal is the
 * host runtimes probe flipping that runtime's `authState` to `ready`. The
 * watch re-runs the probe on a fixed cadence, bounded so an abandoned login
 * does not poll forever. Kept free of timers and React so the verdict logic
 * is testable as data-in/data-out.
 */

/** Milliseconds between re-probes while a login is pending. */
export const CODING_SESSION_LOGIN_WATCH_INTERVAL_MS = 4_000;

/** Give up polling this long after the login was launched. */
export const CODING_SESSION_LOGIN_WATCH_TIMEOUT_MS = 120_000;

export type CodingSessionLoginWatchVerdict = "ready" | "expired" | "continue";

/**
 * What a login watch should do after a probe.
 *
 * `ready` wins over `expired` when both hold: a login that completed on the
 * final tick is still a completed login. Matching is by runtime slug — a
 * probe can list several instances of one runtime, and any of them being
 * ready means the sign-in this watch was started for has landed.
 */
export function codingSessionLoginWatchVerdict(input: {
  /** Runtime slug the Connect targeted, e.g. "claude". */
  runtime: string;
  /** Epoch ms when the login was launched. */
  startedAt: number;
  /** Epoch ms now. */
  now: number;
  runtimes: ReadonlyArray<{ runtime: string; authState: string }>;
  timeoutMs?: number;
}): CodingSessionLoginWatchVerdict {
  const ready = input.runtimes.some(
    (candidate) =>
      candidate.runtime === input.runtime && candidate.authState === "ready",
  );
  if (ready) return "ready";
  const timeoutMs = input.timeoutMs ?? CODING_SESSION_LOGIN_WATCH_TIMEOUT_MS;
  if (input.now - input.startedAt >= timeoutMs) return "expired";
  return "continue";
}
