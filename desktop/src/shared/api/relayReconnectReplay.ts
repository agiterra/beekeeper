import { CHANNEL_EVENT_KINDS } from "@/shared/constants/kinds";
import {
  sortEvents,
  type RelaySubscription,
  type RelaySubscriptionFilter,
} from "@/shared/api/relayClientShared";
import type { RelayEvent } from "@/shared/api/types";
import {
  isRateLimited,
  waitForRateLimit,
} from "@/shared/api/relayRateLimitGate";

const RECONNECT_REPLAY_SKEW_SECS = 5;
export const RECONNECT_REPLAY_PAGE_LIMIT = 500;
export const RECONNECT_REPLAY_PAGE_CONCURRENCY = 4;

/**
 * Maximum attempts for one subscription's history backfill.
 *
 * Backfill failures must never escape `replayLiveSubscriptions`: by the time
 * paging starts, every live REQ has already been re-established on a healthy,
 * authenticated socket. Letting a history rejection propagate makes the
 * session tear that socket down (`resetConnection`) and reconnect straight
 * into the same rate-limit window — the "briefly connected → can't reach the
 * relay" flap loop. Instead each sub retries behind the rate-limit gate a
 * bounded number of times, then degrades to live-only for this connection.
 * The window's lower bound is pinned in `pendingReplaySince` while unresolved
 * (live events advance `lastSeenCreatedAt` regardless of backfill success),
 * so the next reconnect still requests the missed window.
 */
export const PAGE_REPLAY_MAX_ATTEMPTS = 3;

/**
 * Live REQs in flight at once while replaying.
 *
 * This is not the pacing mechanism. Every REQ goes through the session's
 * `sendRaw`, which charges the send budget (`relaySendBudget.ts`) and waits
 * for a slot when the burst window is full; the old fixed "8 per 50 ms"
 * batches ran at 160/s against a 10/s relay budget. Bounding in-flight sends
 * only keeps the visible-channel-first order meaningful.
 */
export const REPLAY_SEND_CONCURRENCY = 4;

type LiveSubscription = Extract<RelaySubscription, { mode: "live" }>;

export type RequestHistoryBatch = (
  filters: RelaySubscriptionFilter[],
) => Promise<RelayEvent[]>;

async function runWithConcurrency<T>(
  items: T[],
  concurrency: number,
  worker: (item: T) => Promise<void>,
) {
  const workerCount = Math.min(Math.max(1, concurrency), items.length);
  let nextIndex = 0;

  await Promise.all(
    Array.from({ length: workerCount }, async () => {
      while (nextIndex < items.length) {
        const item = items[nextIndex++];
        await worker(item);
      }
    }),
  );
}

export function buildReconnectReplayFilter(
  filter: RelaySubscriptionFilter,
  since?: number,
  until?: number,
  limit = Math.min(filter.limit, RECONNECT_REPLAY_PAGE_LIMIT),
) {
  if (since === undefined) return filter;

  const replayFilter: RelaySubscriptionFilter = {
    ...filter,
    limit,
    since: filter.since === undefined ? since : Math.max(filter.since, since),
  };

  if (until !== undefined) {
    replayFilter.until =
      filter.until === undefined ? until : Math.min(filter.until, until);
  }

  return replayFilter;
}

function carriesChannelKinds(filter: RelaySubscriptionFilter) {
  return CHANNEL_EVENT_KINDS.every((kind) => filter.kinds.includes(kind));
}

/**
 * Single-channel subscriptions with a history window: the live REQ is
 * re-sent unchanged (its own `limit` gives the immediate tail) and the missed
 * window is paged separately.
 */
export function shouldPageReconnectReplay(filter: RelaySubscriptionFilter) {
  return (
    filter.limit > 0 &&
    Array.isArray(filter["#h"]) &&
    filter["#h"].length === 1 &&
    carriesChannelKinds(filter)
  );
}

/**
 * Channel subscriptions whose live REQ cannot carry the missed window itself:
 * a multi-`#h` filter shares one `limit` across all its channels, and a
 * `limit: 0` filter carries none. Their missed windows are backfilled one
 * filter per channel from that channel's own cursor.
 */
export function shouldBackfillChannels(filter: RelaySubscriptionFilter) {
  const channels = filter["#h"];
  return (
    Array.isArray(channels) &&
    channels.length > 0 &&
    !shouldPageReconnectReplay(filter) &&
    carriesChannelKinds(filter)
  );
}

