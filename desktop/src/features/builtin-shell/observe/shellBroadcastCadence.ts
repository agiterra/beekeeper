import type { RelayEvent } from "@/shared/api/types";
import { WRITE_RESERVE } from "@/shared/api/relaySendBudget";

/**
 * Pacing facts for a shared terminal (NIP-ST), stated honestly on both ends.
 *
 * Every frame the owner streams is an EVENT charged to the owner's per-key
 * relay quota (60/min, shared with their chat, typing and presence). The
 * broadcaster therefore emits at most one content frame per second and at
 * most {@link SHELL_BROADCAST_CAP_PER_MINUTE} frames per rolling minute, and
 * doubles its interval (to at most {@link SHELL_BROADCAST_MAX_INTERVAL_MS})
 * when the relay answers `rate-limited:`. Each frame carries its current
 * interval as a `["cadence", "<ms>"]` tag so an observer can say the same
 * thing the owner's screen says.
 */

/** Baseline spacing between content frames, in milliseconds. */
export const SHELL_BROADCAST_BASE_INTERVAL_MS = 1_000;
/** Hard cap on frames per rolling minute (of the owner's 60 EVENT/min). */
export const SHELL_BROADCAST_CAP_PER_MINUTE = 40;
/** The back-off ceiling the broadcaster doubles up to. */
export const SHELL_BROADCAST_MAX_INTERVAL_MS = 4_000;

/** Interval in ms from a frame's `cadence` tag, or null when absent/invalid. */
export function cadenceTagMs(event: RelayEvent): number | null {
  const raw = event.tags.find((tag) => tag[0] === "cadence")?.[1];
  if (typeof raw !== "string") return null;
  const value = Number.parseInt(raw, 10);
  return Number.isFinite(value) && value > 0 ? value : null;
}

/**
 * The one-line status both the owner's screen and the observer show.
 *
 * At baseline: `≤1 frame/s, ≤40/min — the relay's per-key quota`. While the
 * broadcaster is backing off it says so and names the current spacing, so a
 * slower stream reads as the relay's quota, never as a dead terminal.
 */
export function shellBroadcastCadenceLabel(
  intervalMs: number | null,
  capPerMinute: number = SHELL_BROADCAST_CAP_PER_MINUTE,
): string {
  const quota = `≤${capPerMinute}/min — the relay's per-key quota`;
  if (intervalMs === null || intervalMs <= SHELL_BROADCAST_BASE_INTERVAL_MS) {
    return `≤1 frame/s, ${quota}`;
  }
  const seconds = intervalMs / 1_000;
  const spacing = Number.isInteger(seconds)
    ? String(seconds)
    : seconds.toFixed(1);
  return `≤1 frame/${spacing} s (backing off), ${quota}`;
}

/** Collaborator keystroke coalescing window at baseline. */
export const INPUT_COALESCE_MS = 30;
/** The wider window used while the write lane is below half its reserve. */
export const INPUT_COALESCE_PRESSURED_MS = 80;

/**
 * How long to coalesce a collaborator's keystrokes before sending one
 * kind:24312 event, given how many write frames the send budget could still
 * admit right now. Each input event is an EVENT against the same per-key
 * budget as the collaborator's own messages, so when fewer than two write
 * reserves are free the window widens from 30 ms to 80 ms — fewer, larger
 * events instead of one per keystroke burst.
 */
export function collaboratorInputCoalesceMs(availableWriteFrames: number) {
  return availableWriteFrames < WRITE_RESERVE * 2
    ? INPUT_COALESCE_PRESSURED_MS
    : INPUT_COALESCE_MS;
}
