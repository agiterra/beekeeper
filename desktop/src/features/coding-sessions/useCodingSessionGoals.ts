import * as React from "react";

import { relayClient as defaultRelayClient } from "@/shared/api/relayClient";
import type { RelaySubscriptionFilter } from "@/shared/api/relayClientShared";
import type { RelayEvent } from "@/shared/api/types";
import {
  buildCodingSessionGoalFilter,
  foldLatestCodingSessionGoalsByFounder,
  type CodingSessionGoal,
} from "./lib/codingSessionGoal";

const GOAL_HISTORY_LIMIT = 1000;

type GoalClient = {
  fetchEvents(filter: RelaySubscriptionFilter): Promise<RelayEvent[]>;
  subscribeLive(
    filter: RelaySubscriptionFilter,
    onEvent: (event: RelayEvent) => void,
  ): Promise<() => void>;
  subscribeToReconnects?(listener: () => void): () => void;
};

export function useCodingSessionGoals(
  channelIds: readonly string[],
  client: GoalClient = defaultRelayClient,
): { goals: Map<string, CodingSessionGoal>; errorMessage: string | null } {
  const scope = [...new Set(channelIds)].sort().join("\u0000");
  const stableChannelIds = React.useMemo(
    () => (scope ? scope.split("\u0000") : []),
    [scope],
  );
  const [events, setEvents] = React.useState<Map<string, RelayEvent>>(
    () => new Map(),
  );
  const [errorMessage, setErrorMessage] = React.useState<string | null>(null);

  React.useEffect(() => {
    let cancelled = false;
    let unsubscribeLive: (() => void) | null = null;
    setEvents(new Map());
    setErrorMessage(null);
    if (stableChannelIds.length === 0) return;

    const admit = (incoming: readonly RelayEvent[]) => {
      if (cancelled) return;
      setEvents((current) => {
        const next = new Map(current);
        for (const event of incoming) next.set(event.id, event);
        return next;
      });
    };
    const load = () => {
      void client
        .fetchEvents(
          buildCodingSessionGoalFilter(stableChannelIds, GOAL_HISTORY_LIMIT),
        )
        .then((history) => {
          admit(history);
          if (!cancelled) setErrorMessage(null);
        })
        .catch((error: unknown) => {
          if (!cancelled) {
            setErrorMessage(
              error instanceof Error
                ? error.message
                : "Failed to load session goals.",
            );
          }
        });
    };
    load();
    void client
      .subscribeLive(
        buildCodingSessionGoalFilter(stableChannelIds, 0),
        (event) => admit([event]),
      )
      .then((unsubscribe) => {
        if (cancelled) unsubscribe();
        else unsubscribeLive = unsubscribe;
      })
      .catch((error: unknown) => {
        if (!cancelled) {
          setErrorMessage(
            error instanceof Error
              ? error.message
              : "Failed to watch session goals.",
          );
        }
      });
    const unsubscribeReconnect = client.subscribeToReconnects?.(load);
    return () => {
      cancelled = true;
      unsubscribeLive?.();
      unsubscribeReconnect?.();
    };
  }, [client, stableChannelIds]);

  const goals = React.useMemo(
    () => foldLatestCodingSessionGoalsByFounder([...events.values()]),
    [events],
  );
  return React.useMemo(() => ({ goals, errorMessage }), [errorMessage, goals]);
}

export function codingSessionGoalKey(
  channelId: string,
  sessionRef: string,
  founderPubkey: string,
) {
  return `${channelId}\u0000${sessionRef}\u0000${founderPubkey.toLowerCase()}`;
}
