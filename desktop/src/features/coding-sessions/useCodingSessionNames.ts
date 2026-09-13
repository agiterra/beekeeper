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
  fetchEventsCoalesced(filter: RelaySubscriptionFilter): Promise<RelayEvent[]>;
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
  /** History failure or incomplete coverage; distinct from a failed live watch. */
  readErrorMessage: string | null;
  /** Recheck history and retry a failed live watch without replacing this scope. */
  refresh: () => void;
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
  const [readErrorMessage, setReadErrorMessage] = React.useState<string | null>(
    null,
  );
  const [readScope, setReadScope] = React.useState(scope);
  const refreshRead = React.useRef<(() => void) | null>(null);
  const refresh = React.useCallback(() => refreshRead.current?.(), []);

  React.useEffect(() => {
    let cancelled = false;
    let unsubscribeLive: (() => void) | null = null;
    let liveSubscribePending = false;
    let historyError: string | null = null;
    let liveError: string | null = null;
    let historyAtLimit = false;
    setReadScope(scope);
    setReadErrorMessage(null);
    setEvents(new Map());
    setErrorMessage(null);
    // A new scope is a new read; nothing to read is the one scope that
    // starts settled.
    setResolved(stableChannelIds.length === 0);
    if (stableChannelIds.length === 0) return;

    const publishError = () => {
      if (cancelled) return;
      const readError =
        historyError ??
        (historyAtLimit
          ? `Only the latest ${NAME_HISTORY_LIMIT} name records were read; older session names may be missing.`
          : null);
      setReadErrorMessage(readError);
      setErrorMessage(
        readError && liveError
          ? `${readError}\n${liveError}`
          : (readError ?? liveError),
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
        const history = await client.fetchEventsCoalesced(
          buildCodingSessionNameFilter(stableChannelIds, NAME_HISTORY_LIMIT),
        );
        if (cancelled) return;
        admit(history);
        return history.length;
      },
      onAttemptStart() {
        if (!cancelled) setResolved(false);
      },
      onSuccess(count) {
        historyError = null;
        historyAtLimit = (count ?? 0) >= NAME_HISTORY_LIMIT;
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
      if (cancelled || unsubscribeLive || liveSubscribePending) return;
      liveSubscribePending = true;
      client
        .subscribeLive(
          // Bounded replay closes the gap after the independent HTTP read.
          // Subscription resolution can be a timeout, not server readiness.
          buildCodingSessionNameFilter(stableChannelIds, NAME_HISTORY_LIMIT),
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

    const refreshScope = () => {
      if (cancelled) return;
      historyController.request();
      establishLive();
    };
    refreshRead.current = refreshScope;
    refreshScope();
    const disarm = armCodingSessionDiscoveryOnConnect(client, refreshScope);
    const unsubscribeAccepted = subscribeToAcceptedCodingSessionNames(
      (event) => {
        if (
          event.tags.some(
            (tag) => tag[0] === "h" && stableChannelIds.includes(tag[1]),
          )
        )
          admit([event]);
      },
      client,
    );
    return () => {
      cancelled = true;
      if (refreshRead.current === refreshScope) refreshRead.current = null;
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
    () => ({
      names,
      errorMessage: readScope === scope ? errorMessage : null,
      resolved: readScope === scope && resolved,
      readErrorMessage: readScope === scope ? readErrorMessage : null,
      refresh,
    }),
    [
      errorMessage,
      names,
      readErrorMessage,
      readScope,
      refresh,
      resolved,
      scope,
    ],
  );
}
