import { makeRootIdStore } from "@/features/channels/unreadRootIdStore";
import {
  forcedUnreadMarker,
  forcedUnreadStore,
  type ForcedUnreadMap,
} from "@/features/channels/forcedUnreadStore";
import { DM_NOTIFIABLE_EVENT_KINDS } from "@/features/channels/isDmNotifiableKind";
import { mergeReadStateEvents } from "@/features/channels/readState/readStateSnapshot";
import {
  maxReadAt,
  msgContextKey,
} from "@/features/channels/readState/readStateFormat";
import { isCodingSessionLaneMessageHiddenFromChannel } from "@/features/messages/lib/codingSessionLaneVisibility";
import {
  getThreadReference,
  isBroadcastReply,
} from "@/features/messages/lib/threading";
import { shouldNotifyForEvent } from "@/features/notifications/lib/shouldNotify";
import {
  mutedChannelIdsFromStore,
  parseMutePayload,
} from "@/features/sidebar/lib/channelMutesStorage";
import type { Community } from "@/features/communities/types";
import { withReadOnlyRelayClient } from "@/shared/api/readOnlyRelayClient";
import type { RelaySubscriptionFilter } from "@/shared/api/relayClientShared";
import { demuxForFilter } from "@/shared/api/relayQueryCoalescer";
import { nip44DecryptFromSelf } from "@/shared/api/tauri";
import type { ChannelType, RelayEvent } from "@/shared/api/types";
import {
  CHANNEL_MESSAGE_EVENT_KINDS,
  HOME_MENTION_EVENT_KINDS,
  KIND_CHANNEL_MUTES,
  KIND_DM_VISIBILITY,
  KIND_READ_STATE,
} from "@/shared/constants/kinds";

const KIND_NIP29_GROUP_METADATA = 39000;
const KIND_NIP29_GROUP_MEMBERS = 39002;

// Stores for thread-relationship sets. Keyed by pubkey only (no relay/community),
// so they read correctly from the same origin regardless of which community is active.
const participationStore = makeRootIdStore("buzz-thread-participation.v1");
const authoredStore = makeRootIdStore("buzz-thread-authored.v1");
const mutedRootsStore = makeRootIdStore("buzz-thread-muted.v1");
const FOLLOWS_STORAGE_KEY_PREFIX = "buzz-thread-follows.v1";

export type ThreadRelationships = {
  participatedRootIds: ReadonlySet<string>;
  followedRootIds: ReadonlySet<string>;
  authoredRootIds: ReadonlySet<string>;
  mutedRootIds: ReadonlySet<string>;
};

function readFollowedRootIds(pubkey: string): Set<string> {
  try {
    const raw = window.localStorage.getItem(
      `${FOLLOWS_STORAGE_KEY_PREFIX}:${pubkey}`,
    );
    if (!raw) return new Set();
    const parsed = JSON.parse(raw);
    if (!Array.isArray(parsed)) return new Set();
    const ids = new Set<string>();
    for (const entry of parsed) {
      if (
        typeof entry === "object" &&
        entry !== null &&
        typeof entry.rootId === "string"
      ) {
        ids.add(entry.rootId);
      }
    }
    return ids;
  } catch {
    return new Set();
  }
}

function defaultReadThreadRelationships(pubkey: string): ThreadRelationships {
  return {
    participatedRootIds: participationStore.read(pubkey),
    followedRootIds: readFollowedRootIds(pubkey),
    authoredRootIds: authoredStore.read(pubkey),
    mutedRootIds: mutedRootsStore.read(pubkey),
  };
}

const MEMBER_CHANNEL_LIMIT = 1000;
const METADATA_LIMIT = 1000;
const UNREAD_EXISTENCE_LIMIT = 50;
const MENTION_COUNT_LIMIT = 100;
const READ_STATE_FETCH_LIMIT = 500;
const READ_STATE_HORIZON_SECONDS = 7 * 24 * 60 * 60;

export type CommunityUnreadObserverResult = {
  hasUnread: boolean;
  mentionCount: number;
};

/**
 * The poll's relay surface: every read is a bundle. The read-only client
 * packs a bundle into as few REQs as the relay allows (10 filters, 128
 * aggregate `#h` per frame) and returns the deduplicated union, which this
 * module splits back per filter with {@link demuxForFilter}.
 */
type CommunityUnreadRelay = {
  fetchEventsBatch(filters: RelaySubscriptionFilter[]): Promise<RelayEvent[]>;
};

