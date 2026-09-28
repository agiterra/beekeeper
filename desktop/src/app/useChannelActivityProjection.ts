import * as React from "react";

import { useThreadActivityFeedItems } from "@/app/useThreadActivityFeedItems";
import {
  maxReadAt,
  msgContextKey,
  THREAD_PREFIX,
} from "@/features/channels/readState/readStateFormat";
import {
  activityScopeKey,
  findAbsentThreadActivityIds,
  recordAbsentThreadActivityIds,
} from "@/features/channels/threadActivityStorage";
import type { ThreadActivityItem } from "@/features/channels/useUnreadChannels";
import { useCommunities } from "@/features/communities/useCommunities";
import {
  getThreadReference,
  isThreadReply,
} from "@/features/messages/lib/threading";
import { useIdentityQuery } from "@/shared/api/hooks";
import type { Channel, FeedItem, HomeFeed } from "@/shared/api/types";

const NO_ABSENT_IDS: ReadonlySet<string> = new Set<string>();

/**
 * Delay before the one reconcile pass per scope. Long enough to be off the
 * boot critical path and to let the relay session finish connecting + NIP-42.
 */
const RECONCILE_START_DELAY_MS = 5_000;
/** Backoff between attempts when the relay never answered. */
const RECONCILE_RETRY_DELAY_MS = 30_000;
/** Total attempts per scope per session, including the first. */
const RECONCILE_MAX_ATTEMPTS = 3;

/**
 * Drop persisted thread-activity rows whose events the relay positively
 * confirms it no longer holds.
 *
 * Why this exists: the rows are a localStorage cache keyed by relay URL with no
 * link back to the events they mirror. A relay that lost its data (reset,
 * reinstall at the same URL) leaves rows behind that the Inbox presented as
 * current, actionable items indefinitely — clicking one only ever produced
 * "Some message context could not be loaded", and the item could never be
 * dismissed because there was nothing on the relay to dismiss.
 *
 * Shape of the pass:
 * - **Bounded.** One pass per (identity, relay) per session, over the rows
 *   hydrated from storage, in chunks of {@link RECONCILE_CHUNK_SIZE}. Rows that
 *   arrive live during the session came off the relay moments earlier and are
 *   not re-queried. No per-render relay traffic.
 * - **Fails closed.** Only a query that *completes* can evict; a relay that is
 *   unreachable, erroring, rate-limited, or closing the subscription leaves
 *   every row in place and simply retries, up to
 *   {@link RECONCILE_MAX_ATTEMPTS} times, then gives up until the next session.
 */
function useReconciledThreadActivityItems(
  threadActivityItems: ThreadActivityItem[],
): ThreadActivityItem[] {
  const identityQuery = useIdentityQuery();
  const { activeCommunity } = useCommunities();
  const pubkey = identityQuery.data?.pubkey ?? null;
  const relayUrl = activeCommunity?.relayUrl ?? "";
  const scope = activityScopeKey(pubkey, relayUrl);

  const [absentIds, setAbsentIds] =
    React.useState<ReadonlySet<string>>(NO_ABSENT_IDS);
  const itemsRef = React.useRef(threadActivityItems);
  itemsRef.current = threadActivityItems;

  React.useEffect(() => {
    // A different identity or relay means a different store; nothing judged
    // about the previous scope carries over.
    setAbsentIds(NO_ABSENT_IDS);
    if (!scope || !pubkey || !relayUrl) return;

    let cancelled = false;
    let timer: ReturnType<typeof setTimeout> | null = null;
    let attempt = 0;

    const runPass = async () => {
      timer = null;
      if (cancelled) return;
      const snapshot = itemsRef.current;
      if (snapshot.length === 0) return;

      attempt += 1;
      let complete = false;
      try {
        // Lazy so the relay singleton stays out of this module's import graph.
        const { relayClient } = await import("@/shared/api/relayClient");
        const result = await findAbsentThreadActivityIds(
          snapshot,
          ({ ids, kinds }) =>
            relayClient.fetchEvents({ ids, kinds, limit: ids.length }),
        );
        if (cancelled) return;
        complete = result.complete;

        if (result.absentIds.length > 0) {
          recordAbsentThreadActivityIds(pubkey, relayUrl, result.absentIds);
          const censored = new Set(result.absentIds);
          setAbsentIds((previous) => {
            for (const id of previous) censored.add(id);
            return censored;
          });
        }
      } catch {
        // Nothing was judged. Leave every row in place and try again.
        if (cancelled) return;
      }

      // `complete === false` means the relay never answered for some ids —
      // ambiguous, so nothing was judged for them. Retry rather than assume.
      if (!complete && attempt < RECONCILE_MAX_ATTEMPTS) {
        timer = setTimeout(() => {
          void runPass();
        }, RECONCILE_RETRY_DELAY_MS);
      }
    };

    timer = setTimeout(() => {
      void runPass();
    }, RECONCILE_START_DELAY_MS);

    return () => {
      cancelled = true;
      if (timer !== null) clearTimeout(timer);
    };
  }, [pubkey, relayUrl, scope]);

  return React.useMemo(
    () =>
      absentIds.size === 0
        ? threadActivityItems
        : threadActivityItems.filter((item) => !absentIds.has(item.id)),
    [absentIds, threadActivityItems],
  );
}

type ReadTimestamp = (contextKey: string) => number | null;
type MarkChannelRead = (
  contextKey: string,
  readAt: string | null | undefined,
  options?: { topLevelOnly?: boolean },
) => void;

