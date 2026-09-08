/**
 * Deterministic phase offsets for periodic relay polls.
 *
 * Every timer that fires "every 30 s" from the moment the app starts fires
 * together with every other 30 s timer — and, when two devices share one key,
 * together with the other device's. Offsetting each poll by a stable hash of
 * (identity, poll name) spreads them across the period so the relay sees a
 * trickle instead of a burst, and the same poll always lands in the same
 * phase across restarts, which makes traffic logs legible.
 */

/** 32-bit FNV-1a of a string (UTF-16 code units). Stable across platforms. */
export function fnv1a32(input: string): number {
  let hash = 0x811c9dc5;
  for (let i = 0; i < input.length; i++) {
    hash ^= input.charCodeAt(i);
    // Multiply by the FNV prime (16777619) in 32-bit arithmetic.
    hash = Math.imul(hash, 0x01000193) >>> 0;
  }
  return hash >>> 0;
}

/** Fraction in [0, 1) derived from the hash of `pubkey + key`. */
export function phaseFraction(key: string, pubkey = ""): number {
  return fnv1a32(`${pubkey}${key}`) / 0x100000000;
}

/**
 * Initial delay before the first tick of a poll: a stable point in
 * `[0, periodMs)` chosen by hashing `pubkey + key`. Feed it to the timer as
 * the first delay, then poll every {@link phaseJitteredPeriodMs}.
 */
export function phaseOffset(
  key: string,
  periodMs: number,
  pubkey = "",
): number {
  if (!(periodMs > 0)) return 0;
  return Math.floor(phaseFraction(key, pubkey) * periodMs);
}

/** Relative spread applied to a poll period by {@link phaseJitteredPeriodMs}. */
export const PHASE_JITTER_FRACTION = 0.1;

/**
 * A poll period nudged by up to ±10 % from the same hash, so two polls with
 * equal nominal periods drift apart instead of re-aligning every cycle.
 */
export function phaseJitteredPeriodMs(
  key: string,
  periodMs: number,
  pubkey = "",
): number {
  if (!(periodMs > 0)) return periodMs;
  const spread = (phaseFraction(key, pubkey) * 2 - 1) * PHASE_JITTER_FRACTION;
  return Math.round(periodMs * (1 + spread));
}
