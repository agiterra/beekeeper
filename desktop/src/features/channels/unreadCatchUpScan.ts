/**
 * The catch-up scan: turn one channel's "anything newer than my read marker?"
 * REQ into the observed-unread events and thread-activity rows the sidebar
 * badge and the Home activity feed are built from.
 *
 * Split out of `useUnreadChannels` so the hook keeps only the React-owned
 * concerns (refs, persistence fencing, version bumps) and this pass stays a
 * plain function over a batch of relay events — the part worth reading when
 * asking "why did (or didn't) this channel light up after a restart?".
 */
import {
  makeObservedUnreadEvent,
  type ObservedUnreadEvent,
} from "@/features/channels/unreadChannelCounts";
import type { ThreadActivityItem } from "@/features/channels/threadActivityStorage";
import {
  isCodingSessionLaneMessageHiddenFromChannel,
  observeCodingSessionLaneRefs,
} from "@/features/messages/lib/codingSessionLaneVisibility";
import {
  getThreadReference,
  isBroadcastReply,
} from "@/features/messages/lib/threading";
import {
  isHighPriorityEventForUser,
  shouldNotifyForEvent,
} from "@/features/notifications/lib/shouldNotify";
import type { Channel, RelayEvent } from "@/shared/api/types";
import { CHANNEL_MESSAGE_EVENT_KINDS } from "@/shared/constants/kinds";
import { DM_NOTIFIABLE_EVENT_KINDS } from "./isDmNotifiableKind";

// Per-channel cap on the catch-up REQ. We only consume the *max matching*
// event per channel, but the relay can return self-authored / non-trigger
// events that we discard client-side, so we need enough head-room for the
// filter to find one external trigger message. 1000 matches the live sub's
// per-channel limit elsewhere in the app.
export const CATCH_UP_LIMIT = 1000;

export function channelCatchUpEventKinds(
  channelType: Channel["channelType"] | undefined,
) {
  return channelType === "dm"
    ? DM_NOTIFIABLE_EVENT_KINDS
    : CHANNEL_MESSAGE_EVENT_KINDS;
}

export function resolveObservedUnreadRootId(tags: string[][]): string | null {
  return isBroadcastReply(tags) ? null : getThreadReference(tags).rootId;
}

export type ChannelCatchUpScan = {
  /** Newest external, notifiable `created_at` seen — 0 when there is none. */
  maxExternal: number;
  unreadEvents: ObservedUnreadEvent[];
  threadReplies: ThreadActivityItem[];
};

export type ChannelCatchUpContext = {
  channelId: string;
  channelName: string;
  channelType: Channel["channelType"] | undefined;
  /** The channel's effective read marker in unix seconds, or null. */
  readAt: number | null;
  normalizedPubkey: string | null;
  /**
   * Membership sets. `participatedRootIds` and `authoredRootIds` are mutated
   * in place by pass 1 — the caller owns persisting them, and the notify gate
   * in pass 2 deliberately reads the freshly-grown sets.
   */
  participatedRootIds: Set<string>;
  authoredRootIds: Set<string>;
  followedRootIds: ReadonlySet<string>;
  mutedRootIds: ReadonlySet<string>;
  mutedChannelIds: ReadonlySet<string>;
  /** Records an external mention's thread root; owned by the caller. */
  recordMentionedRoot: (event: RelayEvent) => void;
};

export function scanChannelCatchUpEvents(
  events: readonly RelayEvent[],
  context: ChannelCatchUpContext,
): ChannelCatchUpScan {
  const {
    channelId,
    channelName,
    channelType,
    readAt,
    normalizedPubkey,
    participatedRootIds,
    authoredRootIds,
    recordMentionedRoot,
  } = context;

  // Session-lane chat this client hides from the channel timeline is not part
  // of the channel's unread surface at all. Observing first lets the visibility
  // hook resolve refs for channels the user has not opened.
  observeCodingSessionLaneRefs(channelId, events);
  const isHiddenLaneMessage = (event: RelayEvent) =>
    isCodingSessionLaneMessageHiddenFromChannel(channelId, event);

  // Pass 1: build participation from self-authored thread replies, track
  // self-authored top-level messages for author notifications, and capture
  // external mentions so their threads gate a badge.
  for (const event of events) {
    if (isHiddenLaneMessage(event)) continue;
    const isSelf =
      normalizedPubkey !== null &&
      event.pubkey.toLowerCase() === normalizedPubkey;
    if (isSelf) {
      const ref = getThreadReference(event.tags);
      if (ref.rootId !== null) {
        participatedRootIds.add(ref.rootId);
      } else {
        authoredRootIds.add(event.id);
      }
    } else {
      recordMentionedRoot(event);
    }
  }

  // Pass 2: compute maxExternal and collect thread reply activity, applying
  // the notification filter to both.
  let maxExternal = 0;
  const unreadEvents: ObservedUnreadEvent[] = [];
  const threadReplies: ThreadActivityItem[] = [];
  for (const event of events) {
    if (isHiddenLaneMessage(event)) continue;
    if (
      normalizedPubkey !== null &&
      event.pubkey.toLowerCase() === normalizedPubkey
    ) {
      continue;
    }
    if (readAt !== null && event.created_at <= readAt) continue;
    const eventChannelId = event.tags.find((t) => t[0] === "h")?.[1] ?? null;
    if (
      !shouldNotifyForEvent(event, normalizedPubkey ?? "", {
        participatedRootIds,
        followedRootIds: context.followedRootIds,
        authoredRootIds,
        mutedRootIds: context.mutedRootIds,
        mutedChannelIds: context.mutedChannelIds,
        channelId: eventChannelId,
      })
    ) {
      continue;
    }
    const evtRef = getThreadReference(event.tags);
    const isThreadedReply =
      evtRef.parentId !== null && !isBroadcastReply(event.tags);
    if (event.created_at > maxExternal) {
      maxExternal = event.created_at;
    }
    const isHighPriority =
      channelType === "dm" ||
      (normalizedPubkey !== null &&
        isHighPriorityEventForUser(event, normalizedPubkey));
    unreadEvents.push(
      makeObservedUnreadEvent({
        id: event.id,
        createdAt: event.created_at,
        rootId: resolveObservedUnreadRootId(event.tags),
        highPriority: isHighPriority,
        channelType,
        isThreadedReply,
      }),
    );
    if (isThreadedReply) {
      threadReplies.push({
        id: event.id,
        kind: event.kind,
        pubkey: event.pubkey,
        content: event.content,
        createdAt: event.created_at,
        channelId,
        channelName,
        tags: [...event.tags],
      });
    }
  }

  return { maxExternal, unreadEvents, threadReplies };
}