/**
 * Page one filter's missed-window history.
 *
 * Returns `true` only when the window was genuinely completed (short page or
 * boundary reached). Returns `false` when the pass aborted because the
 * connection went stale (`isActive()` false) — callers must NOT treat that as
 * completion: the same subscription object is shared with the superseding
 * connection, and clearing its pinned `pendingReplaySince` on a stale abort
 * would erase the floor the new connection still needs.
 */
export async function replayReconnectHistoryPages({
  filter,
  onEvent,
  since,
  until,
  isActive,
  requestHistoryBatch,
}: {
  filter: RelaySubscriptionFilter;
  onEvent: (event: RelayEvent) => void;
  since: number;
  until: number;
  isActive: () => boolean;
  requestHistoryBatch: RequestHistoryBatch;
}): Promise<boolean> {
  let pageUntil = until;

  while (pageUntil >= since) {
    if (!isActive()) return false;

    const events = await requestHistoryBatch([
      buildReconnectReplayFilter(
        filter,
        since,
        pageUntil,
        RECONNECT_REPLAY_PAGE_LIMIT,
      ),
    ]);

    if (!isActive()) return false;

    for (const event of events) onEvent(event);
    if (events.length < RECONNECT_REPLAY_PAGE_LIMIT) return true;

    const oldestCreatedAt = events[0]?.created_at;
    if (oldestCreatedAt === undefined || oldestCreatedAt <= since) return true;

    pageUntil =
      oldestCreatedAt < pageUntil ? oldestCreatedAt : oldestCreatedAt - 1;
  }
  return true;
}

/**
 * One filter per channel of a multi-`#h` (or live-only) channel filter, each
 * from that channel's own replay cursor. Channels without any cursor are
 * skipped: nothing was ever seen there, so there is no window to close.
 */
export function buildChannelBackfillFilters(
  filter: RelaySubscriptionFilter,
  sinceFor: (channelId: string) => number | undefined,
  now: number,
): Array<{ channelId: string; filter: RelaySubscriptionFilter }> {
  const result: Array<{ channelId: string; filter: RelaySubscriptionFilter }> =
    [];
  for (const channelId of filter["#h"] ?? []) {
    const since = sinceFor(channelId);
    if (since === undefined) continue;
    result.push({
      channelId,
      filter: buildReconnectReplayFilter(
        { ...filter, "#h": [channelId] },
        since,
        now,
        RECONNECT_REPLAY_PAGE_LIMIT,
      ),
    });
  }
  return result;
}

function channelIdsOf(event: RelayEvent): string[] {
  const ids: string[] = [];
  for (const tag of event.tags) {
    if (tag[0] === "h" && tag[1] !== undefined) ids.push(tag[1]);
  }
  return ids;
}

/**
 * Backfill every channel of one filter: the first page for all channels goes
 * out as one batched read (one `POST /query`, chunked at the relay's `#h`
 * cap), then only channels that returned a full page keep paging on their
 * own. Same completion contract as {@link replayReconnectHistoryPages}.
 */
export async function replayChannelBackfill({
  filter,
  onEvent,
  sinceFor,
  now,
  isActive,
  requestHistoryBatch,
  pageConcurrency = RECONNECT_REPLAY_PAGE_CONCURRENCY,
}: {
  filter: RelaySubscriptionFilter;
  onEvent: (event: RelayEvent) => void;
  sinceFor: (channelId: string) => number | undefined;
  now: number;
  isActive: () => boolean;
  requestHistoryBatch: RequestHistoryBatch;
  pageConcurrency?: number;
}): Promise<boolean> {
  const perChannel = buildChannelBackfillFilters(filter, sinceFor, now);
  if (perChannel.length === 0) return true;
  if (!isActive()) return false;

  const events = sortEvents(
    await requestHistoryBatch(perChannel.map((entry) => entry.filter)),
  );
  if (!isActive()) return false;

  const oldestByChannel = new Map<string, number>();
  const countByChannel = new Map<string, number>();
  for (const event of events) {
    onEvent(event);
    for (const channelId of channelIdsOf(event)) {
      countByChannel.set(channelId, (countByChannel.get(channelId) ?? 0) + 1);
      const oldest = oldestByChannel.get(channelId);
      if (oldest === undefined || event.created_at < oldest) {
        oldestByChannel.set(channelId, event.created_at);
      }
    }
  }

  // A channel that filled its page may have more; continue paging it alone.
  const continuing = perChannel.filter(
    ({ channelId }) =>
      (countByChannel.get(channelId) ?? 0) >= RECONNECT_REPLAY_PAGE_LIMIT,
  );
  let completed = true;
  await runWithConcurrency(
    continuing,
    pageConcurrency,
    async ({ channelId, filter: channelFilter }) => {
      const since = channelFilter.since ?? 0;
      const oldest = oldestByChannel.get(channelId);
      if (oldest === undefined || oldest <= since) return;
      const pageUntil = oldest < now ? oldest : oldest - 1;
      const done = await replayReconnectHistoryPages({
        filter: channelFilter,
        onEvent,
        since,
        until: pageUntil,
        isActive,
        requestHistoryBatch,
      });
      if (!done) completed = false;
    },
  );
  return completed;
}

