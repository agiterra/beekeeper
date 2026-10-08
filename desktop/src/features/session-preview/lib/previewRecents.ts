/**
 * "Recently used" in the Browser's empty state: the local URLs this computer
 * opened, newest first. Local addresses are host-local facts, so they live in
 * this machine's `localStorage` and nowhere else (never the relay). Read on
 * every use; nothing is cached at module level, so `resetCommunityState()`
 * has nothing to clear.
 */
export const SESSION_PREVIEW_RECENTS_KEY =
  "beekeeper:session-preview:recents:v1";
export const SESSION_PREVIEW_RECENTS_MAX = 8;

export type SessionPreviewRecent = {
  url: string;
  title: string | null;
  /** Unix ms on this computer's clock. */
  at: number;
};

type Storage = Pick<globalThis.Storage, "getItem" | "setItem">;

function defaultStorage(): Storage | null {
  try {
    return typeof window === "undefined" ? null : window.localStorage;
  } catch {
    return null;
  }
}

/** Parse what was stored, dropping anything malformed rather than throwing. */
export function parseSessionPreviewRecents(
  raw: string | null,
): SessionPreviewRecent[] {
  if (!raw) return [];
  try {
    const value = JSON.parse(raw);
    if (!Array.isArray(value)) return [];
    return value
      .filter(
        (entry): entry is SessionPreviewRecent =>
          entry !== null &&
          typeof entry === "object" &&
          typeof entry.url === "string" &&
          typeof entry.at === "number",
      )
      .map((entry) => ({
        url: entry.url,
        title: typeof entry.title === "string" ? entry.title : null,
        at: entry.at,
      }))
      .slice(0, SESSION_PREVIEW_RECENTS_MAX);
  } catch {
    return [];
  }
}

/** The list with `entry` first, the same URL once, capped. */
export function withSessionPreviewRecent(
  list: readonly SessionPreviewRecent[],
  entry: SessionPreviewRecent,
): SessionPreviewRecent[] {
  return [entry, ...list.filter((item) => item.url !== entry.url)].slice(
    0,
    SESSION_PREVIEW_RECENTS_MAX,
  );
}

export function readSessionPreviewRecents(
  storage: Storage | null = defaultStorage(),
): SessionPreviewRecent[] {
  return parseSessionPreviewRecents(
    storage?.getItem(SESSION_PREVIEW_RECENTS_KEY) ?? null,
  );
}

export function rememberSessionPreviewRecent(
  entry: SessionPreviewRecent,
  storage: Storage | null = defaultStorage(),
): SessionPreviewRecent[] {
  const next = withSessionPreviewRecent(
    readSessionPreviewRecents(storage),
    entry,
  );
  try {
    storage?.setItem(SESSION_PREVIEW_RECENTS_KEY, JSON.stringify(next));
  } catch {
    // A full or blocked store loses the recent, never the navigation.
  }
  return next;
}
