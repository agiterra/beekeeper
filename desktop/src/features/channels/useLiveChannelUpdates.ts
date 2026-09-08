import * as React from "react";
import { useQueryClient } from "@tanstack/react-query";

import { channelsQueryKey } from "@/features/channels/hooks";
import { mergeTimelineCacheMessages } from "@/features/messages/hooks";
import { channelMessagesKey } from "@/features/messages/lib/messageQueryKeys";
import {
  getChannelIdFromTags,
  isThreadReply,
} from "@/features/messages/lib/threading";
import {
  isCodingSessionLaneMessageHiddenFromChannel,
  observeCodingSessionLaneRefs,
} from "@/features/messages/lib/codingSessionLaneVisibility";
import { shouldNotifyForEvent } from "@/features/notifications/lib/shouldNotify";
import { relayClient } from "@/shared/api/relayClient";
import type { RelaySubscriptionFilter } from "@/shared/api/relayClientShared";
import { matchesFilter } from "@/shared/api/relayFilterMatch";
import { CHANNEL_MESSAGE_EVENT_KINDS } from "@/shared/constants/kinds";
import type { Channel, RelayEvent } from "@/shared/api/types";
import {
  createTrailingDebounce,
  type TrailingDebounce,
} from "@/shared/lib/trailingDebounce";

import { isDmNotifiableKind } from "./isDmNotifiableKind";
import {
  liveRequestKey,
  mentionMatchFilter,
  planLiveChannelFilters,
} from "./liveChannelSubscriptionPlan";
import { refreshChannelsWhenIdle } from "./refreshChannelsWhenIdle";

export type UseLiveChannelUpdatesOptions = {
  currentPubkey?: string;
  /**
   * When true, DM notifications also fire for the channel the user is
   * currently viewing (normally suppressed).
   */
  notifyForActiveChannel?: boolean;
  onDmMessage?: (event: RelayEvent, channel: Channel) => void;
  onLiveMention?: () => void;
  /**
   * Fired for live "new content" events in a member channel authored by
   * someone other than the current user. Thread replies also fire
   * onThreadReplyNotification so Home inbox activity stays in sync. Used to
   * drive the observed unread-event map that powers sidebar unread state.
   * See `UNREAD_TRIGGER_KINDS` for the exact kind set.
   */
  onChannelMessage?: (channelId: string, event: RelayEvent) => void;
  /**
   * Fired for thread replies that should be surfaced as Home inbox activity.
   */
  onThreadReplyNotification?: (channelId: string, event: RelayEvent) => void;
  /**
   * Fired for external thread replies that do not match the locally-known
   * interest sets. Callers can perform an async backfill and then decide
   * whether to surface the event.
   */
  onThreadReplyCandidate?: (channelId: string, event: RelayEvent) => void;
  /**
   * Fired for replies in threads the user authored, participated in, or
   * follows (non-DM channels only — the DM path owns those). Follows the DM
   * active-channel rule: suppressed for the channel being viewed unless
   * notifyForActiveChannel opts in.
   */
  onThreadReplyDesktopNotification?: (
    channelId: string,
    event: RelayEvent,
  ) => void;
  onSelfChannelMessage?: (event: RelayEvent) => void;
  participatedRootIds?: ReadonlySet<string>;
  followedRootIds?: ReadonlySet<string>;
  authoredRootIds?: ReadonlySet<string>;
  mutedRootIds?: ReadonlySet<string>;
  mutedChannelIds?: ReadonlySet<string>;
};

const LIVE_SUBSCRIPTION_RETRY_BASE_MS = 1_000;
const LIVE_SUBSCRIPTION_RETRY_MAX_MS = 30_000;

// get_channels is an expensive O(channels) relay fan-out. Incoming traffic for
// non-active channels arrives in bursts, so coalesce the refetch into a single
// trailing invalidation instead of one per event.
const CHANNELS_INVALIDATE_DEBOUNCE_MS = 500;

// Only "new content" kinds should bump unread state. Shared with the
// catch-up query in useUnreadChannels so the two paths stay in lockstep.
const UNREAD_TRIGGER_KINDS = new Set<number>(CHANNEL_MESSAGE_EVENT_KINDS);

export const EMPTY_SET: ReadonlySet<string> = new Set();

export function isChannelUnreadTriggerKind(kind: number, isDmChannel: boolean) {
  return isDmChannel
    ? isDmNotifiableKind(kind)
    : UNREAD_TRIGGER_KINDS.has(kind);
}

export function isHomeActivityEvent(
  isDmChannel: boolean,
  isThreadedReply: boolean,
) {
  return isThreadedReply || isDmChannel;
}

/**
 * Guard for events arriving on a bundled live subscription. The REQ covers
 * many channels, so an event without an `h` tag can no longer be attributed
 * to "the" channel of its subscription: it is dropped, and the drop is
 * visible in the console, rather than filed under a guess.
 */
