import * as React from "react";

import { relayClient as defaultRelayClient } from "@/shared/api/relayClient";
import type { RelaySubscriptionFilter } from "@/shared/api/relayClientShared";
import type { RelayEvent } from "@/shared/api/types";
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

    const loadHistory = () =>
      client
        .fetchEvents({
          ...buildCodingSessionLaneFilter(channelId, sessionRef),
          limit: LANE_HISTORY_LIMIT,
        })
        .then((events) => {
          admit(events);
          if (!cancelled) setErrorMessage(null);
        })
        .catch((error: unknown) => {
          if (cancelled) return;
          setErrorMessage(
            error instanceof Error
              ? error.message
              : "Failed to load the session conversation.",
          );
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
    void loadHistory().finally(() => {
      if (!cancelled) setIsLoading(false);
    });
    const unsubscribeReconnect = client.subscribeToReconnects?.(() => {
      // A reconnect may have dropped live events; the history refetch is
      // idempotent because events merge by id.
      void loadHistory();
    });

    return () => {
      cancelled = true;
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
