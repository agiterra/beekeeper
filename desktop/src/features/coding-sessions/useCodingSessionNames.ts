import * as React from "react";

import { relayClient as defaultRelayClient } from "@/shared/api/relayClient";
import type {
  ConnectionState,
  RelaySubscriptionFilter,
} from "@/shared/api/relayClientShared";
import type { RelayEvent } from "@/shared/api/types";
import { armCodingSessionDiscoveryOnConnect } from "./lib/codingSessionDiscoveryArming";
import { createCodingSessionDiscoveryController } from "./lib/codingSessionDiscoveryRetry";
import {
  buildCodingSessionNameFilter,
  foldLatestCodingSessionNamesByFounder,
  subscribeToAcceptedCodingSessionNames,
  type CodingSessionName,
} from "./lib/codingSessionName";

const NAME_HISTORY_LIMIT = 1000;

type NameClient = {
  fetchEvents(filter: RelaySubscriptionFilter): Promise<RelayEvent[]>;
  subscribeLive(
    filter: RelaySubscriptionFilter,
    onEvent: (event: RelayEvent) => void,
  ): Promise<() => void>;
  subscribeToReconnects?(listener: () => void): () => void;
  subscribeToConnectionState?(
    listener: (state: ConnectionState) => void,
  ): () => void;
};

/**
 * The durable human-authored name for every session in scope.
 *
 * Same read discipline as {@link useCodingSessionClosures}, and for the same
 * reason: a name that never loads does not leave a gap on the shelf, it leaves
 * the provider's generic label sitting where the person's own words belong.
 *
 * `resolved` mirrors {@link useCodingSessionGoals}: true only once the history
 * read settled — resolved *or* rejected — so a field that publishes a 44229
 * can refuse to do so over a wire name it has not read yet. It says nothing
 * about whether a name was found.
 */
export function useCodingSessionNames(
  channelIds: readonly string[],
  client: NameClient = defaultRelayClient,
): {
  names: Map<string, CodingSessionName>;
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
    let liveSubscribePending = false;
    let historyError: string | null = null;
    let liveError: string | null = null;
    setEvents(new Map());
    setErrorMessage(null);
    // A new scope is a new read; nothing to read is the one scope that
    // starts settled.
    setResolved(stableChannelIds.length === 0);
    if (stableChannelIds.length === 0) return;

    const publishError = () => {
      if (cancelled) return;
      setErrorMessage(
        historyError && liveError
          ? `${historyError}\n${liveError}`
          : (historyError ?? liveError),
      );
    };
    const admit = (incoming: readonly RelayEvent[]) => {
      if (cancelled) return;
      setEvents((current) => {
        const next = new Map(current);
        for (const event of incoming) next.set(event.id, event);
        return next;
      });
    };

    const historyController = createCodingSessionDiscoveryController({
      async load() {
        const history = await client.fetchEvents(
          buildCodingSessionNameFilter(stableChannelIds, NAME_HISTORY_LIMIT),
        );
        if (cancelled) return;
        admit(history);
      },
      onAttemptStart() {},
      onSuccess() {
        historyError = null;
        publishError();
        if (!cancelled) setResolved(true);
      },
      onError(error, retry) {
        // A scheduled retry is not yet a failure worth reporting.
        historyError = retry.willRetry
          ? null
          : error instanceof Error
            ? error.message
            : "Failed to load session names.";
        publishError();
        // A final failure is a settled read: the relay answered, badly.
        if (!cancelled && !retry.willRetry) setResolved(true);
      },
      retrySeed: `names:${scope}`,
    });

    const establishLive = () => {
      if (unsubscribeLive || liveSubscribePending) return;
      liveSubscribePending = true;
      client
        .subscribeLive(
          buildCodingSessionNameFilter(stableChannelIds, 0),
          (event) => admit([event]),
        )
        .then((unsubscribe) => {
          liveSubscribePending = false;
          if (cancelled) {
            unsubscribe();
            return;
          }
          unsubscribeLive = unsubscribe;
          liveError = null;
          publishError();
          // The live fence comes first; this history read then closes the
          // channel-add window without missing a name published in between.
          historyController.request();
        })
        .catch((error: unknown) => {
          liveSubscribePending = false;
          if (cancelled) return;
          liveError =
            error instanceof Error
              ? error.message
              : "Failed to watch session names.";
          publishError();
          // Degrade to history-only discovery when live setup fails.
          historyController.request();
        });
    };

    establishLive();
    const disarm = armCodingSessionDiscoveryOnConnect(client, () => {
      if (unsubscribeLive) historyController.request();
      else establishLive();
    });
    const unsubscribeAccepted = subscribeToAcceptedCodingSessionNames((event) =>
      admit([event]),
    );
    return () => {
      cancelled = true;
      historyController.cancel();
      unsubscribeLive?.();
      disarm();
      unsubscribeAccepted();
    };
  }, [client, scope, stableChannelIds]);

  const names = React.useMemo(
    () => foldLatestCodingSessionNamesByFounder([...events.values()]),
    [events],
  );
  return React.useMemo(
    () => ({ names, errorMessage, resolved }),
    [errorMessage, names, resolved],
  );
}
