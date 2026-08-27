/**
 * Minimal Nostr client with NIP-01 queries and NIP-42 AUTH.
 *
 * Uses NIP-07 when a browser extension is available, with an ephemeral
 * page-lifetime identity as the fallback for read-only queries on open relays.
 */

import { makeAuthEvent } from "nostr-tools/nip42";
// Relative, not the "@/" alias: the coding-session observer tests run this
// module under `node --test`, which has no bundler to resolve the alias.
import { type SignedNostrEvent, signNostrEvent } from "./nostr-signer.ts";

export interface NostrFilter {
  ids?: string[];
  authors?: string[];
  kinds?: number[];
  since?: number;
  until?: number;
  limit?: number;
  [tag: `#${string}`]: string[] | undefined;
}

export type NostrEvent = SignedNostrEvent;

const QUERY_TIMEOUT_MS = 10_000;

/**
 * Open a WebSocket to `wsUrl`, authenticate via NIP-42 if challenged,
 * send a REQ with the given filter, collect EVENTs until EOSE, then
 * close and return them.
 */
export function queryEvents(
  wsUrl: string,
  filter: NostrFilter,
): Promise<NostrEvent[]> {
  return new Promise((resolve, reject) => {
    const events: NostrEvent[] = [];
    const subId = `q-${Date.now().toString(36)}`;
    let settled = false;
    let reqSent = false;
    let authEventId: string | null = null;
    let unauthenticatedReqTimer: ReturnType<typeof setTimeout> | null = null;

    const ws = new WebSocket(wsUrl);

    const timeout = setTimeout(() => {
      if (!settled) {
        settled = true;
        ws.close();
        reject(new Error(`Relay query timed out after ${QUERY_TIMEOUT_MS}ms`));
      }
    }, QUERY_TIMEOUT_MS);

    const cleanup = () => {
      clearTimeout(timeout);
      if (unauthenticatedReqTimer) {
        clearTimeout(unauthenticatedReqTimer);
      }
      try {
        ws.close();
      } catch {
        // ignore
      }
    };

    const sendReq = () => {
      if (!reqSent) {
        reqSent = true;
        ws.send(JSON.stringify(["REQ", subId, filter]));
      }
    };

    ws.addEventListener("open", () => {
      // Wait briefly for an AUTH challenge before sending REQ.
      // Buzz relays always send AUTH, but other relays may not.
      unauthenticatedReqTimer = setTimeout(() => sendReq(), 100);
    });

    ws.addEventListener("message", async (msg) => {
      let data: unknown;
      try {
        data = JSON.parse(String(msg.data));
      } catch {
        return;
      }
      if (!Array.isArray(data)) return;

      const [type] = data;

      if (type === "AUTH" && typeof data[1] === "string") {
        // NIP-42: relay sent an AUTH challenge — sign and respond.
        if (unauthenticatedReqTimer) {
          clearTimeout(unauthenticatedReqTimer);
          unauthenticatedReqTimer = null;
        }
        const challenge = data[1];
        const template = makeAuthEvent(wsUrl, challenge);
        try {
          const signed = await signNostrEvent(template);
          if (settled) return;
          authEventId = signed.id;
          ws.send(JSON.stringify(["AUTH", signed]));
        } catch (error) {
          if (!settled) {
            settled = true;
            cleanup();
            reject(
              error instanceof Error
                ? error
                : new Error("Failed to sign relay authentication."),
            );
          }
        }
        return;
      }

      if (type === "OK" && data[1] === authEventId) {
        if (data[2] === true) {
          sendReq();
        } else if (!settled) {
          settled = true;
          cleanup();
          reject(
            new Error(
              typeof data[3] === "string"
                ? data[3]
                : "Relay authentication failed.",
            ),
          );
        }
        return;
      }

      if (type === "EVENT" && data[1] === subId && data[2]) {
        events.push(data[2] as NostrEvent);
      } else if (type === "EOSE" && data[1] === subId) {
        if (!settled) {
          settled = true;
          cleanup();
          resolve(events);
        }
      } else if (type === "CLOSED" && data[1] === subId) {
        // Subscription was rejected (e.g. auth failed).
        if (!settled) {
          settled = true;
          cleanup();
          const reason =
            typeof data[2] === "string"
              ? data[2]
              : "subscription closed by relay";
          reject(new Error(reason));
        }
      } else if (type === "NOTICE") {
        // Informational notice from relay — ignore for now.
      }
    });

    ws.addEventListener("error", () => {
      if (!settled) {
        settled = true;
        cleanup();
        reject(new Error("WebSocket connection failed"));
      }
    });

    ws.addEventListener("close", () => {
      if (!settled) {
        settled = true;
        clearTimeout(timeout);
        resolve(events);
      }
    });
  });
}

