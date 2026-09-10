export const CODING_SESSION_ROUTE =
  "/coding-sessions/$channelId/$generationId" as const;

/**
 * The route of a founded umbrella that has no execution yet — a genesis with
 * a goal and a name, waiting for somebody to pick who leads and start it.
 * Keyed by `sessionRef` because that is the only identity such a session has;
 * a sentinel generation id would feed the generation route a lie.
 */
export const CODING_SESSION_FOUNDED_ROUTE =
  "/coding-sessions/$channelId/founded/$sessionRef" as const;

export type CodingSessionSurface = "main" | "popout";

export function buildFoundedCodingSessionPath(
  channelId: string,
  sessionRef: string,
): string {
  return `/coding-sessions/${encodeURIComponent(channelId)}/founded/${encodeURIComponent(
    sessionRef,
  )}`;
}

/**
 * The shelf row id of a founded umbrella, in the slot a generation id fills
 * for started sessions. Mirrors the pending row's `pending:<commandId>`; it
 * can never collide with a real generation key, which carries the
 * `coding-session-transcript-generation/v1` structured-key prefix.
 */
export function foundedCodingSessionRowId(sessionRef: string): string {
  return `founded:${sessionRef}`;
}

/** The sessionRef behind a founded row id, or null for any other id. */
export function parseFoundedCodingSessionRowId(rowId: string): string | null {
  return rowId.startsWith("founded:") ? rowId.slice("founded:".length) : null;
}

export function buildCodingSessionPath(
  channelId: string,
  generationId: string,
): string {
  return `/coding-sessions/${encodeURIComponent(channelId)}/${encodeURIComponent(
    generationId,
  )}`;
}

export function buildCodingSessionPopoutUrl(
  channelId: string,
  generationId: string,
): string {
  return `/#${buildCodingSessionPath(channelId, generationId)}?surface=popout`;
}

export function parseCodingSessionSurface(
  value: unknown,
): CodingSessionSurface {
  return value === "popout" ? "popout" : "main";
}

export function isCodingSessionPopoutLocation(input: {
  pathname: string;
  search: Record<string, unknown>;
}): boolean {
  return (
    input.pathname.startsWith("/coding-sessions/") &&
    parseCodingSessionSurface(input.search.surface) === "popout"
  );
}

/**
 * Stable, Tauri-safe label for focus-existing behavior.
 *
 * The route coordinates stay out of the label because generation ids contain
 * structured-key punctuation that Tauri window labels reject.
 */
export function buildCodingSessionWindowLabel(
  channelId: string,
  generationId: string,
): string {
  const bytes = new TextEncoder().encode(
    `${channelId.length}:${channelId}${generationId.length}:${generationId}`,
  );
  let first = 0x811c9dc5;
  let second = 0x9e3779b9;
  for (const byte of bytes) {
    first = Math.imul(first ^ byte, 0x01000193) >>> 0;
    second = Math.imul(second ^ byte, 0x85ebca6b) >>> 0;
  }
  return `coding-session-${first.toString(16).padStart(8, "0")}${second
    .toString(16)
    .padStart(8, "0")}`;
}
