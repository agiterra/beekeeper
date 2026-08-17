import * as React from "react";

import { relayClient as defaultRelayClient } from "@/shared/api/relayClient";
import type { RelaySubscriptionFilter } from "@/shared/api/relayClientShared";
import type { RelayEvent } from "@/shared/api/types";
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
};

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
    const allowedChannelIds = new Set(stableChannelIds);
    setEvents(new Map());
    setErrorMessage(null);
    if (stableChannelIds.length === 0) return;

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
    const load = () => {
      void client
        .fetchEvents(
          buildCodingSessionClosureFilter(
            stableChannelIds,
            CLOSURE_HISTORY_LIMIT,
          ),
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
                : "Failed to load session closures.",
            );
          }
        });
    };
    void client
      .subscribeLive(
        buildCodingSessionClosureFilter(stableChannelIds, 0),
        (event) => admit([event]),
      )
      .then((unsubscribe) => {
        if (cancelled) unsubscribe();
        else {
          unsubscribeLive = unsubscribe;
          // Establish the live fence before history backfill so a close or
          // reopen published during mount cannot fall between the two reads.
          load();
        }
      })
      .catch((error: unknown) => {
        if (!cancelled) {
          setErrorMessage(
            error instanceof Error
              ? error.message
              : "Failed to watch session closures.",
          );
          // A broken live watch still leaves a useful history-only snapshot.
          load();
        }
      });
    const unsubscribeReconnect = client.subscribeToReconnects?.(load);
    const unsubscribeAccepted = subscribeToAcceptedCodingSessionClosures(
      (event) => admit([event]),
    );
    return () => {
      cancelled = true;
      unsubscribeLive?.();
      unsubscribeReconnect?.();
      unsubscribeAccepted();
    };
  }, [client, stableChannelIds]);

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
