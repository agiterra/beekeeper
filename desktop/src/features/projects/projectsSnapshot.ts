/**
 * Last-known repository/project enumeration per relay+viewer, so the
 * sidebar's Repositories section paints on the first frame instead of
 * waiting out relay connect + auth + the multi-round enumeration
 * (announcements, then tombstones) in `projectEnumeration.ts`.
 *
 * Scoped by viewer pubkey as well as relay URL for the same reason the
 * project-container snapshot is (`projects-container/hooks.ts`): private
 * projects make the list identity-sensitive, and a device with several local
 * identities must never flash one identity's repositories while switched to
 * another.
 *
 * Registered in LOCAL_STORAGE_SWEEP_RULES (shared/lib/localStorageSweep.ts):
 * the payload carries a root `updatedAt` so stale relay+identity combinations
 * are swept instead of accreting forever.
 */
import type { Project } from "./projectModels";

const PROJECTS_SNAPSHOT_PREFIX = "buzz.projects.repos.v1:";

export function projectsSnapshotKey(
  relayUrl: string | undefined,
  viewerPubkey: string | undefined,
): string | undefined {
  if (!relayUrl || !viewerPubkey) return undefined;
  return `${PROJECTS_SNAPSHOT_PREFIX}${relayUrl}:${viewerPubkey}`;
}

export function readProjectsSnapshot(
  key: string | undefined,
): Project[] | undefined {
  if (!key) return undefined;
  try {
    const raw = globalThis.localStorage?.getItem(key);
    if (!raw) return undefined;
    const parsed: unknown = JSON.parse(raw);
    if (typeof parsed !== "object" || parsed === null) return undefined;
    const projects = (parsed as { projects?: unknown }).projects;
    return Array.isArray(projects) ? (projects as Project[]) : undefined;
  } catch {
    return undefined;
  }
}

export function writeProjectsSnapshot(
  key: string | undefined,
  projects: Project[],
): void {
  if (!key) return;
  try {
    // Root `updatedAt` is what LOCAL_STORAGE_SWEEP_RULES keys the TTL on.
    globalThis.localStorage?.setItem(
      key,
      JSON.stringify({ updatedAt: Date.now(), projects }),
    );
  } catch {
    // Best-effort cache — a quota or serialization failure only costs the
    // fast first paint, never the live data.
  }
}
