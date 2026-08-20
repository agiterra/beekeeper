/**
 * Last-good folded digest per project coordinate, so a remount (or a return
 * to the project home) paints the Pulse it already proved rather than an empty
 * card it has not.
 *
 * This is a **community-scoped** cache: project coordinates are
 * `30621:<owner>:<dtag>` and nothing in that string names a relay, so the same
 * coordinate can exist in two communities with different members, different
 * claims, and different observed commits. `resetProjectPulseState()` is wired
 * into `resetCommunityState()` for exactly that reason — without it, one
 * community's claims and observed commits would paint under another
 * community's project, which is the dishonesty this feature exists to prevent.
 */
import type { ProjectPulseDigest } from "./pulseFold.ts";

const lastGoodDigests = new Map<string, ProjectPulseDigest>();

/** Remember a digest that came back complete. Partial reads are not banked. */
export function rememberProjectPulseDigest(
  coordinate: string,
  digest: ProjectPulseDigest,
): void {
  // Banking a partial read would let a one-off relay failure freeze a stale
  // "complete" answer into the surface long after the relay recovered.
  if (!digest.complete) return;
  lastGoodDigests.set(coordinate, digest);
}

/** The last complete digest folded for this coordinate, if any. */
export function readProjectPulseDigest(
  coordinate: string,
): ProjectPulseDigest | null {
  return lastGoodDigests.get(coordinate) ?? null;
}

/**
 * Drop every folded Pulse digest.
 *
 * Called from `resetCommunityState()` on a community switch; also exported for
 * tests, which must be able to prove the cache is empty afterwards.
 */
export function resetProjectPulseState(): void {
  lastGoodDigests.clear();
}
