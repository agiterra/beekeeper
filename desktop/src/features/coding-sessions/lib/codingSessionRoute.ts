export const CODING_SESSION_ROUTE =
  "/coding-sessions/$channelId/$generationId" as const;

export type CodingSessionSurface = "main" | "popout";

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
