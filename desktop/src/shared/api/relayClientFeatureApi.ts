import { signRelayEvent } from "@/shared/api/tauri";
import { queryRelayFilters } from "@/shared/api/relayQueryBridge";
import type { PresenceStatus, RelayEvent } from "@/shared/api/types";
import {
  KIND_STREAM_MESSAGE,
  KIND_TYPING_INDICATOR,
  KIND_USER_STATUS,
  CHANNEL_EVENT_KINDS,
  KIND_CHANNEL_THREAD_SUMMARY,
} from "@/shared/constants/kinds";
import type {
  LiveSubscriptionReadiness,
  RelaySubscriptionFilter,
} from "@/shared/api/relayClientShared";
import {
  buildChannelAuxDeletionFilter,
  buildChannelFilter,
  buildChannelHistoryFilter,
  buildChannelMentionFilter,
  buildGlobalStreamFilter,
} from "@/shared/api/relayChannelFilters";
import { RelaySessionCore } from "@/shared/api/relayClientSession";
import {
  fetchChunkedHistory,
  requestFirstEventGated,
} from "@/shared/api/relayGateBoundary";
import { isMatchableFilter } from "@/shared/api/relayFilterMatch";
import {
  executeQueryBatch,
  unionQueryResults,
} from "@/shared/api/relayQueryCoalescer";
import { isRateLimited } from "@/shared/api/relayRateLimitGate";
import { relaySendBudget } from "@/shared/api/relaySendBudget";
import { HISTORY_TIMEOUT_MS } from "@/shared/api/relayClientTimings";
import { buildThreadReferenceTags } from "@/features/messages/lib/threading";

/**
 * The relay client feature code talks to: the transport core
 * (`RelaySessionCore`, `relayClientSession.ts`) plus the product-level reads,
 * writes and subscriptions built on it. Instantiated once in
 * `relayClient.ts`.
 *
 * Read paths, cheapest first:
 * - {@link fetchEventsCoalesced} — one-shot read that shares a `POST /query`
 *   with every other coalesced read in the same 50 ms. Use it for every
 *   one-shot filter without `search`.
 * - {@link fetchEventsBatch} — an explicit bundle of filters as one call.
 * - {@link fetchEvents} — one WebSocket REQ. Costs an admission unit against
 *   the per-key burst the paired phone shares; keep for `search` filters and
 *   anything the matcher cannot demultiplex.
 */
export class RelayClient extends RelaySessionCore {
  async fetchChannelHistory(channelId: string, limit = 50) {
    return this.fetchHistory(buildChannelHistoryFilter(channelId, limit));
  }

  async fetchChannelHistoryBefore(
    channelId: string,
    before: number,
    limit = 50,
  ) {
    return this.fetchHistory(
      buildChannelHistoryFilter(channelId, limit, before),
    );
  }

  async fetchAuxEventsByReference(
    channelId: string,
    referencedEventIds: string[],
    buildFilter: (
      channelId: string,
      eventIds: string[],
    ) => RelaySubscriptionFilter,
  ) {
    return fetchChunkedHistory(
      referencedEventIds,
      (eventIds) => buildFilter(channelId, eventIds),
      (filter) => this.fetchHistory(filter),
    );
  }

  async fetchAuxDeletionEventsForAuxEvents(
    channelId: string,
    auxEventIds: string[],
  ): Promise<RelayEvent[]> {
    return fetchChunkedHistory(
      auxEventIds,
      (eventIds) => buildChannelAuxDeletionFilter(channelId, eventIds),
      (filter) => this.fetchHistory(filter),
    );
  }

  /** One-shot read as one WebSocket REQ. Prefer {@link fetchEventsCoalesced}. */
  async fetchEvents(filter: RelaySubscriptionFilter): Promise<RelayEvent[]> {
    return this.fetchHistory(filter);
  }

  /**
   * One-shot read that shares one `POST /query` with every other coalesced
   * read issued in the same 50 ms window. Results are exactly what
   * {@link fetchEvents} would have returned for `filter` (matched, deduped,
   * newest `limit`, oldest-first). A filter the matcher cannot demultiplex
   * (`search`, extension fields) goes straight to the WS path.
   */
  async fetchEventsCoalesced(
    filter: RelaySubscriptionFilter,
  ): Promise<RelayEvent[]> {
    if (!isMatchableFilter(filter)) return this.fetchHistory(filter);
    return this.queryCoalescer.enqueue(filter);
  }

  /**
   * Explicit bundle: every filter in one `POST /query` (chunked at the relay's
   * 128 aggregate `#h` cap), resolved as the deduplicated, oldest-first union.
   * Each filter keeps its own `limit`. On HTTP failure every filter falls back
   * to its own WS REQ; the first fallback failure rejects the whole call.
   */
  async fetchEventsBatch(
    filters: RelaySubscriptionFilter[],
  ): Promise<RelayEvent[]> {
    if (filters.length === 0) return [];
    return unionQueryResults(
      await executeQueryBatch(filters, {
        query: queryRelayFilters,
        fallback: (filter) => this.fetchHistory(filter),
      }),
    );
  }

  async fetchFirstEvent(
    filter: RelaySubscriptionFilter,
  ): Promise<RelayEvent | null> {
    await this.ensureConnected();
    return requestFirstEventGated(
      this.subscriptions,
      (payload) => this.sendRaw(payload),
      (subId) => this.closeSubscription(subId),
      filter,
      HISTORY_TIMEOUT_MS,
    );
  }

