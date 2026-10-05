/**
 * "Since you last looked", on this device (SV-22, DB6).
 *
 * Diff's neutral count compares a session's edits against a marker this
 * device keeps in localStorage — the last time Diff was on screen here, or,
 * before that, when this device first showed the session. It is a local read
 * state and says so: every sentence built on it ends "on this device".
 *
 * Keyed by relay, channel and session, so two communities, two channels or
 * two sessions never share a marker. Persistent storage rather than a module
 * cache: nothing here outlives a community switch in memory, so
 * `resetCommunityState()` has nothing to reset.
 *
 * Writes announce themselves on a window event so every Diff badge on screen
 * (launcher row, tab, header) re-reads at once; the browser's own `storage`
 * event covers other windows.
 */
import type { CodingSessionDiffSeenMarker } from "./codingSessionSurfaceBadgeModel";

// v2: markers record every file's newest edit, the baseline included. A v1
// baseline recorded none and was read against a clock, so under the v2 rule
// it would count every old file; v1 markers are left unread and the session
// re-baselines.
const KEY_PREFIX = "buzz.coding-session.surface-seen.v2";

/** The window event a marker write dispatches; `detail` is the key. */
export const CODING_SESSION_SURFACE_SEEN_EVENT =
  "buzz:coding-session-surface-seen";

/** The localStorage key for one surface's marker in one session. */
export function codingSessionSurfaceSeenKey(input: {
  relayUrl: string;
  channelId: string;
  sessionKey: string;
  surfaceId: string;
}): string {
  return [
    KEY_PREFIX,
    input.surfaceId,
    encodeURIComponent(input.relayUrl),
    encodeURIComponent(input.channelId),
    encodeURIComponent(input.sessionKey),
  ].join(":");
}

/** The slice of `Storage` this module uses, so tests can pass a fake. */
export type CodingSessionSeenStorage = Pick<Storage, "getItem" | "setItem">;

/** `window.localStorage`, or null where there is none or it throws. */
export function codingSessionSeenStorage(): CodingSessionSeenStorage | null {
  try {
    return typeof window === "undefined" ? null : window.localStorage;
  } catch {
    return null;
  }
}

/** The raw stored string, or null. Stable between writes, for a snapshot. */
export function readCodingSessionSurfaceSeenRaw(
  storage: CodingSessionSeenStorage | null,
  key: string,
): string | null {
  if (storage === null) return null;
  try {
    return storage.getItem(key);
  } catch {
    return null;
  }
}

/**
 * Parse a stored marker. Anything malformed reads as no marker, which counts
 * nothing: a marker this build cannot read must not invent unseen files.
 */
export function parseCodingSessionDiffSeenMarker(
  raw: string | null,
): CodingSessionDiffSeenMarker | null {
  if (raw === null) return null;
  let value: unknown;
  try {
    value = JSON.parse(raw);
  } catch {
    return null;
  }
  if (typeof value !== "object" || value === null) return null;
  const { via, atMs, seen } = value as Record<string, unknown>;
  if (via !== "opened" && via !== "baseline") return null;
  if (typeof atMs !== "number" || !Number.isFinite(atMs)) return null;
  if (typeof seen !== "object" || seen === null || Array.isArray(seen)) {
    return null;
  }
  const entries = Object.entries(seen as Record<string, unknown>);
  if (entries.some(([, editId]) => typeof editId !== "string")) return null;
  return {
    via,
    atMs,
    seen: Object.fromEntries(entries) as Record<string, string>,
  };
}

/**
 * Store a marker and tell this window's badges. A storage that refuses
 * (quota, private mode) leaves the old marker in place and returns false.
 */
export function writeCodingSessionDiffSeenMarker(
  storage: CodingSessionSeenStorage | null,
  key: string,
  marker: CodingSessionDiffSeenMarker,
): boolean {
  if (storage === null) return false;
  try {
    storage.setItem(key, JSON.stringify(marker));
  } catch {
    return false;
  }
  if (typeof window !== "undefined") {
    window.dispatchEvent(
      new CustomEvent(CODING_SESSION_SURFACE_SEEN_EVENT, { detail: key }),
    );
  }
  return true;
}

/** Call `onChange` whenever any marker may have changed; returns unsubscribe. */
export function subscribeCodingSessionSurfaceSeen(
  onChange: () => void,
): () => void {
  if (typeof window === "undefined") return () => {};
  window.addEventListener(CODING_SESSION_SURFACE_SEEN_EVENT, onChange);
  window.addEventListener("storage", onChange);
  return () => {
    window.removeEventListener(CODING_SESSION_SURFACE_SEEN_EVENT, onChange);
    window.removeEventListener("storage", onChange);
  };
}
