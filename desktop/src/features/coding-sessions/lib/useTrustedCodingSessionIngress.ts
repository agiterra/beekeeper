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
  filterShelfCacheEventsToChannels,
  readCodingSessionShelfCache,
  writeCodingSessionShelfCache,
} from "./codingSessionShelfCache";
import {
  buildCodingSessionIngressAuthorityIdentity,
  buildPinnedCodingSessionIngressAuthority,
  type CodingSessionIngressAuthority,
  OPEN_CODING_SESSION_INGRESS_AUTHORITY,
  resolveCodingSessionIngressAuthority,
} from "./codingSessionIngressAuthority";
import {
  type CodingSessionCommandRefusal,
  type CodingSessionGenerationScope,
  type CodingSessionLifecycleResolution,
  isExactProviderAuthorityPubkey,
  TrustedCodingSessionIngressStore,
  type TrustedCodingSessionIngressSnapshot,
} from "./codingSessionTrustedIngress";
import {
  fanOutObservedCodingSessionEvents,
  subscribeToObservedCodingSessionEvents,
} from "./codingSessionObservedEvents";

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
     * The signed refusal of the same `commandId`, read as a plain turn rather
     * than a lifecycle transition. A turn establishes nothing, so it has no
     * lifecycle resolution worth reading — only "refused, with these words" or
     * nothing at all.
     */
    turnRefusal: CodingSessionCommandRefusal | null;
    /**
     * The verified raw events behind one generation, for pop-out bootstrap.
     *
     * The store is the only thing that has ever seen these bytes verified, so
     * the seam hands them out rather than letting a caller re-derive them from
     * a projection it cannot re-check.
     */
    retainedRawEvents: (scope: CodingSessionGenerationScope) => RelayEvent[];
    /**
     * Resolve one create command's lifecycle from this store's verified
     * receipts, addressable by exactly (channel, commandId, authority). The
     * multi-channel global catalog ingests 44224 receipts but its snapshot
     * never surfaced them — this closure is how the sidebar's optimistic
     * pending rows learn their create was accepted (or failed) without a
     * second store or subscription.
     */
    lifecycleFor: (
      channelId: string,
      commandId: string,
      providerAuthorityPubkey: string,
    ) => CodingSessionLifecycleResolution | null;
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
const NO_LIFECYCLE_RESOLUTION = (): CodingSessionLifecycleResolution | null =>
  null;

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
  turnRefusal: CodingSessionCommandRefusal | null = null,
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
    turnRefusal,
    retainedRawEvents: NO_RETAINED_RAW_EVENTS,
    lifecycleFor: NO_LIFECYCLE_RESOLUTION,
  };
}

/**
 * Which signers this consumer will read.
 *
 * - `config` — the machine-local `allowed-bridge-pubkeys` list: what this
 *   machine may *run*. Correct for the local create flow, which provisions and
 *   then addresses its own provider.
 * - `open` — channel membership: what the relay already accepted into a
 *   readable channel. Correct for display surfaces.
 * - `pinned` — the exact provider this command names. Correct for
 *   command-scoped resolution, where the answer can only come from one signer
 *   and that signer is stated in the command itself.
 */
