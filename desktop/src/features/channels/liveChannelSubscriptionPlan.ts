/**
 * Plan the live REQs that keep a sidebar's channels current.
 *
 * One REQ used to be opened per channel, twice (channel events and mentions),
 * so a 25-channel desktop spent 50 admission units on cold start. A single
 * filter may instead list many channels in `#h`; the relay caps the aggregate
 * `#h` count per REQ at {@link MAX_CHANNEL_VALUES_PER_REQUEST}
 * (`MAX_EXPLICIT_CHANNEL_VALUES` in `crates/beekeeper-relay/src/handlers/req.rs`,
 * summed over every filter in the frame), so this module chunks the channel
 * list at that size and then packs the resulting filters into as few REQs as
 * the same cap and {@link MAX_FILTERS_PER_REQ} allow.
 *
 * Because the cap is aggregate, a full 128-channel chunk saturates its REQ on
 * its own: N channels cost `ceil(N / 128)` REQs, not `ceil(N / 1280)`. The
 * packing step still runs through the shared chunker so the plan follows the
 * caps rather than restating them.
 *
 * Mentions ride the same REQs. The mention filter
 * (`HOME_MENTION_EVENT_KINDS`, the same `#h`, plus `#p: [me]`) is a strict
 * subset of the live filter, so a second REQ per chunk would deliver nothing
 * the first does not; instead the plan hands back that `#p` filter as a
 * client-side matcher and the hook applies it to the live stream. That is
 * one frame per 128 channels, against two for separate REQs or
 * `ceil(N / 64)` for both filters sharing a frame under the aggregate cap.
 */
import {
  MAX_CHANNEL_VALUES_PER_REQUEST,
  MAX_FILTERS_PER_REQ,
  type RelaySubscriptionFilter,
} from "@/shared/api/relayClientShared";
import { chunkFiltersForRequest } from "@/shared/api/relayQueryCoalescer";
import {
  CHANNEL_EVENT_KINDS,
  HOME_MENTION_EVENT_KINDS,
} from "@/shared/constants/kinds";

/**
 * `live`: each inner array is the filter list of one `subscribeLiveMany`
 * call (one REQ). `mention`: the `#p` filter to match live events against
 * client-side (`matchesFilter`), or null when there is no pubkey to match.
 */
export type LiveChannelSubscriptionPlan = {
  live: RelaySubscriptionFilter[][];
  mention: RelaySubscriptionFilter | null;
};

/**
 * The relay-shaped mention filter, minus `#h` and `since`: applied to events
 * the live REQs already scoped by channel and time. Deliberately the same
 * clause the relay would evaluate (`kinds` and `#p`), so client and relay
 * agree on what a mention is.
 */
export function mentionMatchFilter(
  pubkey: string,
): RelaySubscriptionFilter | null {
  const normalizedPubkey = pubkey.trim().toLowerCase();
  if (normalizedPubkey.length === 0) return null;
  return {
    kinds: [...HOME_MENTION_EVENT_KINDS],
    "#p": [normalizedPubkey],
    limit: 0,
  };
}

/** Split a channel list into runs of at most `size` ids, order preserved. */
export function chunkChannelIds(
  channelIds: readonly string[],
  size = MAX_CHANNEL_VALUES_PER_REQUEST,
): string[][] {
  const chunks: string[][] = [];
  for (let start = 0; start < channelIds.length; start += size) {
    chunks.push(channelIds.slice(start, start + size));
  }
  return chunks;
}

/**
 * Pack filters into REQ-sized groups under both relay caps. Full chunks each
 * land in their own group; only trailing partial chunks could ever share one.
 */
function groupFiltersForRequests(
  filters: RelaySubscriptionFilter[],
): RelaySubscriptionFilter[][] {
  return chunkFiltersForRequest(
    filters.map((filter) => ({ filter })),
    { maxFilters: MAX_FILTERS_PER_REQ },
  ).map((group) => group.map((entry) => entry.filter));
}

/**
 * Build the live subscription plan for `channelIds`.
 *
 * Every filter is `limit: 0` with `since: nowSeconds`: nothing is replayed at
 * open, and on reconnect the transport backfills each channel from its own
 * cursor (`lastSeenByChannel`) rather than through the REQ's limit.
 * Ids are deduplicated and sorted so the same membership always yields the
 * same chunks, which is what lets {@link liveRequestKey} act as a stable
 * identity across syncs.
 */
export function planLiveChannelFilters(
  channelIds: readonly string[],
  pubkey: string,
  nowSeconds: number,
): LiveChannelSubscriptionPlan {
  const ids = [...new Set(channelIds)].sort();
  if (ids.length === 0) {
    return { live: [], mention: mentionMatchFilter(pubkey) };
  }
  const live = groupFiltersForRequests(
    chunkChannelIds(ids).map((chunk) => ({
      kinds: [...CHANNEL_EVENT_KINDS],
      "#h": chunk,
      limit: 0,
      since: nowSeconds,
    })),
  );
  return { live, mention: mentionMatchFilter(pubkey) };
}

/**
 * Identity of one REQ group for the diff-based subscription manager: the
 * channels it carries, and nothing time-dependent. Two syncs that plan the
 * same channels produce the same key even though `since` moved, so an
 * unchanged bundle is left open rather than reopened.
 */
export function liveRequestKey(group: readonly RelaySubscriptionFilter[]) {
  return group.map((filter) => (filter["#h"] ?? []).join(",")).join("|");
}
