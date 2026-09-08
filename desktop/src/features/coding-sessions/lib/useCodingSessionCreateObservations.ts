import * as React from "react";

import { relayClient as defaultRelayClient } from "@/shared/api/relayClient";
import { armCodingSessionDiscoveryOnConnect } from "./codingSessionDiscoveryArming";
import { createCodingSessionDiscoveryController } from "./codingSessionDiscoveryRetry";
import {
  buildCodingSessionCreateObservationFilter,
  buildCodingSessionCreateObservationHistoryFilters,
  CodingSessionCreateObservationStore,
  type CodingSessionUmbrellaCreateObservation,
} from "./codingSessionCreateObservations";
import {
  buildCodingSessionIngressAuthorityIdentity,
  OPEN_CODING_SESSION_INGRESS_AUTHORITY,
} from "./codingSessionIngressAuthority";
import type { CodingSessionIngressClient } from "./useTrustedCodingSessionIngress";
import type { RelaySubscriptionFilter } from "@/shared/api/relayClientShared";
import {
  fanOutObservedCodingSessionEvents,
  subscribeToObservedCodingSessionEvents,
} from "./codingSessionObservedEvents";
import type { RelayEvent } from "@/shared/api/types";

const CREATE_OBSERVATION_HISTORY_LIMIT = 1000;

export type CodingSessionCreateObservationSnapshot = {
  /** Receipt-joined create observations, earliest first. */
  observations: CodingSessionUmbrellaCreateObservation[];
  /** True while the first history load for this scope is outstanding. */
  isLoading: boolean;
  errorMessage: string | null;
  /** Identity of the (authority, channels) scope this snapshot belongs to. */
  scopeIdentity: string;
};

const EMPTY_OBSERVATIONS: CodingSessionUmbrellaCreateObservation[] = [];

function emptySnapshot(
  scopeIdentity: string,
  isLoading = false,
): CodingSessionCreateObservationSnapshot {
  return {
    observations: EMPTY_OBSERVATIONS,
    isLoading,
    errorMessage: null,
    scopeIdentity,
  };
}

/**
 * Collect the human-signed 44221 creates for a channel set, joined to the
 * executions they minted.
 *
 * A separate subscription from the trusted ingress on purpose: that hook reads
 * provider-authority-signed facts, while creates are signed by whichever member
 * founded the session and must be read from everyone. Keeping them apart is
 * what stops a human create from ever being mistaken for a provider fact.
 *
 * Trust lives in the join, not in the subscription. Every create names the
 * provider it addressed, and only that provider's own signed receipt joins it
 * to an execution — so reading widely costs nothing, while reading narrowly
 * (through this machine's run-permission list) used to cost every member the
 * founder of every session they did not start themselves.
 *
 * Fail-safe by construction: while history is still loading, and whenever the
 * join is ambiguous, this reports no observations — and no observations is
 * precisely the permissive fallback in {@link groupCodingSessionCatalog}, where
 * founder and operator stay null and nothing is gated.
 */
/**
 * The ingress client plus the bundled read this hook prefers. `relayClient`
 * provides `fetchEventsBatch`; a client without it (a test double built for
 * the trusted-ingress hook) is read one filter at a time instead.
 */
export type CodingSessionCreateObservationClient =
  CodingSessionIngressClient & {
    fetchEventsBatch?(
      filters: RelaySubscriptionFilter[],
    ): Promise<RelayEvent[]>;
  };

async function fetchCreateHistory(
  client: CodingSessionCreateObservationClient,
  filters: RelaySubscriptionFilter[],
): Promise<RelayEvent[]> {
  if (client.fetchEventsBatch) return client.fetchEventsBatch(filters);
  const pages = await Promise.all(
    filters.map((filter) => client.fetchEvents(filter)),
  );
  return pages.flat();
}