export type CodingSessionIngressAuthorityMode = "config" | "open" | "pinned";

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
  authorityMode: CodingSessionIngressAuthorityMode = "config",
  /**
   * localStorage key for the persisted shelf cache
   * (`codingSessionShelfCacheKey`). When set, a fresh store seeds itself from
   * the cached signed metadata events (fully re-verified) before the relay
   * answers, and every published snapshot schedules a debounced rewrite.
   * Only the sidebar's global catalog passes this — per-channel workspace
   * catalogs are cheap to refetch and stay uncached.
   */
  persistenceCacheKey: string | undefined = undefined,
): TrustedCodingSessionIngressHookSnapshot {
  const stableChannelIdentity = [...new Set(channelIds)].sort().join("\u0000");
  const stableChannelIds = React.useMemo(
    () =>
      stableChannelIdentity.length > 0
        ? stableChannelIdentity.split("\u0000")
        : [],
    [stableChannelIdentity],
  );
  const { globalConfig, isLoading: rawConfigLoading } = useGlobalAgentConfig();
  // Only config mode reads the local allowlist, so only config mode waits for
  // it: open reads by channel membership and pinned reads the command's own
  // named provider.
  const isConfigLoading = authorityMode === "config" ? rawConfigLoading : false;
  const configAuthority = React.useMemo(
    () =>
      resolveCodingSessionIngressAuthority(
        globalConfig["allowed-bridge-pubkeys"],
      ),
    [globalConfig],
  );
  // A command with no exact provider pin addresses nobody; there is no signer
  // whose answer would count, so it resolves to nothing readable rather than
  // falling back to a broader set. Memoized apart from the config so a config
  // load that changes nothing here cannot re-arm the subscription.
  const pinnedAuthority = React.useMemo(
    () =>
      buildPinnedCodingSessionIngressAuthority(
        isExactProviderAuthorityPubkey(providerAuthorityPubkey)
          ? providerAuthorityPubkey
          : "",
      ),
    [providerAuthorityPubkey],
  );
  const authority =
    authorityMode === "open"
      ? OPEN_CODING_SESSION_INGRESS_AUTHORITY
      : authorityMode === "pinned"
        ? pinnedAuthority
        : configAuthority;
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
      authority.state !== "invalid"
    ) {
      store.ingestRelayEvents(
        bootstrap.relayEvents,
        stableChannelIds,
        authority,
      );
    }
    // Persisted shelf cache: the same head-start seam, fed from localStorage.
    // Signed bytes only — the classifier re-verifies signature, authority,
    // and channel scope, so a stale or tampered cache cannot inject rows.
    // Pre-filtering to the current channel set keeps out-of-scope events from
    // registering as malformed diagnostics. Idempotent across effect re-runs:
    // the store memoizes verdicts per event id.
    if (
      persistenceCacheKey &&
      authority.state !== "invalid" &&
      stableChannelIds.length > 0
    ) {
      const cached = filterShelfCacheEventsToChannels(
        readCodingSessionShelfCache(persistenceCacheKey),
        stableChannelIds,
      );
      if (cached.length > 0) {
        store.ingestRelayEvents(cached, stableChannelIds, authority);
      }
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
    // The same command id, read the other way: a turn's only signed outcome is
    // a refusal, so it needs no lifecycle machinery and no conflict state.
    const resolveTurnRefusal = (): CodingSessionCommandRefusal | null =>
      commandId &&
      stableChannelIds.length === 1 &&
      isExactProviderAuthorityPubkey(providerAuthorityPubkey)
        ? store.resolveTurnRefusal(
            stableChannelIds[0],
            commandId,
            providerAuthorityPubkey,
          )
        : null;
    if (isConfigLoading) {
      setSnapshot({
        ...emptySnapshot(
          authorityIdentity,
          requestIdentity,
          resolveLifecycle(),
          resolveTurnRefusal(),
        ),
        isLoading: true,
      });
      return;
    }
    if (authority.state === "invalid") {
      setSnapshot({
        ...emptySnapshot(
          authorityIdentity,
          requestIdentity,
          resolveLifecycle(),
          resolveTurnRefusal(),
        ),
        errorMessage: authority.errorMessage,
        authorityErrorMessage: authority.errorMessage,
      });
      return;
    }
    if (stableChannelIds.length === 0) {
      setSnapshot(
        emptySnapshot(
          authorityIdentity,
          requestIdentity,
          resolveLifecycle(),
          resolveTurnRefusal(),
        ),
      );
      return;
    }

    let cancelled = false;
    let unsubscribeLive: (() => void) | null = null;
    let liveSubscribePending = false;
    let historyLoading = false;
    let historyError: string | null = null;
    let liveError: string | null = null;
    let persistTimer: ReturnType<typeof setTimeout> | null = null;

    // Trailing-debounced cache rewrite: publish() fires per live event, and
    // the retained-shelf selection walks every metadata bucket, so the write
    // coalesces bursts rather than serializing on each event.
    const schedulePersist = () => {
      if (!persistenceCacheKey || persistTimer !== null) return;
      persistTimer = setTimeout(() => {
        persistTimer = null;
        writeCodingSessionShelfCache(
          persistenceCacheKey,
          store.retainedShelfEvents(),
        );
      }, 1_000);
    };

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
        turnRefusal: resolveTurnRefusal(),
        retainedRawEvents: (scope) => store.retainedRawEvents(scope),
        lifecycleFor: (forChannelId, forCommandId, forAuthorityPubkey) =>
          isExactProviderAuthorityPubkey(forAuthorityPubkey)
            ? store.resolveLifecycle(
                forChannelId,
                forCommandId,
                forAuthorityPubkey,
              )
            : null,
      });
      schedulePersist();
    };
    const receiveObservedEvents = (events: readonly RelayEvent[]) => {
      if (cancelled) return;
      store.ingestRelayEvents(events, stableChannelIds, authority);
      publish();
    };
    const unsubscribeObserved = subscribeToObservedCodingSessionEvents(
      receiveObservedEvents,
    );

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
            fanOutObservedCodingSessionEvents(events, receiveObservedEvents);
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
          // Close the mount/channel-add gap only after the live fence is in
          // place: history now covers everything before the subscription,
          // while the subscription covers everything after it.
          historyController.request();
        })
        .catch((error) => {
          liveSubscribePending = false;
          liveError =
            error instanceof Error
              ? error.message
              : "Failed to subscribe to coding-session lifecycle events.";
          publish();
          // History-only discovery is still useful when live setup fails.
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
      if (persistTimer !== null) {
        clearTimeout(persistTimer);
        // Flush the pending rewrite so a teardown (scope change, unmount)
        // never loses the last events the store verified.
        writeCodingSessionShelfCache(
          persistenceCacheKey,
          store.retainedShelfEvents(),
        );
      }
      unsubscribeObserved();
    };
  }, [
    authority,
    authorityIdentity,
    bootstrap,
    client,
    commandId,
    isConfigLoading,
    persistenceCacheKey,
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

/**
 * Draft-facing shorthand for one exact command.
 *
 * `authorityMode` defaults to `config` so the local create flow — which
 * provisions this machine's provider and then addresses it — is unchanged. A
 * command addressed to a provider this machine does not run (any session
 * founded by another member) must pass `pinned`, or its answer is neither
 * subscribed for nor admitted.
 */
export function useCodingSessionLifecycleResolution(
  channelId: string | null,
  commandId: string | null,
  providerAuthorityPubkey: string | null,
  client: CodingSessionIngressClient = defaultRelayClient,
  authorityMode: CodingSessionIngressAuthorityMode = "config",
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
    null,
    authorityMode,
  );
}
