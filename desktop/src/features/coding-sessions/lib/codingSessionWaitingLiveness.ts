/**
 * When live action text may move (SV-104), and how long a provider has been
 * silent.
 *
 * Motion is a claim: a shimmering "Thinking" or "Run cargo test" says the
 * work is happening now. So it is earned only by three things at once — the
 * item is live (its turn is open and the provider still calls it running),
 * the provider published a transcript event within
 * {@link CODING_SESSION_QUIET_AFTER_MS}, and the person has not asked the
 * system to reduce motion. A provider silent past the threshold gets the
 * working line's "no update for Nm" instead, and an unknown last event time
 * is no evidence of freshness: nothing moves on it.
 *
 * Pure functions only; the hooks that re-render at the flip live in
 * `ui/CodingSessionTranscriptWorkingShimmer.tsx`.
 */

/**
 * How long a working session may publish nothing before the working line says
 * so. Past this, "Working for 5m" over a provider that has been silent for
 * four of them is a comfortable guess, not a fact (2026-10-05).
 */
export const CODING_SESSION_QUIET_AFTER_MS = 60_000;

/**
 * How long the provider has been silent at `now`, floored to whole minutes,
 * or `null` while it is not past {@link CODING_SESSION_QUIET_AFTER_MS} — or
 * when no last event time is known, which is no reason to claim silence.
 */
export function codingSessionQuietMs(
  lastEventAt: number | null,
  now: number,
): number | null {
  if (lastEventAt === null || !Number.isFinite(lastEventAt)) return null;
  const quiet = now - lastEventAt;
  if (quiet <= CODING_SESSION_QUIET_AFTER_MS) return null;
  return Math.floor(quiet / 60_000) * 60_000;
}

/**
 * Whether the provider has published within the quiet threshold at `now`.
 * Unlike {@link codingSessionQuietMs} an unknown time is *not* fresh: silence
 * is never claimed without a time, and neither is liveness.
 */
export function isCodingSessionTranscriptFresh(
  lastEventAt: number | null | undefined,
  now: number,
): boolean {
  if (lastEventAt === null || lastEventAt === undefined) return false;
  if (!Number.isFinite(lastEventAt)) return false;
  return now - lastEventAt <= CODING_SESSION_QUIET_AFTER_MS;
}

/**
 * Milliseconds from `now` until a fresh transcript goes quiet, or `null` when
 * it is already quiet or its time is unknown — what a one-shot timer waits so
 * the shimmer stops on time without a render per second.
 */
export function codingSessionMsUntilQuiet(
  lastEventAt: number | null | undefined,
  now: number,
): number | null {
  if (!isCodingSessionTranscriptFresh(lastEventAt, now)) return null;
  return (lastEventAt as number) + CODING_SESSION_QUIET_AFTER_MS + 1 - now;
}

/**
 * The shimmer rule for live action text: on only while the item is live,
 * the provider is fresh, and motion is not reduced. Everything else — a
 * settled or unknown turn, a quiet provider, an unknown last event time,
 * reduced motion — is plain text.
 */
export function codingSessionLiveShimmerActive(input: {
  live: boolean;
  lastEventAt: number | null | undefined;
  now: number;
  reducedMotion: boolean;
}): boolean {
  if (!input.live || input.reducedMotion) return false;
  return isCodingSessionTranscriptFresh(input.lastEventAt, input.now);
}
