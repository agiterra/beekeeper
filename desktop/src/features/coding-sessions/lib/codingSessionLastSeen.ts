/**
 * "Since you were here", per device (SV-27).
 *
 * The relay keeps no read receipts, so this is a fact about **this device**
 * only, and the minimap's rule says so ("on this device"). The marker is the
 * id of the newest prompt this device has shown on screen, kept in
 * localStorage under a key that carries the community (relay URL), the
 * channel and the session, so nothing here is a module cache and nothing
 * needs adding to `resetCommunityState()`.
 *
 * The rule is drawn after the turn the marker names, only when newer turns
 * exist. No marker (a first visit) and a marker that names no turn in the
 * transcript (history this view does not hold) both draw no rule: neither
 * says where this device stopped reading.
 */

export const CODING_SESSION_LAST_SEEN_KEY_PREFIX =
  "beekeeper:session-last-seen:v1:";

/** The label the rule carries. */
export const CODING_SESSION_LAST_SEEN_LABEL = "on this device";

export type CodingSessionLastSeenScope = {
  relayUrl: string;
  channelId: string;
  sessionKey: string;
};

type StorageLike = Pick<Storage, "getItem" | "setItem">;

export function codingSessionLastSeenStorageKey(
  scope: CodingSessionLastSeenScope,
): string {
  return `${CODING_SESSION_LAST_SEEN_KEY_PREFIX}${scope.relayUrl}:${scope.channelId}:${scope.sessionKey}`;
}

function defaultStorage(): StorageLike | null {
  try {
    return typeof window === "undefined" ? null : window.localStorage;
  } catch {
    return null;
  }
}

/** The stored prompt id, or null when none (or storage is unreadable). */
export function readCodingSessionLastSeen(
  scope: CodingSessionLastSeenScope,
  storage: StorageLike | null = defaultStorage(),
): string | null {
  if (storage === null) return null;
  try {
    const raw = storage.getItem(codingSessionLastSeenStorageKey(scope));
    if (raw === null) return null;
    const parsed: unknown = JSON.parse(raw);
    if (
      typeof parsed === "object" &&
      parsed !== null &&
      typeof (parsed as { itemId?: unknown }).itemId === "string"
    ) {
      return (parsed as { itemId: string }).itemId;
    }
    return null;
  } catch {
    return null;
  }
}

/** Record `itemId` as the newest prompt shown here. Best effort. */
export function writeCodingSessionLastSeen(
  scope: CodingSessionLastSeenScope,
  itemId: string,
  storage: StorageLike | null = defaultStorage(),
  nowMs: number = Date.now(),
): void {
  if (storage === null) return;
  try {
    storage.setItem(
      codingSessionLastSeenStorageKey(scope),
      JSON.stringify({ itemId, at: nowMs }),
    );
  } catch {
    // A full or refused store leaves the previous marker; the rule is a
    // convenience, and a write failure must never break reading.
  }
}

/**
 * Index of the item the rule follows, or null when no rule is drawn: no
 * marker, a marker this transcript does not hold, or nothing newer.
 */
export function resolveCodingSessionLastSeenIndex(
  itemIds: readonly string[],
  lastSeenId: string | null,
): number | null {
  if (lastSeenId === null) return null;
  const index = itemIds.indexOf(lastSeenId);
  if (index < 0 || index >= itemIds.length - 1) return null;
  return index;
}

/**
 * The marker after this view showed `inViewIds`: the newest of the stored
 * marker and what is on screen now, never moving backwards. Null when there
 * is nothing to write.
 */
export function advanceCodingSessionLastSeen(
  itemIds: readonly string[],
  stored: string | null,
  inViewIds: ReadonlySet<string>,
): string | null {
  let newest = -1;
  itemIds.forEach((id, index) => {
    if (inViewIds.has(id)) newest = index;
  });
  if (newest < 0) return null;
  const storedIndex = stored === null ? -1 : itemIds.indexOf(stored);
  if (storedIndex >= newest) return null;
  return itemIds[newest] ?? null;
}
