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
import {
  armCodingSessionDiscoveryOnConnect,
  type CodingSessionDiscoveryArmingClient,
} from "./codingSessionDiscoveryArming";
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
  type CodingSessionTurnProgress,
  type CodingSessionGenerationScope,
  type CodingSessionLifecycleResolution,
  isExactProviderAuthorityPubkey,
  TrustedCodingSessionIngressStore,
  type TrustedCodingSessionIngressSnapshot,
} from "./codingSessionTrustedIngress";
import {
  acquireCodingSessionIngressStore,
  peekCodingSessionIngressStore,
} from "./codingSessionIngressStoreCache";
import {
  fanOutObservedCodingSessionEvents,
  subscribeToObservedCodingSessionEvents,
} from "./codingSessionObservedEvents";
import {
  createCodingSessionIngressPublishCoalescer,
  reuseCodingSessionIngressSnapshotArrays,
} from "./useTrustedCodingSessionIngressPublish";

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
} & CodingSessionDiscoveryArmingClient;

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
     * How far the same `commandId` has got, from the provider's own per-stage
     * receipts. `null` means it has published none — either the turn is still
     * in flight or this provider predates the per-stage contract — and every
     * surface reading it must say nothing rather than infer a stage.
     */
    turnProgress: CodingSessionTurnProgress | null;
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
    /** Resolve a verified provider-signed `turn_started` event time. */
    turnStartedAtFor: (
      channelId: string,
      turnId: string,
      providerAuthorityPubkey: string,
    ) => number | null;
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
const NO_TURN_STARTED_AT = (): number | null => null;

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

/**
 * History is one filter PER KIND, each with its own `limit`. One filter over
 * all three kinds shared a single budget ordered by `created_at` across kinds,
 * so a long session's metadata and receipts (about four per turn) crowded its
 * transcript out of the newest page — and the relay caps a page at 1000
 * (NIP-11 `max_limit`), so raising the limit is not the fix. The same reason
 * `codingSessionCreateObservations` queries per kind.
 */
