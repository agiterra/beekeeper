import * as React from "react";

import { useGlobalAgentConfig } from "@/features/agents/useGlobalAgentConfig";
import { relayClient as defaultRelayClient } from "@/shared/api/relayClient";
import type { RelaySubscriptionFilter } from "@/shared/api/relayClientShared";
import type { RelayEvent } from "@/shared/api/types";
import {
  KIND_CODING_SESSION_LIFECYCLE_RECEIPT,
  KIND_CODING_SESSION_METADATA,
  KIND_CODING_SESSION_TRANSCRIPT,
} from "@/shared/constants/kinds";
import { createCodingSessionDiscoveryController } from "./codingSessionDiscoveryRetry";
import {
  buildCodingSessionIngressAuthorityIdentity,
  type CodingSessionIngressAuthority,
  resolveCodingSessionIngressAuthority,
} from "./codingSessionIngressAuthority";
import {
  type CodingSessionGenerationScope,
  type CodingSessionLifecycleResolution,
  isExactProviderAuthorityPubkey,
  TrustedCodingSessionIngressStore,
  type TrustedCodingSessionIngressSnapshot,
} from "./codingSessionTrustedIngress";

const TRUSTED_INGRESS_HISTORY_LIMIT = 1000;

/** The three signed kinds this consumer reads. */
export const TRUSTED_CODING_SESSION_INGRESS_KINDS = [
  KIND_CODING_SESSION_METADATA,
  KIND_CODING_SESSION_LIFECYCLE_RECEIPT,
  KIND_CODING_SESSION_TRANSCRIPT,
] as const;

/** Relay client surface the ingress hook needs. */
export type CodingSessionIngressClient = {
  fetchEvents(filter: RelaySubscriptionFilter): Promise<RelayEvent[]>;
  subscribeLive(
    filter: RelaySubscriptionFilter,
    onEvent: (event: RelayEvent) => void,
  ): Promise<() => void>;
  subscribeToReconnects?(listener: () => void): () => void;
};

export type TrustedCodingSessionIngressHookSnapshot =
  TrustedCodingSessionIngressSnapshot & {
    authorityIdentity: string | null;
    scopeIdentity: string;
    isLoading: boolean;
    errorMessage: string | null;
    authorityErrorMessage: string | null;
    lifecycle: CodingSessionLifecycleResolution | null;
    /**
     * The verified raw events behind one generation, for pop-out bootstrap.
     *
     * The store is the only thing that has ever seen these bytes verified, so
     * the seam hands them out rather than letting a caller re-derive them from
     * a projection it cannot re-check.
     */
    retainedRawEvents: (scope: CodingSessionGenerationScope) => RelayEvent[];
  };

/**
 * An exact accepted generation snapshot handed to a fresh pop-out webview.
 *
 * It is only ever a head start: the receiving store re-classifies every event
 * against its own configured authority, and a snapshot minted under a
 * different authority identity is dropped rather than trusted.
 */
export type TrustedCodingSessionIngressBootstrap = {
  authorityIdentity: string;
  relayEvents: readonly RelayEvent[];
};

const NO_RETAINED_RAW_EVENTS = (): RelayEvent[] => [];

/**
 * One native filter covers every coding-session kind.
 *
 * The donor needed a second, deliberately broad kind-9 filter beside this one
 * because its compatibility transport shared a kind with ordinary chat. Owning
 * the relay means the kinds themselves are the filter, so a client never has
 * to pull a bounded window of unrelated messages to find its own events.
 */
export function buildTrustedCodingSessionIngressFilter(
  channelIds: readonly string[],
  authority: CodingSessionIngressAuthority,
  limit: number,
): RelaySubscriptionFilter {
  const filter: RelaySubscriptionFilter = {
    kinds: [...TRUSTED_CODING_SESSION_INGRESS_KINDS],
    "#h": [...channelIds],
    limit,
  };
  if (authority.state === "valid") {
    filter.authors = authority.allowed.map((entry) => entry.pubkey);
  }
  return filter;
}

export function buildTrustedCodingSessionIngressHistoryFilters(
  channelIds: readonly string[],
  authority: CodingSessionIngressAuthority,
  limit: number,
): RelaySubscriptionFilter[] {
  return [buildTrustedCodingSessionIngressFilter(channelIds, authority, limit)];
}

export function buildTrustedCodingSessionIngressLiveFilter(
  channelIds: readonly string[],
  authority: CodingSessionIngressAuthority,
): RelaySubscriptionFilter {
  return buildTrustedCodingSessionIngressFilter(channelIds, authority, 0);
}

function emptySnapshot(
  authorityIdentity: string | null,
  scopeIdentity: string,
  lifecycle: CodingSessionLifecycleResolution | null,
): TrustedCodingSessionIngressHookSnapshot {
  return {
    authorityIdentity,
    scopeIdentity,
    metadata: [],
    transcripts: [],
    malformedCount: 0,
    rejectedAuthorCount: 0,
    invalidSignatureCount: 0,
    isLoading: false,
    errorMessage: null,
    authorityErrorMessage: null,
    lifecycle,
    retainedRawEvents: NO_RETAINED_RAW_EVENTS,
  };
}

/**
 * Subscribe to provider-signed lifecycle receipts, metadata, and transcripts
 * for the visible channel set. Supplying `commandId` exposes a
 * generation-fenced resolution: receipt target first, then metadata for that
 * exact target only.
 */