/** The one-filter-at-a-time surface `communityMarkRead` still drives. */
type ObservedChannelsRelay = {
  fetchEvents(filter: RelaySubscriptionFilter): Promise<RelayEvent[]>;
};

type ObservedChannel = {
  id: string;
  channelType: ChannelType;
  archived: boolean;
};

/** Membership listing for `pubkey`: the ids come from the `d` tags. */
export function memberChannelsFilter(pubkey: string): RelaySubscriptionFilter {
  return {
    kinds: [KIND_NIP29_GROUP_MEMBERS],
    "#p": [pubkey],
    limit: MEMBER_CHANNEL_LIMIT,
  };
}

/**
 * The two reads that turn a membership list into a visibility set: channel
 * metadata (type, archived) for those ids, and the user's DM visibility
 * snapshot. Independent of each other, so they travel in one bundle.
 */
export function observedChannelFilters(
  channelIds: string[],
  pubkey: string,
): { metadata: RelaySubscriptionFilter; visibility: RelaySubscriptionFilter } {
  return {
    metadata: {
      kinds: [KIND_NIP29_GROUP_METADATA],
      "#d": channelIds,
      limit: METADATA_LIMIT,
    },
    visibility: {
      kinds: [KIND_DM_VISIBILITY],
      "#p": [pubkey],
      limit: 1,
    },
  };
}

/**
 * The visibility rule itself: exclude archived channels and hidden DMs. The
 * unread poll and "mark all as read" both go through here, so they cannot
 * disagree about which channels count.
 */
export function resolveVisibleChannels(
  channelIds: string[],
  metadataEvents: RelayEvent[],
  visibilityEvents: RelayEvent[],
): ObservedChannel[] {
  const hiddenDmIds = extractHiddenDmIds(visibilityEvents);
  return resolveObservedChannels(channelIds, metadataEvents).filter(
    (channel) =>
      !channel.archived &&
      (channel.channelType !== "dm" || !hiddenDmIds.has(channel.id)),
  );
}

/**
 * List the channels this pubkey is a member of on the observed relay,
 * excluding archived channels and hidden DMs — the same visibility set the
 * unread poll and "mark all as read" must agree on.
 */
export async function fetchObservedChannels(
  client: ObservedChannelsRelay,
  pubkey: string,
): Promise<ObservedChannel[]> {
  const memberEvents = await client.fetchEvents(memberChannelsFilter(pubkey));
  const channelIds = extractMemberChannelIds(memberEvents);
  if (channelIds.length === 0) {
    return [];
  }

  const { metadata, visibility } = observedChannelFilters(channelIds, pubkey);
  const [metadataEvents, visibilityEvents] = await Promise.all([
    client.fetchEvents(metadata),
    client.fetchEvents(visibility),
  ]);
  return resolveVisibleChannels(channelIds, metadataEvents, visibilityEvents);
}

export async function pollCommunityUnread(
  community: Community,
  pubkey: string,
): Promise<CommunityUnreadObserverResult> {
  return withReadOnlyRelayClient(community.relayUrl, (client) =>
    fetchCommunityUnread({ client, pubkey }),
  );
}

/** The per-channel pair of the unread bundle; `unread` is null once the dot is already lit. */
export type ChannelUnreadFilters = {
  channel: ObservedChannel;
  readAt: number | null;
  unread: RelaySubscriptionFilter | null;
  mention: RelaySubscriptionFilter;
};

/**
 * Build one channel's existence and mention filters, both `since` the
 * channel's read marker. `includeUnread` is false when an earlier gate (a
 * forced-unread channel) has already decided the dot, so only the mention
 * count still needs the relay.
 */
export function buildChannelUnreadFilters(
  channel: ObservedChannel,
  pubkey: string,
  readAt: number | null,
  includeUnread: boolean,
): ChannelUnreadFilters {
  const since = readAt === null ? 0 : readAt + 1;
  return {
    channel,
    readAt,
    unread: includeUnread
      ? {
          kinds: unreadKindsForChannel(channel.channelType),
          "#h": [channel.id],
          since,
          limit: UNREAD_EXISTENCE_LIMIT,
        }
      : null,
    mention: {
      kinds: [...HOME_MENTION_EVENT_KINDS],
      "#h": [channel.id],
      "#p": [pubkey],
      since,
      limit: MENTION_COUNT_LIMIT,
    },
  };
}