type ReplayRequest = {
  subId: string;
  subscription: LiveSubscription;
  /** Filters to send on the restored live REQ. */
  liveFilters: RelaySubscriptionFilter[];
  /** Backfill jobs, each with its own attempt loop. */
  backfills: Array<(isActive: () => boolean) => Promise<boolean>>;
  /** Lower bound of every window the backfills cover, or `undefined`. */
  floor: number | undefined;
};

function minDefined(...values: Array<number | undefined>) {
  let min: number | undefined;
  for (const value of values) {
    if (value === undefined) continue;
    min = min === undefined ? value : Math.min(min, value);
  }
  return min;
}

/**
 * Decide, for one live subscription, what the restored REQ carries and which
 * windows to backfill. Pure apart from `requestHistoryBatch` being captured.
 */
export function planSubscriptionReplay(
  subId: string,
  subscription: LiveSubscription,
  now: number,
  requestHistoryBatch: RequestHistoryBatch,
): ReplayRequest {
  const skewed = (createdAt: number | undefined) =>
    createdAt === undefined
      ? undefined
      : Math.max(0, createdAt - RECONNECT_REPLAY_SKEW_SECS);
  const cursorSince = skewed(subscription.lastSeenCreatedAt);
  // A pinned floor from a previously failed backfill takes precedence over
  // the cursor: live events kept advancing `lastSeenCreatedAt` while the
  // older window stayed unresolved, and starting from the cursor would skip
  // it permanently.
  const floor = subscription.pendingReplaySince;
  const replaySince =
    cursorSince === undefined ? floor : minDefined(cursorSince, floor);
  const perChannel = subscription.lastSeenByChannel ?? {};
  const sinceFor = (channelId: string) => {
    const channelCursor = skewed(perChannel[channelId]);
    return minDefined(channelCursor ?? cursorSince, floor);
  };

  const liveFilters: RelaySubscriptionFilter[] = [];
  const backfills: ReplayRequest["backfills"] = [];
  const floors: Array<number | undefined> = [];

  for (const filter of subscription.filters) {
    if (replaySince !== undefined && shouldPageReconnectReplay(filter)) {
      liveFilters.push(filter);
      floors.push(replaySince);
      backfills.push((isActive) =>
        replayReconnectHistoryPages({
          filter,
          onEvent: subscription.onEvent,
          since: replaySince,
          until: now,
          isActive,
          requestHistoryBatch,
        }),
      );
      continue;
    }
    if (shouldBackfillChannels(filter)) {
      const channelSinces = (filter["#h"] ?? []).map(sinceFor);
      const earliest = minDefined(...channelSinces);
      if (earliest !== undefined) {
        // The live REQ resumes from the earliest channel cursor with no
        // history of its own; the per-channel backfill closes each window.
        liveFilters.push({
          ...buildReconnectReplayFilter(filter, earliest),
          limit: 0,
        });
        floors.push(earliest);
        backfills.push((isActive) =>
          replayChannelBackfill({
            filter,
            onEvent: subscription.onEvent,
            sinceFor,
            now,
            isActive,
            requestHistoryBatch,
          }),
        );
        continue;
      }
    }
    liveFilters.push(buildReconnectReplayFilter(filter, replaySince));
  }

  return {
    subId,
    subscription,
    liveFilters,
    backfills,
    floor: minDefined(...floors),
  };
}