export function requireChannelTag(event: RelayEvent): RelayEvent | null {
  if (getChannelIdFromTags(event.tags)) {
    return event;
  }
  console.warn(
    "Dropping live event without an h tag; it cannot be attributed to a channel",
    { id: event.id, kind: event.kind },
  );
  return null;
}

type RequestSubscriptions = Map<string, () => Promise<void>>;

/**
 * Make-before-break sync of one plan half against the open REQs. Every group
 * missing from `activeSubs` is opened first; only once those are open are the
 * groups no longer planned closed, so a membership change never leaves a
 * channel unobserved in between. Returns false when any open failed, which
 * leaves the superseded groups in place for the retry.
 */
async function syncRequestGroups(
  activeSubs: RequestSubscriptions,
  groups: readonly (readonly RelaySubscriptionFilter[])[],
  onEvent: (event: RelayEvent) => void,
  isCancelled: () => boolean,
  label: string,
): Promise<boolean> {
  const targets = new Map(
    groups.map((group) => [liveRequestKey(group), group]),
  );
  let anyFailed = false;
  const additions = Array.from(targets)
    .filter(([key]) => !activeSubs.has(key))
    .map(async ([key, group]) => {
      try {
        const dispose = await relayClient.subscribeLiveMany(
          [...group],
          onEvent,
        );
        if (isCancelled()) {
          void dispose().catch(() => {});
          return;
        }
        activeSubs.set(key, dispose);
      } catch (err) {
        anyFailed = true;
        console.error(`Failed to subscribe to ${label}`, key, err);
      }
    });
  await Promise.allSettled(additions);
  if (isCancelled() || anyFailed) {
    return !anyFailed;
  }
  for (const [key, dispose] of activeSubs) {
    if (!targets.has(key)) {
      activeSubs.delete(key);
      void dispose().catch(() => {});
    }
  }
  return true;
}

function isExternalMentionEvent(event: RelayEvent, currentPubkey: string) {
  return (
    currentPubkey.length > 0 && event.pubkey.toLowerCase() !== currentPubkey
  );
}

const SEEN_NOTIFICATION_EVENT_LIMIT = 5_000;

export function trackSeenEvent(
  seenEventIds: Set<string>,
  eventId: string,
  limit = 200,
): boolean {
  if (seenEventIds.has(eventId)) {
    return false;
  }

  seenEventIds.add(eventId);
  if (seenEventIds.size > limit) {
    const oldestEventId = seenEventIds.values().next().value;
    if (oldestEventId) {
      seenEventIds.delete(oldestEventId);
    }
  }

  return true;
}

