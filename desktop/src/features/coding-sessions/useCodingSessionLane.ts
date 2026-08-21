import * as React from "react";

import { relayClient as defaultRelayClient } from "@/shared/api/relayClient";
import type { RelaySubscriptionFilter } from "@/shared/api/relayClientShared";
import type { RelayEvent } from "@/shared/api/types";
import { createCodingSessionDiscoveryController } from "./lib/codingSessionDiscoveryRetry";
import {
  buildCodingSessionLaneFilter,
  isCodingSessionLaneEventForChannel,
  projectCodingSessionLaneMessages,
  type CodingSessionLaneMessage,
} from "./lib/codingSessionConversationLane";

const LANE_HISTORY_LIMIT = 500;

/** Relay surface the lane hook needs; the shared relay client satisfies it. */
export type CodingSessionLaneClient = {
  fetchEvents(filter: RelaySubscriptionFilter): Promise<RelayEvent[]>;
  subscribeLive(
    filter: RelaySubscriptionFilter,
    onEvent: (event: RelayEvent) => void,
  ): Promise<() => void>;
  subscribeToReconnects?(listener: () => void): () => void;
};

export type CodingSessionLaneSnapshot = {
  messages: CodingSessionLaneMessage[];
  isLoading: boolean;
  errorMessage: string | null;
};

/**
 * One umbrella's conversation lane: the channel's kind:9 stream narrowed to
 * `["cs-session", sessionRef]` by a dedicated subscription (explicit `kinds`
 * for the p-gate; `#h` scoping preserves the community boundary). Every event
 * is re-checked on arrival against *both* halves of that filter — the exact
 * session ref and this hook's own channel id — so a relay answering either
 * narrowing loosely can never place foreign chat in the lane.
 */
export function useCodingSessionLane(
  channelId: string | null,
  sessionRef: string | null,
  client: CodingSessionLaneClient = defaultRelayClient,
): CodingSessionLaneSnapshot {
  const [eventsById, setEventsById] = React.useState<Map<string, RelayEvent>>(
    () => new Map(),
  );
  const [isLoading, setIsLoading] = React.useState(false);
  const [errorMessage, setErrorMessage] = React.useState<string | null>(null);

  React.useEffect(() => {
    setEventsById(new Map());
    setErrorMessage(null);
    if (!channelId || !sessionRef) {
      setIsLoading(false);
      return;
    }
    let cancelled = false;
    let unsubscribeLive: (() => void) | null = null;
    setIsLoading(true);

    const admit = (incoming: readonly RelayEvent[]) => {
      if (cancelled) return;
      const admissible = incoming.filter((event) =>
        // `channelId`/`sessionRef` are the effect's captured values, so a
        // late callback from a previous channel is rejected too.
        isCodingSessionLaneEventForChannel(event, channelId, sessionRef),
      );
      if (admissible.length === 0) return;
      setEventsById((current) => {
        const next = new Map(current);
        for (const event of admissible) next.set(event.id, event);
        return next;
      });
    };

    // Entering a session view issues a burst of REQs, so this history frame is
    // a routine loser of the relay's per-pubkey admission race. A bare catch
    // would strand the lane empty until the next reconnect; the shared
    // controller backs off behind the rate-limit gate and converges instead.
    const historyController = createCodingSessionDiscoveryController({
      async load() {
        const events = await client.fetchEvents({
          ...buildCodingSessionLaneFilter(channelId, sessionRef),
          limit: LANE_HISTORY_LIMIT,
        });
        if (cancelled) return;
        admit(events);
      },
      onAttemptStart() {
        if (!cancelled) setIsLoading(true);
      },
      onSuccess() {
        if (cancelled) return;
        setErrorMessage(null);
        setIsLoading(false);
      },
      onError(error, retry) {
        if (cancelled) return;
        setIsLoading(retry.willRetry);
        setErrorMessage(
          retry.willRetry
            ? null
            : error instanceof Error
              ? error.message
              : "Failed to load the session conversation.",
        );
      },
      retrySeed: `lane:${channelId}:${sessionRef}`,
    });

    void client
      .subscribeLive(
        { ...buildCodingSessionLaneFilter(channelId, sessionRef), limit: 0 },
        (event) => admit([event]),
      )
      .then((unsubscribe) => {
        if (cancelled) {
          unsubscribe();
          return;
        }
        unsubscribeLive = unsubscribe;
      })
      .catch((error: unknown) => {
        if (cancelled) return;
        setErrorMessage(
          error instanceof Error
            ? error.message
            : "Failed to subscribe to the session conversation.",
        );
      });
    historyController.request();
    const unsubscribeReconnect = client.subscribeToReconnects?.(() => {
      // A reconnect may have dropped live events; the history refetch is
      // idempotent because events merge by id.
      historyController.request();
    });

    return () => {
      cancelled = true;
      historyController.cancel();
      unsubscribeLive?.();
      unsubscribeReconnect?.();
    };
  }, [channelId, client, sessionRef]);

  const messages = React.useMemo(
    () =>
      sessionRef
        ? projectCodingSessionLaneMessages([...eventsById.values()], sessionRef)
        : [],
    [eventsById, sessionRef],
  );

  return React.useMemo(
    () => ({ messages, isLoading, errorMessage }),
    [errorMessage, isLoading, messages],
  );
}
