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
import type { PulseMissionRowsResponse } from "./pulseMissionWire";
import { resetProjectPulseNameOrigins } from "./pulseNameOrigins.ts";

const lastGoodDigests = new Map<string, ProjectPulseDigest>();
const lastGoodMissionRows = new Map<string, PulseMissionRowsResponse>();

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
 * Remember a mission read that lost nothing.
 *
 * Same discipline as the digest above, and for the same reason: `missionErrors`
 * is a read that did not see everything, and banking it would keep a project
 * looking quieter than it is long after the source recovered. A seat that
 * stopped reporting and a seat this read could not see must never share a
 * cache entry.
 */
export function rememberPulseMissionRows(
  coordinate: string,
  rows: PulseMissionRowsResponse,
): void {
  if (rows.missionErrors.length > 0) return;
  lastGoodMissionRows.set(coordinate, rows);
}

/** The last complete mission read for this coordinate, if any. */
export function readPulseMissionRows(
  coordinate: string,
): PulseMissionRowsResponse | null {
  return lastGoodMissionRows.get(coordinate) ?? null;
}

/**
 * Drop every folded Pulse digest.
 *
 * Called from `resetCommunityState()` on a community switch; also exported for
 * tests, which must be able to prove the cache is empty afterwards.
 */
export function resetProjectPulseState(): void {
  lastGoodDigests.clear();
  lastGoodMissionRows.clear();
  resetProjectPulseNameOrigins();
}
