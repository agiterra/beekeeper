/**
 * Persisted head start for the sidebar's session shelf.
 *
 * The trusted ingress is fact-driven and in-memory only, so every boot the
 * Active/Recent Sessions sections waited out relay connect + auth + a history
 * REQ before showing anything. This cache stores the bounded raw-event set
 * `TrustedCodingSessionIngressStore.retainedShelfEvents()` selects (newest
 * accepted 44223 metadata per session — no transcripts) so the next boot can
 * seed the store immediately.
 *
 * Trust model: this is a cache of *signed bytes*, never of projections. A
 * rehydrating store runs every cached event through the full classifier —
 * signature, authority, channel scope — exactly like the pop-out bootstrap
 * seam, so a tampered or stale cache can only fail verification, not inject
 * rows. Keyed per relay+viewer for the same identity-sensitivity reason as
 * the other first-paint snapshots.
 *
 * Registered in LOCAL_STORAGE_SWEEP_RULES (shared/lib/localStorageSweep.ts):
 * the payload carries a root `updatedAt` so stale relay+identity combinations
 * are swept instead of accreting forever.
 */
import type { RelayEvent } from "@/shared/api/types";

const SHELF_CACHE_PREFIX = "buzz.codingSessions.shelf.v1:";

/** Hard cap on cached events — one metadata event per session is small, but a
 * pathological relay must not be able to grow this without bound. */
export const MAX_CODING_SESSION_SHELF_CACHE_EVENTS = 500;

export function codingSessionShelfCacheKey(
  relayUrl: string | undefined,
  viewerPubkey: string | undefined,
): string | undefined {
  if (!relayUrl || !viewerPubkey) return undefined;
  return `${SHELF_CACHE_PREFIX}${relayUrl}:${viewerPubkey}`;
}

export function readCodingSessionShelfCache(
  key: string | undefined,
): RelayEvent[] {
  if (!key) return [];
  try {
    const raw = globalThis.localStorage?.getItem(key);
    if (!raw) return [];
    const parsed: unknown = JSON.parse(raw);
    if (typeof parsed !== "object" || parsed === null) return [];
    const events = (parsed as { events?: unknown }).events;
    if (!Array.isArray(events)) return [];
    return events
      .filter(
        (event): event is RelayEvent =>
          typeof event === "object" &&
          event !== null &&
          typeof (event as { id?: unknown }).id === "string" &&
          typeof (event as { sig?: unknown }).sig === "string",
      )
      .slice(0, MAX_CODING_SESSION_SHELF_CACHE_EVENTS);
  } catch {
    return [];
  }
}

export function writeCodingSessionShelfCache(
  key: string | undefined,
  events: readonly RelayEvent[],
): void {
  if (!key) return;
  try {
    // Newest last per retainedShelfEvents' ordering; cap keeps the newest.
    const bounded = events.slice(-MAX_CODING_SESSION_SHELF_CACHE_EVENTS);
    // Root `updatedAt` is what LOCAL_STORAGE_SWEEP_RULES keys the TTL on.
    globalThis.localStorage?.setItem(
      key,
      JSON.stringify({ updatedAt: Date.now(), events: bounded }),
    );
  } catch {
    // Best-effort cache — a quota failure only costs the fast first paint.
  }
}

/** Keep only cached events whose `h` tag names a channel in scope, so a
 * changed channel set never routes out-of-scope events into the classifier's
 * malformed diagnostics. */
export function filterShelfCacheEventsToChannels(
  events: readonly RelayEvent[],
  channelIds: readonly string[],
): RelayEvent[] {
  const allowed = new Set(channelIds);
  return events.filter((event) =>
    (event.tags ?? []).some(
      (tag) =>
        Array.isArray(tag) && tag[0] === "h" && allowed.has(tag[1] ?? ""),
    ),
  );
}