export function useCodingSessionCreateObservations(
  channelIds: readonly string[],
  client: CodingSessionCreateObservationClient = defaultRelayClient,
): CodingSessionCreateObservationSnapshot {
  const stableChannelIdentity = [...new Set(channelIds)].sort().join("\u0000");
  const stableChannelIds = React.useMemo(
    () =>
      stableChannelIdentity.length > 0
        ? stableChannelIdentity.split("\u0000")
        : [],
    [stableChannelIdentity],
  );
  // Channel membership is the read authority here, exactly as it is for the
  // display surfaces: the relay already refused these events from non-members,
  // and *which* provider may answer a given create is fenced per-create by the
  // pin that create signed. The local `allowed-bridge-pubkeys` list governs
  // what this machine runs, so consulting it here only ever blinded a member
  // to sessions founded by someone else.
  const authorityIdentity = buildCodingSessionIngressAuthorityIdentity(
    OPEN_CODING_SESSION_INGRESS_AUTHORITY,
  );
  const scopeIdentity = `${authorityIdentity}|${stableChannelIdentity}`;
  const [snapshot, setSnapshot] = React.useState(() =>
    emptySnapshot(scopeIdentity),
  );
  const storeRef = React.useRef<{
    identity: string;
    store: CodingSessionCreateObservationStore;
  } | null>(null);

  React.useEffect(() => {
    if (storeRef.current?.identity !== scopeIdentity) {
      storeRef.current = {
        identity: scopeIdentity,
        store: new CodingSessionCreateObservationStore(),
      };
    }
    const store = storeRef.current.store;
    if (stableChannelIds.length === 0) {
      setSnapshot(emptySnapshot(scopeIdentity));
      return;
    }

    let cancelled = false;
    let unsubscribeLive: (() => void) | null = null;
    let liveSubscribePending = false;
    let historyLoading = false;
    let historyError: string | null = null;
    let liveError: string | null = null;

    const publish = () => {
      if (cancelled) return;
      setSnapshot({
        observations: store.snapshot(stableChannelIds),
        isLoading: historyLoading,
        errorMessage:
          historyError && liveError
            ? `${historyError}\n${liveError}`
            : (historyError ?? liveError),
        scopeIdentity,
      });
    };
    const receiveObservedEvents = (events: readonly RelayEvent[]) => {
      if (cancelled) return;
      store.ingestRelayEvents(
        events,
        stableChannelIds,
        OPEN_CODING_SESSION_INGRESS_AUTHORITY,
      );
      publish();
    };
    const unsubscribeObserved = subscribeToObservedCodingSessionEvents(
      receiveObservedEvents,
    );

    const historyController = createCodingSessionDiscoveryController({
      async load() {
        // One filter per kind, each with its own row budget — per-turn 44224
        // volume must never be able to push a channel's creates out of the
        // newest-first window this store bootstraps from — bundled into one
        // `POST /query` rather than three REQs against the per-key burst.
        const events = await fetchCreateHistory(
          client,
          buildCodingSessionCreateObservationHistoryFilters(
            stableChannelIds,
            CREATE_OBSERVATION_HISTORY_LIMIT,
          ),
        );
        if (cancelled) return;
        store.ingestRelayEvents(
          events,
          stableChannelIds,
          OPEN_CODING_SESSION_INGRESS_AUTHORITY,
        );
        fanOutObservedCodingSessionEvents(events, receiveObservedEvents);
      },
      onAttemptStart() {
        historyLoading = true;
        publish();
      },
      onSuccess() {
        historyLoading = false;
        historyError = null;
        publish();
      },
      onError(error, retry) {
        historyLoading = retry.willRetry;
        historyError = retry.willRetry
          ? null
          : error instanceof Error
            ? error.message
            : "Failed to load coding-session create history.";
        publish();
      },
      retrySeed: `creates:${scopeIdentity}`,
    });

    const establishLive = () => {
      if (unsubscribeLive || liveSubscribePending) return;
      liveSubscribePending = true;
      client
        .subscribeLive(
          buildCodingSessionCreateObservationFilter(stableChannelIds, 0),
          (event) => {
            store.ingestRelayEvents(
              [event],
              stableChannelIds,
              OPEN_CODING_SESSION_INGRESS_AUTHORITY,
            );
            publish();
            fanOutObservedCodingSessionEvents([event], receiveObservedEvents);
          },
        )
        .then((unsubscribe) => {
          liveSubscribePending = false;
          if (cancelled) {
            unsubscribe();
            return;
          }
          unsubscribeLive = unsubscribe;
          liveError = null;
          publish();
          // Backfill only after live is fenced so a create/receipt emitted
          // while this channel is entering the sidebar cannot fall between
          // an early empty history read and a late subscription.
          historyController.request();
        })
        .catch((error) => {
          liveSubscribePending = false;
          liveError =
            error instanceof Error
              ? error.message
              : "Failed to subscribe to coding-session creates.";
          publish();
          historyController.request();
        });
    };

    publish();
    establishLive();
    const disarm = armCodingSessionDiscoveryOnConnect(client, () => {
      if (unsubscribeLive) historyController.request();
      else establishLive();
    });
    return () => {
      cancelled = true;
      historyController.cancel();
      unsubscribeLive?.();
      disarm();
      unsubscribeObserved();
    };
  }, [client, scopeIdentity, stableChannelIds]);

  // A snapshot minted for a different authority or channel set is stale scope,
  // not weaker scope: it is dropped rather than shown against the new one.
  return snapshot.scopeIdentity === scopeIdentity
    ? snapshot
    : emptySnapshot(scopeIdentity, true);
}