export async function replayLiveSubscriptions({
  subscriptions,
  sendRaw,
  requestHistoryBatch,
  now = Math.floor(Date.now() / 1_000),
  pageReplayConcurrency = RECONNECT_REPLAY_PAGE_CONCURRENCY,
  sendConcurrency = REPLAY_SEND_CONCURRENCY,
  visibleChannelId = null,
  isActive = () => true,
}: {
  subscriptions: Map<string, RelaySubscription>;
  /** Budgeted send: the session's `sendRaw`, which paces through the bucket. */
  sendRaw: (payload: unknown[]) => Promise<void>;
  /** Batched one-shot read (`fetchEventsBatch` without the connect gate). */
  requestHistoryBatch: RequestHistoryBatch;
  now?: number;
  pageReplayConcurrency?: number;
  /** Max live REQs in flight (injectable for tests). */
  sendConcurrency?: number;
  /** Channel currently visible in the UI — its subscriptions are sent first. */
  visibleChannelId?: string | null;
  /**
   * Returns false when the connection that initiated this replay has been
   * superseded by a newer one. After the gate await resumes, a stale replay
   * must not double-send REQs on the live socket.
   */
  isActive?: () => boolean;
}) {
  // If the relay has signalled back-pressure, wait for the gate to clear
  // before re-establishing REQs that would immediately be rate-limited.
  if (isRateLimited()) await waitForRateLimit();

  // A newer connection may have replayed while this one was suspended at the
  // gate — abort silently to avoid double-sending every REQ on the live socket.
  if (!isActive()) return;

  const replayRequests = Array.from(subscriptions.entries())
    .filter(
      (entry): entry is [string, LiveSubscription] => entry[1].mode === "live",
    )
    .map(([subId, subscription]) =>
      planSubscriptionReplay(subId, subscription, now, requestHistoryBatch),
    );

  // Sort the visible channel's subscriptions first so the user sees their
  // active channel recover before others on degraded networks.
  if (visibleChannelId !== null) {
    const mentionsVisible = (request: ReplayRequest) =>
      request.subscription.filters.some(
        (filter) => filter["#h"]?.includes(visibleChannelId) ?? false,
      );
    replayRequests.sort((a, b) => {
      const aVis = mentionsVisible(a);
      const bVis = mentionsVisible(b);
      if (aVis === bVis) return 0;
      return aVis ? -1 : 1;
    });
  }

  // Re-establish the live REQs. Pacing is the send budget's job inside
  // `sendRaw`; here we only re-check the gate before each frame — a previous
  // REQ may have been refused and armed it mid-replay — and confirm the
  // connection is still current after any wait.
  let stale = false;
  await runWithConcurrency(
    replayRequests,
    sendConcurrency,
    async ({ subId, liveFilters }) => {
      if (stale) return;
      if (isRateLimited()) await waitForRateLimit();
      if (!isActive()) {
        stale = true;
        return;
      }
      await sendRaw(["REQ", subId, ...liveFilters]);
    },
  );
  if (stale) return;

  await runWithConcurrency(
    replayRequests.filter((request) => request.backfills.length > 0),
    pageReplayConcurrency,
    async ({ subId, subscription, backfills, floor }) => {
      // Backfill is best-effort: a failure here (typically a `rate-limited:`
      // refusal) must never escape to the session and tear down the healthy,
      // authenticated socket carrying the live REQs — that is the
      // connect→drop flap loop. Retry behind the gate a bounded number of
      // times, then degrade to live-only for this connection.
      //
      // Pin the window's lower bound before the first attempt: events on the
      // already-restored live REQ advance the cursors independently of
      // backfill success, so without the pin an exhausted backfill followed
      // by one live event would make the next reconnect skip the unresolved
      // window permanently. Cleared only when every job completes.
      subscription.pendingReplaySince = floor;
      // Both guards are required. The identity check catches the sub being
      // torn down/replaced; the outer isActive() catches connection
      // supersession, which bumps the generation while the SAME subscription
      // key and object survive in the map — identity alone stays true and a
      // stale pass could complete and clear the floor the superseding
      // connection needs.
      const jobActive = () =>
        isActive() && subscriptions.get(subId) === subscription;
      let allCompleted = true;
      for (const backfill of backfills) {
        let completed = false;
        for (let attempt = 1; attempt <= PAGE_REPLAY_MAX_ATTEMPTS; attempt++) {
          try {
            completed = await backfill(jobActive);
            break;
          } catch (error) {
            console.warn(
              `[reconnect replay] history backfill attempt ${attempt}/${PAGE_REPLAY_MAX_ATTEMPTS} failed for ${subId}:`,
              error,
            );
            if (attempt === PAGE_REPLAY_MAX_ATTEMPTS) break;
            // A refused read arms the rate-limit gate before rejecting; wait
            // for it (no-op when the failure wasn't back-pressure) and
            // re-check that this replay's connection is still current.
            if (isRateLimited()) await waitForRateLimit();
            if (!jobActive()) return;
          }
        }
        if (!completed) allCompleted = false;
      }
      // A stale-connection abort is NOT completion: the superseding
      // connection shares this subscription object and still needs the
      // pinned floor for its own replay. Only a genuinely completed window
      // may release it.
      if (allCompleted) subscription.pendingReplaySince = undefined;
    },
  );
}