export function buildTrustedCodingSessionIngressHistoryFilters(
  channelIds: readonly string[],
  authority: CodingSessionIngressAuthority,
  limit: number,
): RelaySubscriptionFilter[] {
  const filter = buildTrustedCodingSessionIngressFilter(
    channelIds,
    authority,
    limit,
  );
  return TRUSTED_CODING_SESSION_INGRESS_KINDS.map((kind) => ({
    ...filter,
    kinds: [kind],
  }));
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
  turnProgress: CodingSessionTurnProgress | null = null,
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
    turnProgress,
    retainedRawEvents: NO_RETAINED_RAW_EVENTS,
    lifecycleFor: NO_LIFECYCLE_RESOLUTION,
    turnStartedAtFor: NO_TURN_STARTED_AT,
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
  // Keep the verified store warm for scopes a person navigates *back* to.
  // Every coding session is its own transport channel, so switching sessions
  // changes the scope — and discarding the store meant paying for a live
  // subscribe and a history fetch, behind a "Loading" screen, every time.
  //
  // A command-scoped ingress is excluded on purpose: it waits on exactly one
  // command id and then unmounts for good, so retaining its store buys nothing
  // and would only widen what a one-shot wait can see.
  const retainStore = commandId === null;
  /**
   * What a display scope shows before its own effect has run — on first mount
   * and again after a scope change. Two things are true at that moment and both
   * have to be said: this client may already hold verified facts for the scope
   * (paint them), and it has definitely not subscribed or refetched for it yet
   * (`isLoading`). Without the second half an empty catalog reads as settled,
   * and the workspace renders "this generation is not in the relay catalog" for
   * a session the person just clicked.
   *
   * Command scopes keep the plain empty snapshot: they wait on one command id,
   * hold no navigable history, and their consumers read `isLoading` as a claim
   * about that one command.
   */
  const pendingScopeSnapshot = (): TrustedCodingSessionIngressHookSnapshot => ({
    ...emptySnapshot(authorityIdentity, requestIdentity, initialLifecycle),
    ...retainedIngressSnapshot(storeIdentity, stableChannelIds),
    isLoading: true,
  });
  const [snapshot, setSnapshot] = React.useState(() =>
    retainStore
      ? pendingScopeSnapshot()
      : emptySnapshot(authorityIdentity, requestIdentity, initialLifecycle),
  );
  const storeRef = React.useRef<{
    identity: string;
    store: TrustedCodingSessionIngressStore;
  } | null>(null);

  React.useEffect(() => {
    if (!retainStore && storeRef.current?.identity !== storeIdentity) {
      storeRef.current = {
        identity: storeIdentity,
        store: new TrustedCodingSessionIngressStore(),
      };
    }
    const store = retainStore
      ? acquireCodingSessionIngressStore(storeIdentity)
      : // Non-null by construction: the branch above minted one for this exact
        // identity. Falling back keeps a future edit from reading `null`.
        (storeRef.current?.store ?? new TrustedCodingSessionIngressStore());
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
    const resolveTurnProgress = (): CodingSessionTurnProgress | null =>
      commandId &&
      stableChannelIds.length === 1 &&
      isExactProviderAuthorityPubkey(providerAuthorityPubkey)
        ? store.resolveTurnProgress(
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
          resolveTurnProgress(),
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
          resolveTurnProgress(),
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
          resolveTurnProgress(),
        ),
      );
      return;
    }

    let cancelled = false;
    let unsubscribeLive: (() => void) | null = null;
    let liveSubscribePending = false;
    // True from the first publish, not from `onAttemptStart`. A history request
    // always follows `establishLive()` — on both the resolve and the reject
    // path — so the window between arming the scope and that request starting
    // is part of the first load. Publishing `isLoading: false` across it told
    // every consumer the catalog had settled while it was still empty, which is
    // what made a session switch flash "this generation is not in the relay
    // catalog" before the spinner it should have shown all along.
    let historyLoading = true;
    let historyError: string | null = null;
    let liveError: string | null = null;
    let persistTimer: ReturnType<typeof setTimeout> | null = null;

    // Trailing-debounced cache rewrite: publish() can still fire once per
    // frame during a stream, and the retained-shelf selection walks every
    // metadata bucket, so the write coalesces bursts rather than serializing.
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

    // Stores are retained and shared between mounted catalogs. Another
    // listener can ingest a batch first; duplicate ingestion is only a no-op
    // for this reader after it has published that shared store revision.
    let publishedRevision = -1;
    let lastStored: ReturnType<typeof store.snapshot> | null = null;
    const publishSnapshot = () => {
      if (cancelled) return;
      const stored = reuseCodingSessionIngressSnapshotArrays(
        lastStored,
        store.snapshot(stableChannelIds),
      );
      lastStored = stored;
      publishedRevision = store.getRevision();
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
        turnProgress: resolveTurnProgress(),
        retainedRawEvents: (scope) => store.retainedRawEvents(scope),
        lifecycleFor: (forChannelId, forCommandId, forAuthorityPubkey) =>
          isExactProviderAuthorityPubkey(forAuthorityPubkey)
            ? store.resolveLifecycle(
                forChannelId,
                forCommandId,
                forAuthorityPubkey,
              )
            : null,
        turnStartedAtFor: (forChannelId, turnId, forAuthorityPubkey) =>
          isExactProviderAuthorityPubkey(forAuthorityPubkey)
            ? store.resolveTurnStartedAtMs(
                forChannelId,
                turnId,
                forAuthorityPubkey,
              )
            : null,
      });
      schedulePersist();
    };
    // Relay events publish at most once per animation frame: a streaming turn
    // delivers many events per frame and only the frame's last state is ever
    // painted. Everything else — loading, errors, the live fence arming —
    // publishes at once, and absorbs any frame already requested.
    const coalescer =
      createCodingSessionIngressPublishCoalescer(publishSnapshot);
    const publish = coalescer.flushNow;
    const receiveObservedEvents = (events: readonly RelayEvent[]) => {
      if (cancelled) return;
      store.ingestRelayEvents(events, stableChannelIds, authority);
      if (store.getRevision() !== publishedRevision) coalescer.schedule();
    };
    const unsubscribeObserved = subscribeToObservedCodingSessionEvents(
      receiveObservedEvents,
    );

    const historyController = createCodingSessionDiscoveryController({
      async load() {
        const errors: string[] = [];
        // The per-kind pages are independent: fetch them together so history
        // costs one round trip, then ingest in filter order.
        const pages = await Promise.allSettled(
          buildTrustedCodingSessionIngressHistoryFilters(
            stableChannelIds,
            authority,
            TRUSTED_INGRESS_HISTORY_LIMIT,
          ).map((filter) => client.fetchEvents(filter)),
        );
        if (cancelled) return;
        for (const page of pages) {
          if (page.status === "fulfilled") {
            store.ingestRelayEvents(page.value, stableChannelIds, authority);
            fanOutObservedCodingSessionEvents(
              page.value,
              receiveObservedEvents,
            );
          } else {
            errors.push(
              page.reason instanceof Error
                ? page.reason.message
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
            coalescer.schedule();
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
    const disarm = armCodingSessionDiscoveryOnConnect(client, () => {
      if (unsubscribeLive) historyController.request();
      else establishLive();
    });
    return () => {
      cancelled = true;
      coalescer.cancel();
      historyController.cancel();
      unsubscribeLive?.();
      disarm();
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
    retainStore,
    stableChannelIds,
    storeIdentity,
  ]);

  if (
    snapshot.authorityIdentity === authorityIdentity &&
    snapshot.scopeIdentity === requestIdentity
  ) {
    return snapshot;
  }
  // The scope changed and this render is ahead of the effect that will serve
  // it — the same moment as first mount, so it gets the same answer.
  return retainStore
    ? pendingScopeSnapshot()
    : emptySnapshot(authorityIdentity, requestIdentity, initialLifecycle);
}

/**
 * The already-verified contents of a warm store for this scope, if any.
 *
 * Read-only and creation-free: render must not mint a store, and "no store
 * yet" is a real answer — it is the first visit to this scope.
 */
function retainedIngressSnapshot(
  storeIdentity: string,
  channelIds: readonly string[],
): Partial<TrustedCodingSessionIngressHookSnapshot> {
  const store = peekCodingSessionIngressStore(storeIdentity);
  if (!store || channelIds.length === 0) return {};
  return {
    ...store.snapshot(channelIds),
    retainedRawEvents: (scope) => store.retainedRawEvents(scope),
    lifecycleFor: (forChannelId, forCommandId, forAuthorityPubkey) =>
      isExactProviderAuthorityPubkey(forAuthorityPubkey)
        ? store.resolveLifecycle(forChannelId, forCommandId, forAuthorityPubkey)
        : null,
  };
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