/** How the socket behind a long-lived subscription is doing right now. */
export type RelaySubscriptionState = "connecting" | "open" | "error" | "closed";

/** Callbacks and policy for {@link subscribeEvents}. */
export interface SubscribeEventsOptions {
  /** The relay finished replaying stored events for the filter at this index. */
  onEose?: (filterIndex: number) => void;
  /**
   * The relay refused a subscription (`CLOSED`), or the socket failed.
   * `filterIndex` is null for socket-level failures.
   */
  onClosed?: (reason: string, filterIndex: number | null) => void;
  /** Every transition of the underlying socket. */
  onStateChange?: (state: RelaySubscriptionState) => void;
  /**
   * Page size used when re-subscribing after a socket loss.
   *
   * A live filter carries `limit: 0` — "send nothing stored, only what is
   * new" — which would make the gap-filling `since` on a replay meaningless
   * and silently drop everything published while the socket was down. When
   * this is set, a replay send borrows it for filters whose limit is 0.
   */
  replayLimit?: number;
  /** Close the socket once every live filter has reported EOSE. */
  closeOnEose?: boolean;
  /** Base reconnect delay; doubles per consecutive failure, capped at 30s. */
  reconnectDelayMs?: number;
}

const DEFAULT_RECONNECT_DELAY_MS = 1_000;
const MAX_RECONNECT_DELAY_MS = 30_000;
/** Replays overlap by this much, because `since` has second granularity. */
const REPLAY_OVERLAP_SECONDS = 5;
const AUTH_GRACE_MS = 100;

/**
 * Open a long-lived REQ per filter and keep it open past EOSE.
 *
 * Unlike {@link queryEvents} this does not resolve at EOSE: the socket stays
 * up, streams what the relay publishes next, and re-subscribes after a drop
 * with `since = lastSeen - 5s` so events published during the outage are
 * replayed rather than silently lost. NIP-42 AUTH is answered the same way.
 *
 * One REQ per filter, never one REQ with many filters: relays apply `limit`
 * per filter, but callers still need to know *which* filter filled its page,
 * and a shared subscription id would make that unknowable.
 *
 * @returns an unsubscribe function; calling it stops all reconnection.
 */
