/**
 * Persisted model favourites.
 *
 * Local to this machine on purpose: a favourite is a preference about how one
 * person drives their own providers, not a fact about the session, so it never
 * becomes a signed event. Stored as provider-scoped keys (see
 * `codingSessionModelFavoriteKey`) so pinning Codex on this computer does not
 * pin a different machine's Codex in the catalog.
 */
const STORAGE_KEY = "buzz-coding-session-model-favorites-v1";
/** A pin list is a convenience; an unbounded one is a storage leak. */
const MAX_FAVORITES = 64;

/** Parse stored JSON into a favourites set, tolerating anything malformed. */
export function parseCodingSessionModelFavorites(
  raw: string | null,
): Set<string> {
  if (raw === null || raw === "") return new Set();
  try {
    const parsed: unknown = JSON.parse(raw);
    if (!Array.isArray(parsed)) return new Set();
    return new Set(
      parsed
        .filter((value): value is string => typeof value === "string")
        .filter((value) => value.length > 0)
        .slice(0, MAX_FAVORITES),
    );
  } catch {
    // A corrupt preference is a preference nobody set, not a broken picker.
    return new Set();
  }
}

/** Toggle one favourite, bounded, preserving insertion order. */
export function toggleCodingSessionModelFavorite(
  favorites: ReadonlySet<string>,
  favoriteKey: string,
): Set<string> {
  const next = new Set(favorites);
  if (next.has(favoriteKey)) {
    next.delete(favoriteKey);
    return next;
  }
  if (next.size >= MAX_FAVORITES) return next;
  next.add(favoriteKey);
  return next;
}

export function readCodingSessionModelFavorites(): Set<string> {
  if (typeof window === "undefined") return new Set();
  try {
    return parseCodingSessionModelFavorites(
      window.localStorage.getItem(STORAGE_KEY),
    );
  } catch {
    return new Set();
  }
}

export function writeCodingSessionModelFavorites(
  favorites: ReadonlySet<string>,
): void {
  if (typeof window === "undefined") return;
  try {
    window.localStorage.setItem(STORAGE_KEY, JSON.stringify([...favorites]));
  } catch {
    // Storage full or blocked: the picker still works for this session.
  }
}
