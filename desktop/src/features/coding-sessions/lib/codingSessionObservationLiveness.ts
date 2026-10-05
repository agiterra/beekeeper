/**
 * Whether the view's kind-44246 read is being kept live (SV-41).
 *
 * The observations read is a snapshot; only the live subscription turns a
 * gate start the provider later closes into "ended" without anybody asking
 * again. When that subscription is not up, a "running" line is a snapshot of
 * when it was read, and every running-gate surface says so on the line itself
 * rather than going on reading as live.
 */

/**
 * The live subscription's state: `connecting` until it is up or has failed,
 * `subscribed` once the relay accepted it, `unavailable` when the client has
 * no live subscription or opening it failed.
 */
export type CodingSessionObservationLive =
  | "connecting"
  | "subscribed"
  | "unavailable";

/** The short note a running line carries, e.g. `not live — read at 14:02`. */
export type CodingSessionObservationNotLive = {
  /** Appended to the line: `not live — read at 14:02`. */
  readonly short: string;
  /** Appended to the line's title (hover), one sentence. */
  readonly sentence: string;
};

/**
 * The not-live note for a read, or null while the subscription is up.
 *
 * `readAtMs` is this machine's clock (when the answer on screen was read).
 */
export function codingSessionObservationNotLive(
  live: CodingSessionObservationLive,
  readAtMs: number | null,
  formatTime: (ms: number) => string,
): CodingSessionObservationNotLive | null {
  if (live === "subscribed") return null;
  const at = readAtMs === null ? null : formatTime(readAtMs);
  const why =
    live === "connecting"
      ? "the live subscription to new observations is not up yet"
      : "the live subscription to new observations is not available";
  return {
    short: at === null ? "not live" : `not live — read at ${at}`,
    sentence:
      at === null
        ? `Not live: ${why}, so this line is not being kept current.`
        : `Not live: ${why}, so this line is as read at ${at} (this computer's clock) and is not being kept current.`,
  };
}

/**
 * The not-live note for a surface `ctx.observations`, or null. A read that
 * never happened (`not-read`) carries no running gates, so no note.
 */
export function codingSessionObservationNotLiveOf(
  observations:
    | { readonly state: "not-read" }
    | {
        readonly state: "read";
        readonly live: CodingSessionObservationLive;
        readonly readAtMs: number | null;
      },
  formatTime: (ms: number) => string,
): CodingSessionObservationNotLive | null {
  if (observations.state !== "read") return null;
  return codingSessionObservationNotLive(
    observations.live,
    observations.readAtMs,
    formatTime,
  );
}