type UseChannelActivityProjectionOptions = {
  channels: Channel[];
  feed: HomeFeed | undefined;
  unreadFeedItemIds: ReadonlySet<string>;
  getChannelReadAt: ReadTimestamp;
  getOwnReadAt: ReadTimestamp;
  markChannelRead: MarkChannelRead;
  readStateVersion: number;
  threadActivityItems: ThreadActivityItem[];
  mutedRootIds: ReadonlySet<string>;
};

/**
 * When a thread-activity row counts as read — the predicate behind the channel
 * unread pip and the Inbox row.
 *
 * Three terms, and the middle one is load-bearing. The row's own `msg:<id>`
 * marker is what the Inbox click writes per reply. `thread:<root>` is the
 * AGGREGATE marker, written by the Inbox click and by opening the thread; it
 * covers every reply in the thread at or below its timestamp, which is what
 * makes opening a thread clear the channel pip without touching a single
 * reply's own marker. Reading it here is what lets the pip be a thread-level
 * signal while the in-panel per-branch badges stay per-message: those read
 * effective(msg:<id>) only, so a collapsed branch still says it holds replies
 * you have not looked at (ledger 279(g)). The channel marker covers both.
 *
 * A newer reply still re-raises the pip: the caller's predicate is strictly
 * createdAt > this value, and a reply that lands after the open is newer than
 * the aggregate marker the open wrote.
 */
export function resolveChannelActivityFeedItemReadAt(
  item: Pick<FeedItem, "channelId" | "id" | "tags">,
  getOwnReadAt: ReadTimestamp,
): number | null {
  const rootId = getThreadReference(item.tags ?? []).rootId;
  return maxReadAt(
    getOwnReadAt(msgContextKey(item.id)),
    rootId ? getOwnReadAt(`${THREAD_PREFIX}${rootId}`) : null,
    item.channelId ? getOwnReadAt(item.channelId) : null,
  );
}

export function useChannelActivityProjection({
  channels,
  feed,
  unreadFeedItemIds,
  getChannelReadAt,
  getOwnReadAt,
  markChannelRead,
  readStateVersion,
  threadActivityItems,
  mutedRootIds,
}: UseChannelActivityProjectionOptions) {
  const getThreadReadAt = React.useCallback(
    (rootId: string, channelId?: string | null) => {
      const threadReadAt = getOwnReadAt(`thread:${rootId}`);
      if (!channelId) return threadReadAt;

      const channelReadAt = getChannelReadAt(channelId);
      if (threadReadAt === null) return channelReadAt;
      if (channelReadAt === null) return threadReadAt;
      return Math.max(threadReadAt, channelReadAt);
    },
    [getChannelReadAt, getOwnReadAt],
  );
  const markThreadRead = React.useCallback(
    (rootId: string, timestamp: number) =>
      markChannelRead(
        `thread:${rootId}`,
        new Date(timestamp * 1_000).toISOString(),
      ),
    [markChannelRead],
  );
  const getMessageReadAt = React.useCallback(
    (messageId: string) => getChannelReadAt(msgContextKey(messageId)),
    [getChannelReadAt],
  );
  const getChannelActivityItemReadAt = React.useCallback(
    (item: Pick<FeedItem, "channelId" | "id" | "tags">) =>
      resolveChannelActivityFeedItemReadAt(item, getOwnReadAt),
    [getOwnReadAt],
  );
  const markMessageRead = React.useCallback(
    (messageId: string, timestamp: number) =>
      markChannelRead(
        msgContextKey(messageId),
        new Date(timestamp * 1_000).toISOString(),
      ),
    [markChannelRead],
  );
  // Every surface that shows thread activity (Home/Inbox rows, sidebar dots,
  // mark-all-read) projects from this one list, so a row the relay has
  // confirmed gone disappears everywhere at once rather than lingering in
  // whichever surface reads the raw buffer.
  const reconciledActivityItems =
    useReconciledThreadActivityItems(threadActivityItems);
  const threadActivityFeedItems = useThreadActivityFeedItems(
    reconciledActivityItems,
    mutedRootIds,
    channels,
  );
  const locallyUnreadFeedItems = React.useMemo(() => {
    if (!feed || unreadFeedItemIds.size === 0) return [];
    return [
      ...feed.mentions,
      ...feed.needsAction,
      ...feed.activity,
      ...feed.agentActivity,
    ].filter((item) => unreadFeedItemIds.has(item.id));
  }, [feed, unreadFeedItemIds]);
  const unreadThreadFeedItems = React.useMemo(() => {
    void readStateVersion;
    const candidatesById = new Map<string, FeedItem>(
      threadActivityFeedItems.map((item) => [item.id, item]),
    );
    for (const item of locallyUnreadFeedItems)
      candidatesById.set(item.id, item);

    return [...candidatesById.values()].filter(
      (item) =>
        isThreadReply(item.tags) &&
        (unreadFeedItemIds.has(item.id) ||
          item.createdAt > (getChannelActivityItemReadAt(item) ?? 0)),
    );
  }, [
    getChannelActivityItemReadAt,
    locallyUnreadFeedItems,
    readStateVersion,
    threadActivityFeedItems,
    unreadFeedItemIds,
  ]);
  const unreadThreadChannelIds = React.useMemo(
    () =>
      new Set(
        unreadThreadFeedItems.flatMap((item) =>
          item.channelId ? [item.channelId] : [],
        ),
      ) as ReadonlySet<string>,
    [unreadThreadFeedItems],
  );

  return {
    getThreadReadAt,
    markThreadRead,
    getMessageReadAt,
    getChannelActivityItemReadAt,
    markMessageRead,
    threadActivityFeedItems,
    locallyUnreadFeedItems,
    unreadThreadFeedItems,
    unreadThreadChannelIds,
  };
}