function isForcedUnread(
  forcedUnreadMap: ForcedUnreadMap,
  channelId: string,
  readAt: number | null,
): boolean {
  // Forced-unread lights the dot without a relay fetch, but only if the
  // synced read marker has NOT advanced past the stored baseline. This
  // prevents stale forced-unread from lighting the rail after a cross-device
  // read has covered the channel (the drain path in useUnreadChannels only
  // runs while the community is active, so the store may not be pruned for
  // inactive communities).
  if (!Object.hasOwn(forcedUnreadMap, channelId)) return false;
  const markerAtWhenForced = forcedUnreadMarker(forcedUnreadMap[channelId]);
  return (
    readAt === null ||
    (markerAtWhenForced !== null && readAt <= markerAtWhenForced)
  );
}

/**
 * One poll of an inactive community: three bundles instead of `5 + 2N`
 * frames — the membership list; then metadata, DM visibility, read state and
 * mutes together (the metadata `#d` needs the member ids, which is why the
 * list travels alone); then every channel's existence and mention filter.
 */
export async function fetchCommunityUnread(args: {
  client: CommunityUnreadRelay;
  pubkey: string;
  nowSeconds?: number;
  decryptReadState?: (ciphertext: string) => Promise<string>;
  decryptMutes?: (ciphertext: string) => Promise<string>;
  readThreadRelationships?: (pubkey: string) => ThreadRelationships;
  readForcedUnread?: (pubkey: string) => ForcedUnreadMap;
}): Promise<CommunityUnreadObserverResult> {
  const { client, pubkey } = args;
  const normalizedPubkey = pubkey.toLowerCase();
  const nowSeconds = args.nowSeconds ?? Math.floor(Date.now() / 1_000);
  const decryptMutes = args.decryptMutes ?? nip44DecryptFromSelf;
  const readRelationships =
    args.readThreadRelationships ?? defaultReadThreadRelationships;
  const readForcedUnread =
    args.readForcedUnread ?? ((pk) => forcedUnreadStore.read(pk));

  const memberEvents = await client.fetchEventsBatch([
    memberChannelsFilter(pubkey),
  ]);
  const channelIds = extractMemberChannelIds(memberEvents);
  if (channelIds.length === 0) {
    return { hasUnread: false, mentionCount: 0 };
  }

  const { metadata, visibility } = observedChannelFilters(channelIds, pubkey);
  const readStateFilter: RelaySubscriptionFilter = {
    kinds: [KIND_READ_STATE],
    authors: [pubkey],
    "#t": ["read-state"],
    since: nowSeconds - READ_STATE_HORIZON_SECONDS,
    limit: READ_STATE_FETCH_LIMIT,
  };
  const mutesFilter: RelaySubscriptionFilter = {
    kinds: [KIND_CHANNEL_MUTES],
    authors: [pubkey],
    "#d": ["channel-mutes"],
    limit: 1,
  };
  const stateEvents = await client.fetchEventsBatch([
    metadata,
    visibility,
    readStateFilter,
    mutesFilter,
  ]);

  const channels = resolveVisibleChannels(
    channelIds,
    demuxForFilter(stateEvents, metadata),
    demuxForFilter(stateEvents, visibility),
  );
  if (channels.length === 0) {
    return { hasUnread: false, mentionCount: 0 };
  }

  const readState = await mergeReadStateEvents(
    demuxForFilter(stateEvents, readStateFilter),
    pubkey,
    args.decryptReadState,
  );

  let mutedIds = new Set<string>();
  const mutesEvents = demuxForFilter(stateEvents, mutesFilter);
  if (mutesEvents.length > 0) {
    try {
      const plaintext = await decryptMutes(mutesEvents[0].content);
      const store = parseMutePayload(JSON.parse(plaintext));
      if (store) {
        mutedIds = mutedChannelIdsFromStore(store);
      }
    } catch {
      // decryption failure → treat as empty mutes set
    }
  }

  const {
    participatedRootIds,
    followedRootIds,
    authoredRootIds,
    mutedRootIds,
  } = readRelationships(normalizedPubkey);

  // Channels manually marked unread on this device. Stored as a record of
  // { channelId: markerAtWhenForced } so the observer can gate the dot on
  // whether a cross-device read has since advanced past the stored baseline.
  const forcedUnreadMap = readForcedUnread(normalizedPubkey);

  const observed = channels
    .filter((channel) => !mutedIds.has(channel.id))
    .map((channel) => ({
      channel,
      readAt: readState.get(channel.id) ?? null,
    }));

  // The forced-unread gate needs no relay round-trip, so it decides first:
  // once the dot is lit, only the mention count still needs the relay.
  let hasUnread = observed.some(({ channel, readAt }) =>
    isForcedUnread(forcedUnreadMap, channel.id, readAt),
  );
  const plans = observed.map(({ channel, readAt }) =>
    buildChannelUnreadFilters(channel, pubkey, readAt, !hasUnread),
  );
  const filters = plans.flatMap((plan) =>
    plan.unread ? [plan.unread, plan.mention] : [plan.mention],
  );
  const events =
    filters.length === 0 ? [] : await client.fetchEventsBatch(filters);

  let mentionCount = 0;
  for (const { channel, readAt, unread, mention } of plans) {
    // Both queries use kind sets whose kind:9 members may be coding-session
    // lane chat, which renders inside a session umbrella rather than in the
    // channel — counting it here would light a rail dot with nothing behind
    // it. Same rule as the timeline and the active community's unread: an
    // unresolved ref stays ordinary chat and still counts, which is also what
    // an inactive community sees, since lane refs only resolve for the
    // community the user currently has open.
    const isHiddenLaneMessage = (event: RelayEvent) =>
      isCodingSessionLaneMessageHiddenFromChannel(channel.id, event);

    if (!hasUnread && unread !== null) {
      hasUnread = demuxForFilter(events, unread).some(
        (event) =>
          !isHiddenLaneMessage(event) &&
          isUnreadExternalEvent(event, readState, readAt, normalizedPubkey) &&
          shouldNotifyForEvent(event, normalizedPubkey, {
            participatedRootIds,
            followedRootIds,
            authoredRootIds,
            mutedRootIds,
            mutedChannelIds: mutedIds,
            channelId: channel.id,
          }),
      );
    }

    mentionCount += demuxForFilter(events, mention).filter(
      (event) =>
        !isHiddenLaneMessage(event) &&
        isUnreadExternalEvent(event, readState, readAt, normalizedPubkey),
    ).length;
  }

  return { hasUnread: hasUnread || mentionCount > 0, mentionCount };
}

