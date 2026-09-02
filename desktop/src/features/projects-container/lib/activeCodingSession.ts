/**
 * Which coding session the sidebar should show as selected.
 *
 * A coding session is addressed by `(channelId, generationId)` — the same
 * pair `onOpenCodingSession` navigates with — so the sidebar's active key is
 * those two joined. The sidebar's other two row types have carried an active
 * state since they existed (`isActiveChannel` for channels,
 * `activeShellSessionId` for terminals); coding sessions never did, so
 * opening one left the sidebar showing nothing selected at all.
 */

/** The key a session row compares itself against. */
export function codingSessionRowKey(
  channelId: string,
  generationId: string,
): string {
  return `${channelId}:${generationId}`;
}

/**
 * The active key for the current route, or null when no coding session is
 * open.
 *
 * The pathname gate is load-bearing: `channelId` is a route param on other
 * surfaces too, so matching on the params alone would mark a session row
 * active while a *channel* in the same transport channel was on screen.
 */
export function activeCodingSessionKey(
  pathname: string,
  params: { channelId?: string; generationId?: string },
): string | null {
  if (!pathname.startsWith("/coding-sessions/")) return null;
  if (!params.channelId || !params.generationId) return null;
  return codingSessionRowKey(params.channelId, params.generationId);
}

/**
 * The project coordinate a coding-session route belongs to.
 *
 * A session lives in a **transport** channel, and `useChannelsQuery` hides
 * transports from every consumer by default — so the app shell's
 * `activeChannel` is null on a coding-session route and the hold-a-modifier
 * navigation resolved no project, published a null scope, and numbered no
 * rows. Callers pass the transport-inclusive channel list.
 *
 * Returns null off a coding-session route so a caller can fall back to the
 * active channel's own project without a second branch.
 */
export function codingSessionProjectRef(
  pathname: string,
  channelId: string | undefined,
  channels: readonly { id: string; projectRef?: string | null }[] | undefined,
): string | null {
  if (!pathname.startsWith("/coding-sessions/")) return null;
  if (!channelId || !channels) return null;
  return channels.find((c) => c.id === channelId)?.projectRef ?? null;
}