export function useLiveChannelUpdates(
  channels: Channel[],
  activeChannelId: string | null,
  options: UseLiveChannelUpdatesOptions = {},
) {
  const queryClient = useQueryClient();
  const normalizedCurrentPubkey =
    options.currentPubkey?.trim().toLowerCase() ?? "";
  const seenMentionEventIdsRef = React.useRef(new Set<string>());
  // Reconnect replay overlaps each live filter by five seconds so no message is
  // lost at the boundary. Keep one shared guard for every notification side
  // effect: the same event can be replayed repeatedly while a relay flaps, and
  // mention events also arrive through both the channel and mention filters.
  const seenNotificationEventIdsRef = React.useRef(new Set<string>());
  const channelsInvalidateRef = React.useRef<TrailingDebounce | null>(null);
  if (channelsInvalidateRef.current === null) {
    channelsInvalidateRef.current = createTrailingDebounce(() => {
      refreshChannelsWhenIdle({
        isFetching: () =>
          queryClient.isFetching({ queryKey: channelsQueryKey }),
        invalidate: () => {
          void queryClient.invalidateQueries({ queryKey: channelsQueryKey });
        },
        reArm: () => channelsInvalidateRef.current?.trigger(),
      });
    }, CHANNELS_INVALIDATE_DEBOUNCE_MS);
  }
  const invalidateChannelsDebounced = React.useCallback(() => {
    channelsInvalidateRef.current?.trigger();
  }, []);
  const liveChannelIds = React.useMemo(
    () => new Set(channels.map((channel) => channel.id)),
    [channels],
  );
  const dmChannelMap = React.useMemo(
    () =>
      new Map(
        channels
          .filter((channel) => channel.channelType === "dm")
          .map((channel) => [channel.id, channel]),
      ),
    [channels],
  );
  const dmSubscriptionStartedAtRef = React.useRef(0);

  // Reset subscription timestamp when identity changes.
  React.useEffect(() => {
    void normalizedCurrentPubkey;
    dmSubscriptionStartedAtRef.current = 0;
  }, [normalizedCurrentPubkey]);

  // Effect deps use primitive keys so refetches that produce new refs with
  // identical contents don't churn subscriptions. The Set/array memos are
  // still handy for closure reads via useEffectEvent.
  const channelIdsKey = React.useMemo(
    () => [...new Set(channels.map((channel) => channel.id))].sort().join(","),
    [channels],
  );

  const handleDmEvent = React.useEffectEvent(
    (event: RelayEvent, isFirstNotificationDelivery: boolean) => {
      // Only human-visible message kinds should fire DM notifications.
      if (!isDmNotifiableKind(event.kind) || !isFirstNotificationDelivery) {
        return;
      }

      // Suppress backlog events that predate our subscription — these are
      // historical replays, not live messages.
      if (event.created_at < dmSubscriptionStartedAtRef.current) {
        return;
      }

      const channelId = getChannelIdFromTags(event.tags);
      if (!channelId) {
        return;
      }

      if (!isExternalMentionEvent(event, normalizedCurrentPubkey)) {
        return;
      }

      const dmChannel = dmChannelMap.get(channelId);
      if (!dmChannel) {
        return;
      }

      // Don't fire a notification for the channel the user is already viewing,
      // unless the notify-while-viewing setting opts in.
      if (channelId === activeChannelId && !options.notifyForActiveChannel) {
        return;
      }

      options.onDmMessage?.(event, dmChannel);
    },
  );

  const handleIncomingMessage = React.useEffectEvent((event: RelayEvent) => {
    const channelId = getChannelIdFromTags(event.tags);
    if (!channelId) {
      return;
    }

    if (!liveChannelIds.has(channelId)) {
      if (channelId !== activeChannelId) {
        invalidateChannelsDebounced();
      }
      return;
    }

    // A session-lane message this client renders inside a coding session's
    // umbrella is not visible in this channel, so none of the side effects
    // below apply to it: no OS notification, no DM alert, no mention ping, no
    // unread. Observing first lets the visibility hook resolve refs for
    // channels the user has not opened; the gate itself fails open, so an
    // unresolved or forged ref stays ordinary chat and notifies normally.
    observeCodingSessionLaneRefs(channelId, [event]);
    if (isCodingSessionLaneMessageHiddenFromChannel(channelId, event)) {
      return;
    }

    const isDmChannel = dmChannelMap.has(channelId);
    const isUnreadTriggerKind = isChannelUnreadTriggerKind(
      event.kind,
      isDmChannel,
    );

    // Let the caller observe self-authored trigger events (e.g. to track
    // thread participation) before the author-exclusion guard filters them.
    if (
      isUnreadTriggerKind &&
      normalizedCurrentPubkey.length > 0 &&
      event.pubkey.toLowerCase() === normalizedCurrentPubkey
    ) {
      options.onSelfChannelMessage?.(event);
    }

    // Notify the unread tracker. Restricted to human-visible message kinds
    // and to events authored by someone other than the current user — your
    // own outgoing messages should never make a channel unread, and
    // reactions / edits / system messages aren't "new content".
    const isExternalTriggerEvent =
      isUnreadTriggerKind &&
      (normalizedCurrentPubkey.length === 0 ||
        event.pubkey.toLowerCase() !== normalizedCurrentPubkey);
    const isFirstNotificationDelivery =
      !isExternalTriggerEvent ||
      trackSeenEvent(
        seenNotificationEventIdsRef.current,
        event.id,
        SEEN_NOTIFICATION_EVENT_LIMIT,
      );
    const isThreadedReply = isThreadReply(event.tags);

    // DM alerts and every other notification side effect share this delivery
    // decision, preventing a replayed event from escaping through a second
    // callback path.
    handleDmEvent(event, isFirstNotificationDelivery);

    if (isExternalTriggerEvent && isFirstNotificationDelivery) {
      const shouldNotify = shouldNotifyForEvent(
        event,
        normalizedCurrentPubkey,
        {
          participatedRootIds: options.participatedRootIds ?? EMPTY_SET,
          followedRootIds: options.followedRootIds ?? EMPTY_SET,
          authoredRootIds: options.authoredRootIds ?? EMPTY_SET,
          mutedRootIds: options.mutedRootIds ?? EMPTY_SET,
          mutedChannelIds: options.mutedChannelIds ?? EMPTY_SET,
          channelId,
        },
      );

      if (!shouldNotify) {
        if (isThreadedReply) {
          options.onThreadReplyCandidate?.(channelId, event);
        }
      } else {
        options.onChannelMessage?.(channelId, event);
        if (isHomeActivityEvent(isDmChannel, isThreadedReply)) {
          options.onThreadReplyNotification?.(channelId, event);
        }
      }

      if (shouldNotify && isThreadedReply) {
        if (
          !dmChannelMap.has(channelId) &&
          (channelId !== activeChannelId || options.notifyForActiveChannel)
        ) {
          options.onThreadReplyDesktopNotification?.(channelId, event);
        }
      }
    }

    // Merge into the timeline cache for the active channel.
    // useChannelSubscription also writes to this cache, but there's a
    // race window where it hasn't connected yet. Writes are idempotent
    // (mergeTimelineCacheMessages deduplicates by event ID).
    queryClient.setQueryData<RelayEvent[]>(
      channelMessagesKey(channelId),
      (current) => {
        if (!current) {
          return current;
        }

        return mergeTimelineCacheMessages(current, event);
      },
    );
  });

  // Mentions are matched client-side on the live stream: the relay-shaped
  // `#p` filter is a strict subset of the live filter, so a second REQ per
  // chunk would carry nothing new. This event handler reads the current
  // pubkey and options, so a long-lived REQ never holds a stale identity.
  const handleMentionEvent = React.useEffectEvent((event: RelayEvent) => {
    if (
      !options.onLiveMention ||
      !isExternalMentionEvent(event, normalizedCurrentPubkey)
    ) {
      return;
    }

    const matcher = mentionMatchFilter(normalizedCurrentPubkey);
    if (matcher === null || !matchesFilter(event, matcher)) {
      return;
    }

    if (!trackSeenEvent(seenMentionEventIdsRef.current, event.id)) {
      return;
    }

    // The mention filter matches on kind + `#p`, so it also admits lane-tagged
    // chat. A mention inside a lane the user can open is surfaced by that
    // lane, not by the Home mention chime — resolve the channel from the
    // event's own `h` tag, as the timeline merge path does.
    if (
      isCodingSessionLaneMessageHiddenFromChannel(
        getChannelIdFromTags(event.tags),
        event,
      )
    ) {
      return;
    }

    options.onLiveMention?.();
  });

  React.useEffect(() => {
    return relayClient.subscribeToReconnects(() => {
      void queryClient.invalidateQueries({ queryKey: channelsQueryKey });

      // Update the subscription timestamp so replayed backlog events
      // (which have created_at in the past) are naturally suppressed.
      dmSubscriptionStartedAtRef.current = Math.floor(Date.now() / 1000);
    });
  }, [queryClient]);

  // Live channel events arrive on one REQ per 128 channels rather than one
  // per channel. The diff manager is keyed by the channels a REQ carries, so
  // a membership change opens the new bundle before closing the old one.
  const liveSubsRef = React.useRef<RequestSubscriptions>(new Map());

  React.useEffect(() => {
    let isCancelled = false;
    let retryTimeout: number | undefined;
    let retryAttempt = 0;

    const syncSubs = async (): Promise<boolean> => {
      const targetIds = channelIdsKey ? channelIdsKey.split(",") : [];
      const nowSeconds = Math.floor(Date.now() / 1_000);

      if (targetIds.length > 0) {
        // Record the subscription start time so handleDmEvent can distinguish
        // backlog replays (created_at < startedAt) from live messages.
        dmSubscriptionStartedAtRef.current = nowSeconds;
      }

      const { live } = planLiveChannelFilters(targetIds, "", nowSeconds);
      // Both handlers are stable useEffectEvent callbacks. Do NOT wrap them
      // in an isCancelled check: subs persist across effect runs (that's the
      // point of the diff manager), so a stale isCancelled flag from a prior
      // run would silently drop events on long-lived subs.
      return syncRequestGroups(
        liveSubsRef.current,
        live,
        (event) => {
          const scoped = requireChannelTag(event);
          if (scoped) {
            handleIncomingMessage(scoped);
            handleMentionEvent(scoped);
          }
        },
        () => isCancelled,
        "live channel updates",
      );
    };

    const runSync = async () => {
      const ok = await syncSubs();
      if (isCancelled) return;
      if (ok) {
        retryAttempt = 0;
        return;
      }
      const delayMs = Math.min(
        LIVE_SUBSCRIPTION_RETRY_BASE_MS * 2 ** retryAttempt,
        LIVE_SUBSCRIPTION_RETRY_MAX_MS,
      );
      retryAttempt += 1;
      retryTimeout = window.setTimeout(() => {
        retryTimeout = undefined;
        void runSync();
      }, delayMs);
    };

    void runSync();

    return () => {
      isCancelled = true;
      if (retryTimeout !== undefined) {
        window.clearTimeout(retryTimeout);
      }
    };
  }, [channelIdsKey]);

  React.useEffect(() => {
    return () => {
      channelsInvalidateRef.current?.cancel();

      for (const dispose of liveSubsRef.current.values()) {
        void dispose().catch(() => {});
      }
      liveSubsRef.current.clear();
    };
  }, []);
}
