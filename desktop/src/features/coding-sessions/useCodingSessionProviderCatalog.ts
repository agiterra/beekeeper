import * as React from "react";

import { useGlobalAgentConfig } from "@/features/agents/useGlobalAgentConfig";
import { relayClient as defaultRelayClient } from "@/shared/api/relayClient";
import type { RelaySubscriptionFilter } from "@/shared/api/relayClientShared";
import { KIND_CODING_SESSION_PROVIDER_CATALOG } from "@/shared/constants/kinds";
import { createCodingSessionDiscoveryController } from "./lib/codingSessionDiscoveryRetry";
import {
  buildCodingSessionIngressAuthorityIdentity,
  resolveCodingSessionIngressAuthority,
} from "./lib/codingSessionIngressAuthority";
import {
  CodingSessionProviderCatalogStore,
  type CodingSessionProviderCatalogSnapshot,
} from "./lib/codingSessionProviderCatalog";
import type { CodingSessionIngressClient } from "./lib/useTrustedCodingSessionIngress";

const PROVIDER_CATALOG_HISTORY_LIMIT = 1000;

export type CodingSessionProviderCatalogHookSnapshot =
  CodingSessionProviderCatalogSnapshot & {
    authorityIdentity: string | null;
    scopeIdentity: string;
    isLoading: boolean;
    errorMessage: string | null;
    authorityErrorMessage: string | null;
  };

/**
 * 44222 is a native kind here, so one filter serves both history and live.
 * The donor needed a second kind-9 filter because it kept the catalog TS-only.
 */
export function buildCodingSessionProviderCatalogFilter(
  channelIds: readonly string[],
  authors: readonly string[],
  limit: number,
): RelaySubscriptionFilter {
  return {
    kinds: [KIND_CODING_SESSION_PROVIDER_CATALOG],
    "#h": [...channelIds],
    authors: [...authors],
    limit,
  };
}

export function buildCodingSessionProviderCatalogHistoryFilters(
  channelIds: readonly string[],
  authors: readonly string[],
  limit: number,
): RelaySubscriptionFilter[] {
  return [buildCodingSessionProviderCatalogFilter(channelIds, authors, limit)];
}

function emptySnapshot(
  authorityIdentity: string | null,
  scopeIdentity: string,
): CodingSessionProviderCatalogHookSnapshot {
  return {
    authorityIdentity,
    scopeIdentity,
    entries: [],
    malformedCount: 0,
    rejectedAuthorCount: 0,
    invalidSignatureCount: 0,
    conflicts: [],
    conflictCount: 0,
    isLoading: false,
    errorMessage: null,
    authorityErrorMessage: null,
  };
}

/** Trusted provider availability across the exact visible channels. */
export function useCodingSessionProviderCatalog(
  channelIds: readonly string[],
  client: CodingSessionIngressClient = defaultRelayClient,
): CodingSessionProviderCatalogHookSnapshot {
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
    emptySnapshot(authorityIdentity, scopeIdentity),
  );
  const storeRef = React.useRef<{
    identity: string;
    store: CodingSessionProviderCatalogStore;
  } | null>(null);

  React.useEffect(() => {
    if (storeRef.current?.identity !== scopeIdentity) {
      storeRef.current = {
        identity: scopeIdentity,
        store: new CodingSessionProviderCatalogStore(),
      };
    }
    const store = storeRef.current.store;
    if (isConfigLoading) {
      setSnapshot({
        ...emptySnapshot(authorityIdentity, scopeIdentity),
        isLoading: true,
      });
      return;
    }
    if (authority.state !== "valid") {
      setSnapshot({
        ...emptySnapshot(authorityIdentity, scopeIdentity),
        errorMessage: authority.errorMessage,
        authorityErrorMessage: authority.errorMessage,
      });
      return;
    }
    if (stableChannelIds.length === 0) {
      setSnapshot(emptySnapshot(authorityIdentity, scopeIdentity));
      return;
    }

    let cancelled = false;
    let unsubscribeLive: (() => void) | null = null;
    let liveSubscribePending = false;
    let historyLoading = false;
    let historyError: string | null = null;
    let liveError: string | null = null;
    const authors = authority.allowed.map((entry) => entry.pubkey);
    const publish = () => {
      if (cancelled) return;
      setSnapshot({
        authorityIdentity,
        scopeIdentity,
        ...store.snapshot(stableChannelIds),
        isLoading: historyLoading,
        errorMessage: combineErrors(historyError, liveError),
        authorityErrorMessage: null,
      });
    };
    const historyController = createCodingSessionDiscoveryController({
      async load() {
        const errors: string[] = [];
        for (const filter of buildCodingSessionProviderCatalogHistoryFilters(
          stableChannelIds,
          authors,
          PROVIDER_CATALOG_HISTORY_LIMIT,
        )) {
          try {
            const events = await client.fetchEvents(filter);
            if (cancelled) return;
            store.ingestRelayEvents(events, stableChannelIds, authority);
          } catch (error) {
            errors.push(formatProviderCatalogHistoryError(error));
          }
        }
        if (errors.length > 0) {
          throw new Error([...new Set(errors)].join("\n"));
        }
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
          : formatProviderCatalogHistoryError(error);
        publish();
      },
      retrySeed: `provider:${scopeIdentity}`,
    });
    const establishLive = () => {
      if (unsubscribeLive || liveSubscribePending) return;
      liveSubscribePending = true;
      client
        .subscribeLive(
          buildCodingSessionProviderCatalogFilter(stableChannelIds, authors, 0),
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
        })
        .catch((error) => {
          liveSubscribePending = false;
          liveError =
            error instanceof Error
              ? error.message
              : "Failed to subscribe to coding-session provider availability.";
          publish();
        });
    };

    publish();
    establishLive();
    historyController.request();
    const unsubscribeReconnect = client.subscribeToReconnects?.(() => {
      establishLive();
      historyController.request();
    });
    return () => {
      cancelled = true;
      historyController.cancel();
      unsubscribeLive?.();
      unsubscribeReconnect?.();
    };
  }, [
    authority,
    authorityIdentity,
    client,
    isConfigLoading,
    scopeIdentity,
    stableChannelIds,
  ]);

  return snapshot.authorityIdentity === authorityIdentity &&
    snapshot.scopeIdentity === scopeIdentity
    ? snapshot
    : emptySnapshot(authorityIdentity, scopeIdentity);
}

function combineErrors(
  left: string | null,
  right: string | null,
): string | null {
  if (left && right) return `${left}\n${right}`;
  return left ?? right;
}

function formatProviderCatalogHistoryError(error: unknown): string {
  return error instanceof Error
    ? error.message
    : "Failed to load coding-session provider availability.";
}
