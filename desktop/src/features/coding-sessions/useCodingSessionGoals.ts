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
  fetchEventsCoalesced(filter: RelaySubscriptionFilter): Promise<RelayEvent[]>;
  subscribeLive(
    filter: RelaySubscriptionFilter,
    onEvent: (event: RelayEvent) => void,
  ): Promise<() => void>;
  subscribeToReconnects?(listener: () => void): () => void;
};

/**
 * Read this channel's published mission goals, and say which of three things
 * is true about the read itself.
 *
 * L4.1, seen live 2026-09-01 at 12:36: `No accepted mission goal published.`
 * over a session whose goal had been signed at launch. Three different facts
 * reached that one sentence — there is no record, the reader has not settled
 * yet, and the reader errored — and only the first of them is what the
 * sentence claims. `errorMessage` already existed and *nothing read it*.
 *
 * So the reader states its own condition:
 *
 * - `resolved` is false on the first render and stays false while the history
 *   fetch is in flight. It resets to false whenever the channel scope changes,
 *   because a settled read of the previous channel says nothing about this one.
 * - An error **settles** the fetch: `resolved` is true and `errorMessage`
 *   carries the reader's own message. "We tried and failed" is a resolved
 *   condition, and a caller that treated it as still-loading would spin
 *   forever over a relay that already answered.
 * - With no channel ids at all, `resolved` is true: there is nothing to read,
 *   so the read is as finished as it will ever be, and the caller may
 *   truthfully say the session has no published goal.
 *
 * The live subscription is deliberately **not** part of settling. A goal that
 * is already on the relay is readable whether or not the watch attached, and
 * gating the sentence on the watch would leave the surface unresolved over a
 * goal it had in hand.
 */
export function useCodingSessionGoals(
  channelIds: readonly string[],
  client: GoalClient = defaultRelayClient,
): {
  goals: Map<string, CodingSessionGoal>;
  errorMessage: string | null;
  /** True only once the history fetch settled — resolved *or* rejected. */
  resolved: boolean;
} {
  const scope = [...new Set(channelIds)].sort().join("\u0000");
  const stableChannelIds = React.useMemo(
    () => (scope ? scope.split("\u0000") : []),
    [scope],
  );
  const [events, setEvents] = React.useState<Map<string, RelayEvent>>(
    () => new Map(),
  );
  const [errorMessage, setErrorMessage] = React.useState<string | null>(null);
  const [resolved, setResolved] = React.useState(false);

  React.useEffect(() => {
    let cancelled = false;
    let unsubscribeLive: (() => void) | null = null;
    setEvents(new Map());
    setErrorMessage(null);
    // A new scope is a new read: whatever the last one settled says nothing
    // about this one. Nothing to read is the one scope that starts settled.
    setResolved(stableChannelIds.length === 0);
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
        .fetchEventsCoalesced(
          buildCodingSessionGoalFilter(stableChannelIds, GOAL_HISTORY_LIMIT),
        )
        .then((history) => {
          admit(history);
          if (cancelled) return;
          setErrorMessage(null);
          setResolved(true);
        })
        .catch((error: unknown) => {
          if (cancelled) return;
          setErrorMessage(
            error instanceof Error
              ? error.message
              : "Failed to load session goals.",
          );
          // A refusal is an answer. The read is over either way.
          setResolved(true);
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
  return React.useMemo(
    () => ({ goals, errorMessage, resolved }),
    [errorMessage, goals, resolved],
  );
}

export function codingSessionGoalKey(
  channelId: string,
  sessionRef: string,
  founderPubkey: string,
) {
  return `${channelId}\u0000${sessionRef}\u0000${founderPubkey.toLowerCase()}`;
}
