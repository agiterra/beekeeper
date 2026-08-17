import * as React from "react";

import { useGlobalAgentConfig } from "@/features/agents/useGlobalAgentConfig";
import { relayClient as defaultRelayClient } from "@/shared/api/relayClient";
import { createCodingSessionDiscoveryController } from "./codingSessionDiscoveryRetry";
import {
  buildCodingSessionCreateObservationFilter,
  CodingSessionCreateObservationStore,
  type CodingSessionUmbrellaCreateObservation,
} from "./codingSessionCreateObservations";
import {
  buildCodingSessionIngressAuthorityIdentity,
  resolveCodingSessionIngressAuthority,
} from "./codingSessionIngressAuthority";
import type { CodingSessionIngressClient } from "./useTrustedCodingSessionIngress";

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
 * provider-authority-signed facts and narrows its filter to the allowlist,
 * while creates are signed by whichever member founded the session and must be
 * read from everyone. Keeping them apart is what stops a human create from
 * ever being mistaken for a provider fact.
 *
 * Fail-safe by construction: while the config or history is still loading, and
 * whenever the provider authority is unusable (no trusted receipts means no
 * join), this reports no observations — and no observations is precisely the
 * permissive fallback in {@link groupCodingSessionCatalog}, where founder and
 * operator stay null and nothing is gated.
 */
export function useCodingSessionCreateObservations(
  channelIds: readonly string[],
  client: CodingSessionIngressClient = defaultRelayClient,
): CodingSessionCreateObservationSnapshot {
  const stableChannelIdentity = [...new Set(channelIds)].sort().join("\u0000");
  const stableChannelIds = React.useMemo(
    () =>
      stableChannelIdentity.length > 0
        ? stableChannelIdentity.split("\u0000")
        : [],
    [stableChannelIdentity],
  );
  const { globalConfig, isLoading: isConfigLoading } = useGlobalAgentConfig();
  const authority = React.useMemo(
    () =>
      resolveCodingSessionIngressAuthority(
        globalConfig["allowed-bridge-pubkeys"],
      ),
    [globalConfig],
  );
  const authorityIdentity = React.useMemo(
    () => buildCodingSessionIngressAuthorityIdentity(authority),
    [authority],
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
    if (isConfigLoading) {
      setSnapshot(emptySnapshot(scopeIdentity, true));
      return;
    }
    // Without a usable provider authority no receipt is trustworthy, so no
    // create can be joined to an execution. Reading creates anyway would only
    // produce claims this client cannot bind to anything.
    if (authority.state !== "valid" || stableChannelIds.length === 0) {
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

    const historyController = createCodingSessionDiscoveryController({
      async load() {
        const events = await client.fetchEvents(
          buildCodingSessionCreateObservationFilter(
            stableChannelIds,
            CREATE_OBSERVATION_HISTORY_LIMIT,
          ),
        );
        if (cancelled) return;
        store.ingestRelayEvents(events, stableChannelIds, authority);
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
            store.ingestRelayEvents([event], stableChannelIds, authority);
            publish();
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
    const unsubscribeReconnect = client.subscribeToReconnects?.(() => {
      if (unsubscribeLive) historyController.request();
      else establishLive();
    });
    return () => {
      cancelled = true;
      historyController.cancel();
      unsubscribeLive?.();
      unsubscribeReconnect?.();
    };
  }, [authority, client, isConfigLoading, scopeIdentity, stableChannelIds]);

  // A snapshot minted for a different authority or channel set is stale scope,
  // not weaker scope: it is dropped rather than shown against the new one.
  return snapshot.scopeIdentity === scopeIdentity
    ? snapshot
    : emptySnapshot(scopeIdentity, true);
}
