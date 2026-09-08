/**
 * The unread catch-up read, expressed as one batch instead of one REQ per
 * channel. Each channel keeps its own `since` (its read marker) and its own
 * `limit`; the transport sends every filter in one `POST /query` (chunked at
 * the relay's 128 aggregate `#h` cap) and returns the deduplicated union,
 * which {@link groupEventsByChannel} splits back per channel for the scan.
 */
import { getChannelIdFromTags } from "@/features/messages/lib/threading";
import type { RelaySubscriptionFilter } from "@/shared/api/relayClientShared";
import type { Channel, RelayEvent } from "@/shared/api/types";

import { CATCH_UP_LIMIT, channelCatchUpEventKinds } from "./unreadCatchUpScan";

export type CatchUpTarget = {
  channelId: string;
  channelType: Channel["channelType"] | undefined;
  /** The channel's effective read marker in unix seconds, or null. */
  readAt: number | null;
};

/**
 * NIP-01 `since` is inclusive (`created_at >= since`). The +1 makes the
 * relay-side filter strict-newer; the scan's own `> readAt` check is the belt
 * to these suspenders.
 */
export function catchUpSince(readAt: number | null): number {
  return readAt === null ? 0 : readAt + 1;
}

/** The catch-up filter for one channel — unchanged from the per-REQ shape. */
export function buildCatchUpFilter(
  target: CatchUpTarget,
): RelaySubscriptionFilter {
  return {
    kinds: [...channelCatchUpEventKinds(target.channelType)],
    "#h": [target.channelId],
    since: catchUpSince(target.readAt),
    limit: CATCH_UP_LIMIT,
  };
}

/** One filter per target, in the order given. */
export function buildCatchUpFilters(
  targets: readonly CatchUpTarget[],
): RelaySubscriptionFilter[] {
  return targets.map(buildCatchUpFilter);
}

/**
 * Split a batch response by the `h` tag each event carries, preserving the
 * union's order within each channel. Every catch-up filter is `#h`-scoped, so
 * the relay only returns `h`-tagged events; an event without one cannot be
 * attributed to a channel and is dropped rather than guessed.
 */
export function groupEventsByChannel(
  events: readonly RelayEvent[],
): Map<string, RelayEvent[]> {
  const byChannel = new Map<string, RelayEvent[]>();
  for (const event of events) {
    const channelId = getChannelIdFromTags(event.tags);
    if (!channelId) continue;
    const bucket = byChannel.get(channelId);
    if (bucket) {
      bucket.push(event);
    } else {
      byChannel.set(channelId, [event]);
    }
  }
  return byChannel;
}