export function subscribeEvents(
  wsUrl: string,
  filters: readonly NostrFilter[],
  onEvent: (event: NostrEvent, filterIndex: number) => void,
  options: SubscribeEventsOptions = {},
): () => void {
  const lastSeenAt: (number | null)[] = filters.map(() => null);
  const refused = filters.map(() => false);
  const subPrefix = `s${Date.now().toString(36)}${Math.floor(
    Math.random() * 1e6,
  ).toString(36)}`;
  const subIdFor = (index: number) => `${subPrefix}-${index}`;
  const indexOfSub = (subId: string) => {
    const index = Number(subId.slice(subPrefix.length + 1));
    return subId.startsWith(`${subPrefix}-`) && Number.isInteger(index)
      ? index
      : -1;
  };

  let stopped = false;
  let socket: WebSocket | null = null;
  let reconnectTimer: ReturnType<typeof setTimeout> | null = null;
  let consecutiveFailures = 0;

  const wireFilter = (index: number): NostrFilter => {
    const base = filters[index];
    const seen = lastSeenAt[index];
    if (seen === null) return base;
    const replay: NostrFilter = {
      ...base,
      since: seen - REPLAY_OVERLAP_SECONDS,
    };
    if (base.limit === 0 && options.replayLimit !== undefined) {
      replay.limit = options.replayLimit;
    }
    return replay;
  };

  const scheduleReconnect = () => {
    if (stopped || reconnectTimer !== null) return;
    if (refused.every((value) => value)) {
      options.onStateChange?.("closed");
      return;
    }
    const base = options.reconnectDelayMs ?? DEFAULT_RECONNECT_DELAY_MS;
    const delay = Math.min(
      MAX_RECONNECT_DELAY_MS,
      base * 2 ** Math.min(consecutiveFailures, 5),
    );
    consecutiveFailures += 1;
    reconnectTimer = setTimeout(() => {
      reconnectTimer = null;
      connect();
    }, delay);
  };

  function connect() {
    if (stopped) return;
    options.onStateChange?.("connecting");

    let ws: WebSocket;
    try {
      ws = new WebSocket(wsUrl);
    } catch (error) {
      options.onClosed?.(
        error instanceof Error ? error.message : "WebSocket failed to open",
        null,
      );
      options.onStateChange?.("error");
      scheduleReconnect();
      return;
    }
    socket = ws;

    let reqSent = false;
    let authEventId: string | null = null;
    let eoseSeen = filters.map(() => false);
    let graceTimer: ReturnType<typeof setTimeout> | null = null;

    const sendReqs = () => {
      if (reqSent || stopped || ws.readyState !== 1) return;
      reqSent = true;
      eoseSeen = filters.map(() => false);
      consecutiveFailures = 0;
      for (let index = 0; index < filters.length; index += 1) {
        if (refused[index]) continue;
        ws.send(JSON.stringify(["REQ", subIdFor(index), wireFilter(index)]));
      }
      options.onStateChange?.("open");
    };

    const closeSocket = (state: RelaySubscriptionState) => {
      if (graceTimer !== null) clearTimeout(graceTimer);
      try {
        ws.close();
      } catch {
        // The socket is already gone; nothing to close.
      }
      if (socket === ws) socket = null;
      options.onStateChange?.(state);
    };

    ws.addEventListener("open", () => {
      // Buzz relays always challenge; other relays never do. Give AUTH a
      // moment before falling back to an unauthenticated REQ.
      graceTimer = setTimeout(() => sendReqs(), AUTH_GRACE_MS);
    });

    ws.addEventListener("message", async (message: MessageEvent) => {
      let data: unknown;
      try {
        data = JSON.parse(String(message.data));
      } catch {
        return;
      }
      if (!Array.isArray(data)) return;
      const [type] = data;

      if (type === "AUTH" && typeof data[1] === "string") {
        if (graceTimer !== null) {
          clearTimeout(graceTimer);
          graceTimer = null;
        }
        try {
          const signed = await signNostrEvent(makeAuthEvent(wsUrl, data[1]));
          if (stopped || ws.readyState !== 1) return;
          authEventId = signed.id;
          ws.send(JSON.stringify(["AUTH", signed]));
        } catch (error) {
          options.onClosed?.(
            error instanceof Error
              ? error.message
              : "Failed to sign relay authentication.",
            null,
          );
          closeSocket("error");
          scheduleReconnect();
        }
        return;
      }

      if (type === "OK" && data[1] === authEventId) {
        if (data[2] === true) {
          sendReqs();
        } else {
          options.onClosed?.(
            typeof data[3] === "string"
              ? data[3]
              : "Relay authentication failed.",
            null,
          );
          closeSocket("error");
          scheduleReconnect();
        }
        return;
      }

      if (type === "EVENT" && typeof data[1] === "string" && data[2]) {
        const index = indexOfSub(data[1]);
        if (index < 0 || index >= filters.length) return;
        const event = data[2] as NostrEvent;
        const seen = lastSeenAt[index];
        if (seen === null || event.created_at > seen) {
          lastSeenAt[index] = event.created_at;
        }
        onEvent(event, index);
        return;
      }

      if (type === "EOSE" && typeof data[1] === "string") {
        const index = indexOfSub(data[1]);
        if (index < 0 || index >= filters.length) return;
        eoseSeen[index] = true;
        options.onEose?.(index);
        if (
          options.closeOnEose &&
          eoseSeen.every((value, at) => value || refused[at])
        ) {
          stopped = true;
          closeSocket("closed");
        }
        return;
      }

      if (type === "CLOSED" && typeof data[1] === "string") {
        const index = indexOfSub(data[1]);
        if (index < 0 || index >= filters.length) return;
        // The relay refused this subscription. Re-sending it on every
        // reconnect would be a hot loop against a standing refusal.
        refused[index] = true;
        options.onClosed?.(
          typeof data[2] === "string"
            ? data[2]
            : "subscription closed by relay",
          index,
        );
        if (refused.every((value) => value)) {
          stopped = true;
          closeSocket("closed");
        }
      }
    });

    ws.addEventListener("error", () => {
      if (stopped) return;
      options.onClosed?.("WebSocket connection failed", null);
      closeSocket("error");
      scheduleReconnect();
    });

    ws.addEventListener("close", () => {
      if (graceTimer !== null) clearTimeout(graceTimer);
      if (socket === ws) socket = null;
      if (stopped) return;
      options.onStateChange?.("error");
      scheduleReconnect();
    });
  }

  connect();

  return () => {
    if (stopped && socket === null) return;
    stopped = true;
    if (reconnectTimer !== null) {
      clearTimeout(reconnectTimer);
      reconnectTimer = null;
    }
    const ws = socket;
    socket = null;
    if (!ws) return;
    try {
      if (ws.readyState === 1) {
        for (let index = 0; index < filters.length; index += 1) {
          if (refused[index]) continue;
          ws.send(JSON.stringify(["CLOSE", subIdFor(index)]));
        }
      }
      ws.close();
    } catch {
      // Already closed.
    }
    options.onStateChange?.("closed");
  };
}
