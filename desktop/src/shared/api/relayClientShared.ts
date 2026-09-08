import type { RelayEvent } from "@/shared/api/types";

/**
 * Observable connection state for the relay singleton.
 *
 * - `idle`         — never tried to connect yet (post-init, pre-community).
 * - `connecting`   — initial socket + AUTH handshake in flight.
 * - `connected`    — socket open and AUTH'd.
 * - `reconnecting` — socket dropped, waiting for the backoff timer.
 * - `stalled`      — socket is *open* per the WS layer but no inbound frames
 *                    for a long time (half-open socket / VPN split-brain). We
 *                    surface this so the UI can warn even though tungstenite
 *                    hasn't reported anything wrong yet.
 * - `disconnected` — final/terminal disconnect (auth rejected, community
 *                    switch, etc.) — no auto-reconnect scheduled.
 */
export type ConnectionState =
  | "idle"
  | "connecting"
  | "connected"
  | "reconnecting"
  | "stalled"
  | "disconnected";

/** True when the UI should surface a "connection lost" indicator. */
export function isRelayConnectionDegraded(state: ConnectionState): boolean {
  return (
    state === "reconnecting" || state === "stalled" || state === "disconnected"
  );
}

export type RelaySubscriptionFilter = {
  ids?: string[];
  kinds: number[];
  limit: number;
  authors?: string[];
  since?: number;
  until?: number;
} & Partial<Record<`#${string}`, string[]>>;

/** NIP-11 `max_filters`: the relay rejects a REQ carrying more than this. */
export const MAX_FILTERS_PER_REQ = 10;

/**
 * Aggregate `#h` values the relay accepts per request (WebSocket REQ and
 * `POST /query` alike): `MAX_EXPLICIT_CHANNEL_VALUES` in
 * `crates/buzz-relay/src/handlers/req.rs`, summed across every filter in the
 * request.
 */
export const MAX_CHANNEL_VALUES_PER_REQUEST = 128;

/** Number of `#h` values a filter contributes to the per-request channel cap. */
export function filterChannelValueCount(filter: RelaySubscriptionFilter) {
  return filter["#h"]?.length ?? 0;
}

type HistorySubscription = {
  mode: "history";
  events: RelayEvent[];
  resolve: (events: RelayEvent[]) => void;
  reject: (error: Error) => void;
  timeout: number;
};

type FirstEventSubscription = {
  mode: "first";
  onEvent: (event: RelayEvent) => void;
  resolve: (event: RelayEvent | null) => void;
  reject: (error: Error) => void;
  timeout: number;
};

export type LiveSubscriptionReadiness = "eose" | "closed" | "timeout";

type LiveSubscription = {
  mode: "live";
  /**
   * The NIP-01 filters this REQ carries (OR-ed by the relay, 1–10 per frame).
   * One REQ can cover many channels through a multi-value `#h`, which is how
   * the desktop keeps one live subscription per 128 channels instead of one
   * per channel.
   */
  filters: RelaySubscriptionFilter[];
  onEvent: (event: RelayEvent) => void;
  resolveReady?: (readiness: LiveSubscriptionReadiness) => void;
  lastSeenCreatedAt?: number;
  /**
   * Newest `created_at` seen per `h` tag on this subscription. A multi-`#h`
   * REQ shares one `limit`, so a reconnect cannot ask the relay for "what I
   * missed" in one filter; replay backfills each channel from its own cursor
   * instead (`relayReconnectReplay.ts`).
   */
  lastSeenByChannel?: Record<string, number>;
  /**
   * Lower bound of a reconnect backfill window that has not yet completed.
   *
   * Events on the restored live REQ advance `lastSeenCreatedAt` regardless of
   * backfill success, so after an exhausted backfill the cursor alone would
   * make the next reconnect skip the unresolved older window — silent message
   * loss. This floor is pinned when paging starts and cleared only when a
   * backfill pass completes; the next replay starts from
   * `min(pendingReplaySince, cursor window)`.
   */
  pendingReplaySince?: number;
  closedRetryAttempt?: number;
  closedRetryTimeout?: number;
};

export type PendingEvent = {
  event: RelayEvent;
  resolve: (event: RelayEvent) => void;
  reject: (error: Error) => void;
  timeout: number;
};

export type RelaySubscription =
  | HistorySubscription
  | FirstEventSubscription
  | LiveSubscription;

export function sortEvents(events: RelayEvent[]) {
  return [...events].sort((left, right) => {
    if (left.created_at !== right.created_at) {
      return left.created_at - right.created_at;
    }
    // Same (created_at, id) tiebreak as the cache sort (sortMessages) so a
    // history REQ resolves same-second events in a stable, relay-matching
    // order. Currently every consumer re-sorts downstream, but keeping the
    // two sorts on one invariant avoids a latent ordering drift.
    return left.id < right.id ? -1 : left.id > right.id ? 1 : 0;
  });
}

export function getTextPayload(message: unknown) {
  if (typeof message === "string") {
    return message;
  }

  if (
    typeof message === "object" &&
    message !== null &&
    "type" in message &&
    message.type === "Text" &&
    "data" in message &&
    typeof message.data === "string"
  ) {
    return message.data;
  }

  if (
    typeof message === "object" &&
    message !== null &&
    "Text" in message &&
    typeof message.Text === "string"
  ) {
    return message.Text;
  }

  return null;
}
