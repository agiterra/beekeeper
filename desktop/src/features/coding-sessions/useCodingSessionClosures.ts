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
  buildCodingSessionClosureFilter,
  foldAuthorizedCodingSessionClosures,
  parseCodingSessionClosure,
  subscribeToAcceptedCodingSessionClosures,
  type CodingSessionClosure,
} from "./lib/codingSessionClosure";

const CLOSURE_HISTORY_LIMIT = 1000;

type ClosureClient = {
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
 * The shared closure fact for every session in scope.
 *
 * This read is what tells "Open Sessions" from "Recent Sessions", so a failed
 * one is not a blank shelf — it is a confident *wrong* shelf: rows paint from
 * the persisted metadata cache while every closed session reads as open work.
 * It therefore carries the same retry and re-arm discipline as the trusted
 * ingress and create observations rather than the single best-effort read it
 * used to be, and reports its failure to the caller instead of leaving the
 * shelf to present a guess as a fact.
 */
export function useCodingSessionClosures(
  channelIds: readonly string[],
  founderPubkeysByGenesisRef: ReadonlyMap<string, string>,
  client: ClosureClient = defaultRelayClient,
): {
  closures: Map<string, CodingSessionClosure>;
  errorMessage: string | null;
} {
  const channelScope = [...new Set(channelIds)].sort().join("\u0000");
  const stableChannelIds = React.useMemo(
    () => (channelScope ? channelScope.split("\u0000") : []),
    [channelScope],
  );
  const founderScope = JSON.stringify(
    [...founderPubkeysByGenesisRef.entries()].sort(([left], [right]) =>
      left.localeCompare(right),
    ),
  );
  const stableFounderPubkeys = React.useMemo<ReadonlyMap<string, string>>(
    () => new Map(JSON.parse(founderScope) as [string, string][]),
    [founderScope],
  );
  const [events, setEvents] = React.useState<Map<string, RelayEvent>>(
    () => new Map(),
  );
  const [errorMessage, setErrorMessage] = React.useState<string | null>(null);

  React.useEffect(() => {
    let cancelled = false;
    let unsubscribeLive: (() => void) | null = null;
    let liveSubscribePending = false;
    let historyError: string | null = null;
    let liveError: string | null = null;
    const allowedChannelIds = new Set(stableChannelIds);
    setEvents(new Map());
    setErrorMessage(null);
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
        for (const event of incoming) {
          const closure = parseCodingSessionClosure(event);
          if (closure && allowedChannelIds.has(closure.channelId)) {
            next.set(event.id, event);
          }
        }
        return next;
      });
    };

    const historyController = createCodingSessionDiscoveryController({
      async load() {
        const history = await client.fetchEvents(
          buildCodingSessionClosureFilter(
            stableChannelIds,
            CLOSURE_HISTORY_LIMIT,
          ),
        );
        if (cancelled) return;
        admit(history);
      },
      onAttemptStart() {},
      onSuccess() {
        historyError = null;
        publishError();
      },
      onError(error, retry) {
        // A pending retry is not yet a failure to report: saying so would
        // flash "session state may be incomplete" across every backoff.
        historyError = retry.willRetry
          ? null
          : error instanceof Error
            ? error.message
            : "Failed to load session closures.";
        publishError();
      },
      retrySeed: `closures:${channelScope}`,
    });

    const establishLive = () => {
      if (unsubscribeLive || liveSubscribePending) return;
      liveSubscribePending = true;
      client
        .subscribeLive(
          buildCodingSessionClosureFilter(stableChannelIds, 0),
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
          // Establish the live fence before history backfill so a close or
          // reopen published during mount cannot fall between the two reads.
          historyController.request();
        })
        .catch((error: unknown) => {
          liveSubscribePending = false;
          if (cancelled) return;
          liveError =
            error instanceof Error
              ? error.message
              : "Failed to watch session closures.";
          publishError();
          // A broken live watch still leaves a useful history-only snapshot.
          historyController.request();
        });
    };

    establishLive();
    const disarm = armCodingSessionDiscoveryOnConnect(client, () => {
      if (unsubscribeLive) historyController.request();
      else establishLive();
    });
    const unsubscribeAccepted = subscribeToAcceptedCodingSessionClosures(
      (event) => admit([event]),
    );
    return () => {
      cancelled = true;
      historyController.cancel();
      unsubscribeLive?.();
      disarm();
      unsubscribeAccepted();
    };
  }, [channelScope, client, stableChannelIds]);

  const closures = React.useMemo(
    () =>
      foldAuthorizedCodingSessionClosures(
        [...events.values()],
        stableFounderPubkeys,
      ),
    [events, stableFounderPubkeys],
  );
  return React.useMemo(
    () => ({ closures, errorMessage }),
    [closures, errorMessage],
  );
}