export function extractMemberChannelIds(events: RelayEvent[]): string[] {
  const ids = new Set<string>();
  for (const event of events) {
    for (const tag of event.tags) {
      if (tag[0] === "d" && tag[1]) {
        ids.add(tag[1]);
      }
    }
  }
  return [...ids];
}

export function resolveObservedChannels(
  channelIds: string[],
  metadataEvents: RelayEvent[],
): ObservedChannel[] {
  const latestMetadata = new Map<string, RelayEvent>();
  for (const event of metadataEvents) {
    const channelId = tagValue(event, "d");
    if (!channelId) continue;
    const existing = latestMetadata.get(channelId);
    if (!existing || event.created_at > existing.created_at) {
      latestMetadata.set(channelId, event);
    }
  }

  return channelIds.map((id) => {
    const metadata = latestMetadata.get(id);
    const typeTag = metadata ? tagValue(metadata, "t") : null;
    return {
      id,
      channelType: toChannelType(typeTag),
      archived:
        metadata?.tags.some(
          (tag) => tag[0] === "archived" && tag[1] === "true",
        ) ?? false,
    };
  });
}

export function extractHiddenDmIds(events: RelayEvent[]): Set<string> {
  const latest = events.reduce<RelayEvent | null>(
    (current, event) =>
      current === null || event.created_at > current.created_at
        ? event
        : current,
    null,
  );
  return new Set(
    (latest?.tags ?? [])
      .filter((tag) => tag[0] === "h" && tag[1])
      .map((tag) => tag[1]),
  );
}

function unreadKindsForChannel(channelType: ChannelType): number[] {
  return channelType === "dm"
    ? [...DM_NOTIFIABLE_EVENT_KINDS]
    : [...CHANNEL_MESSAGE_EVENT_KINDS];
}

function isUnreadExternalEvent(
  event: RelayEvent,
  readState: ReadonlyMap<string, number>,
  channelReadAt: number | null,
  normalizedPubkey: string,
): boolean {
  if (event.pubkey.toLowerCase() === normalizedPubkey) return false;

  const rootId = isBroadcastReply(event.tags)
    ? null
    : getThreadReference(event.tags).rootId;
  const readAt = maxReadAt(
    channelReadAt,
    readState.get(msgContextKey(event.id)) ?? null,
    rootId === null ? null : (readState.get(`thread:${rootId}`) ?? null),
  );

  return readAt === null || event.created_at > readAt;
}

function tagValue(event: RelayEvent, name: string): string | null {
  return event.tags.find((tag) => tag[0] === name)?.[1] ?? null;
}

function toChannelType(value: string | null): ChannelType {
  return value === "forum" || value === "dm" ? value : "stream";
}