export function useTrustedCodingSessionIngress(
  channelIds: readonly string[],
  commandId: string | null = null,
  providerAuthorityPubkey: string | null = null,
  client: CodingSessionIngressClient = defaultRelayClient,
  bootstrap: TrustedCodingSessionIngressBootstrap | null = null,
): TrustedCodingSessionIngressHookSnapshot {
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
  const initialLifecycle =
    commandId && stableChannelIds.length === 1
      ? isExactProviderAuthorityPubkey(providerAuthorityPubkey)
        ? ({ state: "pending", commandId } as const)
        : ({ state: "conflict", commandId } as const)
      : null;
  const storeIdentity = `${authorityIdentity}|${stableChannelIdentity}`;
  const requestIdentity = `${storeIdentity}|${commandId ?? ""}|${providerAuthorityPubkey ?? ""}`;
  const [snapshot, setSnapshot] = React.useState(() =>
    emptySnapshot(authorityIdentity, requestIdentity, initialLifecycle),
  );
  const storeRef = React.useRef<{
    identity: string;
    store: TrustedCodingSessionIngressStore;
  } | null>(null);

  React.useEffect(() => {
    if (storeRef.current?.identity !== storeIdentity) {
      storeRef.current = {
        identity: storeIdentity,
        store: new TrustedCodingSessionIngressStore(),
      };
    }
    const store = storeRef.current.store;
    // A staged pop-out snapshot is a head start, never a grant: it only enters
    // the store when it was minted under this window's own authority
    // identity, and every event still runs the full classifier.
    if (
      bootstrap &&
      bootstrap.authorityIdentity === authorityIdentity &&
      authority.state === "valid"
    ) {
      store.ingestRelayEvents(
        bootstrap.relayEvents,
        stableChannelIds,
        authority,
      );
    }
    const resolveLifecycle = (): CodingSessionLifecycleResolution | null =>
      commandId && stableChannelIds.length === 1
        ? isExactProviderAuthorityPubkey(providerAuthorityPubkey)
          ? store.resolveLifecycle(
              stableChannelIds[0],
              commandId,
              providerAuthorityPubkey,
            )
          : { state: "conflict", commandId }
        : null;
    if (isConfigLoading) {
      setSnapshot({
        ...emptySnapshot(
          authorityIdentity,
          requestIdentity,
          resolveLifecycle(),
        ),
        isLoading: true,
      });
      return;
    }
    if (authority.state !== "valid") {
      setSnapshot({
        ...emptySnapshot(
          authorityIdentity,
          requestIdentity,
          resolveLifecycle(),
        ),
        errorMessage: authority.errorMessage,
        authorityErrorMessage: authority.errorMessage,
      });
      return;
    }
    if (stableChannelIds.length === 0) {
      setSnapshot(
        emptySnapshot(authorityIdentity, requestIdentity, resolveLifecycle()),
      );
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
      const stored = store.snapshot(stableChannelIds);
      setSnapshot({
        authorityIdentity,
        scopeIdentity: requestIdentity,
        ...stored,
        isLoading: historyLoading,
        errorMessage:
          historyError && liveError
            ? `${historyError}\n${liveError}`
            : (historyError ?? liveError),
        authorityErrorMessage: null,
        lifecycle: resolveLifecycle(),
        retainedRawEvents: (scope) => store.retainedRawEvents(scope),
      });
    };

    const historyController = createCodingSessionDiscoveryController({
      async load() {
        const errors: string[] = [];
        for (const filter of buildTrustedCodingSessionIngressHistoryFilters(
          stableChannelIds,
          authority,
          TRUSTED_INGRESS_HISTORY_LIMIT,
        )) {
          try {
            const events = await client.fetchEvents(filter);
            if (cancelled) return;
            store.ingestRelayEvents(events, stableChannelIds, authority);
          } catch (error) {
            errors.push(
              error instanceof Error
                ? error.message
                : "Failed to load coding-session lifecycle history.",
            );
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
          : error instanceof Error
            ? error.message
            : "Failed to load coding-session lifecycle history.";
        publish();
      },
      retrySeed: `ingress:${requestIdentity}`,
    });

    const establishLive = () => {
      if (unsubscribeLive || liveSubscribePending) return;
      liveSubscribePending = true;
      client
        .subscribeLive(
          buildTrustedCodingSessionIngressLiveFilter(
            stableChannelIds,
            authority,
          ),
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
              : "Failed to subscribe to coding-session lifecycle events.";
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
    bootstrap,
    client,
    commandId,
    isConfigLoading,
    providerAuthorityPubkey,
    requestIdentity,
    stableChannelIds,
    storeIdentity,
  ]);

  return snapshot.authorityIdentity === authorityIdentity &&
    snapshot.scopeIdentity === requestIdentity
    ? snapshot
    : emptySnapshot(authorityIdentity, requestIdentity, initialLifecycle);
}

/** Draft-facing shorthand for one exact create command. */
export function useCodingSessionLifecycleResolution(
  channelId: string | null,
  commandId: string | null,
  providerAuthorityPubkey: string | null,
  client: CodingSessionIngressClient = defaultRelayClient,
): TrustedCodingSessionIngressHookSnapshot {
  const channelIds = React.useMemo(
    () => (channelId ? [channelId] : []),
    [channelId],
  );
  return useTrustedCodingSessionIngress(
    channelIds,
    commandId,
    providerAuthorityPubkey,
    client,
  );
}