  async sendMessage(
    channelId: string,
    content: string,
    mentionPubkeys: string[] = [],
    extraTags: string[][] = [],
  ) {
    await this.ensureConnected();

    const tags: string[][] = [["h", channelId]];
    for (const pubkey of mentionPubkeys) {
      tags.push(["p", pubkey]);
    }
    for (const tag of extraTags) {
      tags.push(tag);
    }

    const event = await signRelayEvent({
      kind: KIND_STREAM_MESSAGE,
      content: content.trim(),
      tags,
    });

    return this.publishEvent(
      event,
      "Timed out while sending the message.",
      "Failed to send the message.",
    );
  }

  async sendPresence(status: PresenceStatus) {
    await this.ensureConnected();

    const event = await signRelayEvent({
      kind: 20001,
      content: status,
      tags: [],
    });

    return this.publishEvent(
      event,
      "Timed out while updating presence.",
      "Failed to update presence.",
    );
  }

  /**
   * Fire-and-forget typing frame. Dropped — not queued — when the relay has
   * signalled back-pressure or the ephemeral lane has no slot: a late typing
   * indicator is worse than none, and the relay counts it against the same
   * per-key EVENT quota as a chat message.
   */
  async sendTypingIndicator(
    channelId: string,
    parentEventId?: string | null,
    rootEventId?: string | null,
  ) {
    // Disconnected: not worth triggering a reconnect for ephemeral typing.
    if (this.wsId === null) {
      return;
    }
    if (isRateLimited() || !relaySendBudget().tryAcquire("ephemeral")) {
      return;
    }
    const event = await signRelayEvent({
      kind: KIND_TYPING_INDICATOR,
      content: "",
      tags: buildThreadReferenceTags(
        channelId,
        parentEventId ?? null,
        rootEventId ?? null,
      ),
    });

    // Fire-and-forget: no need to wait for relay acknowledgement. The slot
    // was charged above, so the send must not charge it again.
    void this.sendRaw(["EVENT", event], { preAdmitted: true }).catch(() => {});
  }

  async subscribeToChannel(
    channelId: string,
    onEvent: (event: RelayEvent) => void,
  ) {
    return this.subscribe(buildChannelFilter(channelId, 50), onEvent);
  }

  /** Subscribe to channel rows and aux starting now, with no history replay. */
  async subscribeToChannelLive(
    channelId: string,
    onEvent: (event: RelayEvent) => void,
  ) {
    // 39005 rides only this window-store subscription — CHANNEL_EVENT_KINDS'
    // other consumers (unread tracking, cache merges) must never see
    // summary overlays.
    return this.subscribe(
      {
        kinds: [...CHANNEL_EVENT_KINDS, KIND_CHANNEL_THREAD_SUMMARY],
        "#h": [channelId],
        limit: 1000,
        since: Math.floor(Date.now() / 1_000),
      },
      onEvent,
    );
  }

  /**
   * Subscribe to huddle lifecycle events (kinds 48100–48103) for a channel,
   * so HuddleIndicator detects active huddles without being drowned out by
   * regular channel messages. Includes the last 10 historical events.
   */
  async subscribeToHuddleEvents(
    channelId: string,
    onEvent: (event: RelayEvent) => void,
  ) {
    return this.subscribe(
      {
        kinds: [48100, 48101, 48102, 48103],
        "#h": [channelId],
        limit: 100,
      },
      onEvent,
    );
  }

  async subscribeToTypingIndicators(
    channelId: string,
    onEvent: (event: RelayEvent) => void,
  ) {
    return this.subscribe(
      {
        kinds: [KIND_TYPING_INDICATOR],
        "#h": [channelId],
        limit: 10,
        since: Math.floor(Date.now() / 1_000) - 10,
      },
      onEvent,
    );
  }

  async publishUserStatus(text: string, emoji: string): Promise<void> {
    await this.ensureConnected();
    const tags: string[][] = [["d", "general"]];
    if (emoji) tags.push(["emoji", emoji]);
    const event = await signRelayEvent({
      kind: KIND_USER_STATUS,
      content: text,
      tags,
    });
    await this.publishEvent(
      event,
      "Timed out publishing user status",
      "Failed to publish user status",
    );
  }

  /** Subscribe to kind:30315 user status events (live only, no backfill). */
  async subscribeToUserStatusUpdates(onEvent: (event: RelayEvent) => void) {
    return this.subscribe(
      { kinds: [KIND_USER_STATUS], "#d": ["general"], limit: 0 },
      onEvent,
    );
  }

  async subscribeToAllStreamMessages(onEvent: (event: RelayEvent) => void) {
    return this.subscribe(buildGlobalStreamFilter(50), onEvent);
  }

  async subscribeLive(
    filter: RelaySubscriptionFilter,
    onEvent: (event: RelayEvent) => void,
    onReady?: (readiness: LiveSubscriptionReadiness) => void,
    readinessTimeoutMs?: number,
  ) {
    return this.subscribe(filter, onEvent, onReady, readinessTimeoutMs);
  }

  /**
   * Live subscription carrying several filters on one REQ (1–10, OR-ed by
   * the relay, one admission unit). A filter's `#h` may list up to 128
   * channels in aggregate per REQ, so one call covers a whole sidebar.
   * Identical filter lists share one REQ; the returned function leaves it and
   * sends CLOSE once nobody holds it.
   */
  async subscribeLiveMany(
    filters: RelaySubscriptionFilter[],
    onEvent: (event: RelayEvent) => void,
    onReady?: (readiness: LiveSubscriptionReadiness) => void,
    readinessTimeoutMs?: number,
  ): Promise<() => Promise<void>> {
    return this.subscribeMany(filters, onEvent, onReady, readinessTimeoutMs);
  }

  async subscribeToChannelMentionEvents(
    channelId: string,
    pubkey: string,
    onEvent: (event: RelayEvent) => void,
  ) {
    return this.subscribe(
      buildChannelMentionFilter(channelId, pubkey, 50),
      onEvent,
    );
  }
}
