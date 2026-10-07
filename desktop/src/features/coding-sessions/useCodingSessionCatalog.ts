import * as React from "react";

import { relayClient as defaultRelayClient } from "@/shared/api/relayClient";
import type { RelayEvent } from "@/shared/api/types";
import type { CodingSessionPopoutBootstrap } from "./lib/codingSessionBootstrap";
import { groupCodingSessionCatalog } from "./lib/codingSessionUmbrellaModel";
import { rememberCodingSessionPopoutBootstrap } from "./lib/codingSessionBootstrap";
import { buildCodingSessionTargetKey } from "./lib/codingSessionCommand";
import {
  type CodingSessionCatalogProjection,
  createCodingSessionCatalogProjection,
  useRetainedCodingSessionCatalogProjection,
} from "./lib/codingSessionCatalogProjection";
import type {
  TrustedCodingSessionMetadataEntry,
  TrustedCodingSessionTranscriptEntry,
} from "./lib/codingSessionTrustedIngress";
import type {
  CodingSessionCatalogRecord,
  CodingSessionCatalogSnapshot,
  GlobalCodingSessionCatalogSnapshot,
} from "./lib/codingSessionTypes";
import {
  type CodingSessionCreateObservationSnapshot,
  useCodingSessionCreateObservations,
} from "./lib/useCodingSessionCreateObservations";
import { useTrustedCodingSessionIngress } from "./lib/useTrustedCodingSessionIngress";

/**
 * The coding-session catalog for one channel.
 *
 * Trusted ingress is the only source of session *facts*. The donor merged a
 * second, Hive-backed projection catalog in beside it; with the compatibility
 * transport gone there is nothing to merge — a session exists here if and only
 * if a trusted signer published signed 442xx events for it.
 *
 * Alongside them the snapshot carries `creates`: human-signed 44221 create
 * observations, each joined to an execution by that provider's own receipt.
 * They decide only *who operates what*, never whether a session exists or what
 * happened in it, and the catalog's `isLoading` deliberately does not wait on
 * them — a session renders as soon as its facts arrive, with authority
 * unresolved (and therefore ungated) until the observations land.
 *
 * The same read carries `geneses`: every accepted 44226 in the channel. A
 * genesis no receipt-joined create names is a founded umbrella with nothing
 * running, which the catalog cannot express as an entry — so the founding
 * facts ride beside the entries with their own `foundingIsLoading`, and a
 * cold start reads "loading" rather than "missing" for a session that exists.
 */
export function useCodingSessionCatalog(
  channelId: string | null,
  popoutBootstrap: CodingSessionPopoutBootstrap | null = null,
  options: {
    requirePopoutBootstrap?: boolean;
    /** `open` reads by channel membership — display surfaces every reader of
     * the channel should share. `config` (default) additionally restricts to
     * this machine's trusted provider allowlist. */
    authorityMode?: "config" | "open";
  } = {},
): CodingSessionCatalogSnapshot {
  const ingressChannelIds = React.useMemo(
    () => (channelId ? [channelId] : []),
    [channelId],
  );
  const requirePopoutBootstrap = options.requirePopoutBootstrap ?? false;
  const ingressBootstrap = React.useMemo(
    () =>
      popoutBootstrap
        ? {
            authorityIdentity: popoutBootstrap.authorityIdentity,
            relayEvents: popoutBootstrap.relayEvents,
          }
        : null,
    [popoutBootstrap],
  );
  const trustedIngress = useTrustedCodingSessionIngress(
    ingressChannelIds,
    null,
    null,
    defaultRelayClient,
    ingressBootstrap,
    options.authorityMode ?? "config",
  );
  // Operator authority rides a second, deliberately separate subscription:
  // 44221 creates are signed by humans, so they must never be admitted to the
  // provider-authority-signed trusted store. Each observation is bound to an
  // execution only through that provider's own 44224 receipt.
  const createObservations =
    useCodingSessionCreateObservations(ingressChannelIds);
  const projection = useRetainedCodingSessionCatalogProjection(
    catalogProjectionScopeKey(trustedIngress),
  );
  const snapshot = React.useMemo(
    () =>
      composeCodingSessionCatalogSnapshot(
        channelId,
        trustedIngress,
        createObservations,
        projection,
      ),
    [channelId, createObservations, projection, trustedIngress],
  );

  useRememberedCodingSessionPopoutBootstraps(
    channelId,
    snapshot.entries,
    trustedIngress.authorityIdentity,
    trustedIngress.retainedRawEvents,
  );

  // A pop-out that was opened without a staged snapshot has no way to know it
  // is looking at the same accepted generation the parent window offered, so
  // it refuses rather than resolving something that merely shares an id.
  if (requirePopoutBootstrap && !popoutBootstrap) {
    return refuseCodingSessionPopoutSnapshot(
      snapshot,
      "This pop-out did not receive an exact signed session snapshot. Reopen the generation from the main window.",
    );
  }
  // A snapshot minted under a different configured authority is stale trust,
  // not weaker trust; it is refused outright rather than merged.
  if (
    popoutBootstrap &&
    trustedIngress.authorityIdentity !== null &&
    popoutBootstrap.authorityIdentity !== trustedIngress.authorityIdentity
  ) {
    return refuseCodingSessionPopoutSnapshot(
      snapshot,
      "This pop-out snapshot no longer matches the configured coding-session authority. Reopen the generation from the main window.",
    );
  }
  return snapshot;
}

/**
 * The ingress scope a retained projection is valid for: authority identity
 * plus the channel set and request the snapshot was read under. A change to
 * either starts from an empty projection rather than resuming folds built
 * under different trust.
 */
function catalogProjectionScopeKey(
  trustedIngress: Pick<
    ReturnType<typeof useTrustedCodingSessionIngress>,
    "authorityIdentity" | "scopeIdentity"
  >,
): string {
  return `${trustedIngress.authorityIdentity ?? ""}\u0000${trustedIngress.scopeIdentity}`;
}

/** The trusted-ingress slice the channel snapshot is composed from. */
type TrustedIngressSlice = Pick<
  ReturnType<typeof useTrustedCodingSessionIngress>,
  | "metadata"
  | "transcripts"
  | "isLoading"
  | "historyCompleteness"
  | "errorMessage"
  | "authorityErrorMessage"
  | "rejectedAuthorCount"
  | "invalidSignatureCount"
  | "turnStartedAtFor"
>;

/**
 * Compose the channel snapshot from its two reads. Pure apart from the
 * optional retained `projection`, which only changes how much work a merge
 * repeats, never its answer — so the field contract (creates and geneses ride
 * beside the entries, and the founding read's own loading flag stays separate
 * from the catalog's) is testable without mounting either subscription.
 */
export function composeCodingSessionCatalogSnapshot(
  channelId: string | null,
  trustedIngress: TrustedIngressSlice,
  createObservations: Pick<
    CodingSessionCreateObservationSnapshot,
    "observations" | "geneses" | "isLoading"
  >,
  projection: CodingSessionCatalogProjection = createCodingSessionCatalogProjection(),
): CodingSessionCatalogSnapshot {
  return {
    channelId,
    entries: projection.merge(
      channelId,
      trustedIngress.metadata,
      trustedIngress.transcripts,
    ),
    creates: createObservations.observations,
    geneses: createObservations.geneses,
    foundingIsLoading: createObservations.isLoading,
    isLoading: trustedIngress.isLoading,
    historyCompleteness: trustedIngress.historyCompleteness,
    errorMessage: trustedIngress.errorMessage,
    authorityErrorMessage: trustedIngress.authorityErrorMessage,
    rejectedAuthorCount: trustedIngress.rejectedAuthorCount,
    invalidSignatureCount: trustedIngress.invalidSignatureCount,
    turnStartedAtFor: trustedIngress.turnStartedAtFor,
  };
}

/**
 * A pop-out that cannot vouch for its snapshot shows nothing from it: no
 * entries, no creates, and no geneses — a founded row is as much a claim
 * about the channel as a started one. Loading is over; the refusal is the
 * answer.
 */
export function refuseCodingSessionPopoutSnapshot(
  snapshot: CodingSessionCatalogSnapshot,
  authorityErrorMessage: string,
): CodingSessionCatalogSnapshot {
  return {
    ...snapshot,
    entries: [],
    creates: [],
    geneses: [],
    isLoading: false,
    foundingIsLoading: false,
    authorityErrorMessage,
  };
}

/**
 * Keep the raw signed bytes behind each accepted generation available to a
 * future pop-out, keyed by the same route coordinates the window will use.
 */
function useRememberedCodingSessionPopoutBootstraps(
  channelId: string | null,
  entries: readonly CodingSessionCatalogRecord[],
  authorityIdentity: string | null,
  retainedRawEvents: TrustedRawEventReader,
): void {
  React.useEffect(() => {
    if (!channelId || !authorityIdentity) return;
    // An umbrella pop-out must be able to re-verify every member execution,
    // not only the routed one, so each generation's staged snapshot carries
    // the raw signed events of its whole umbrella (deduplicated by event id).
    const umbrellaMembers = buildUmbrellaMemberIndex(entries);
    for (const entry of entries) {
      if (!entry.commandTarget || !entry.providerAuthorityPubkey) continue;
      const members = umbrellaMembers.get(entry.generationId) ?? [entry];
      const relayEventsById = new Map<string, RelayEvent>();
      for (const member of members) {
        if (!member.commandTarget || !member.providerAuthorityPubkey) continue;
        for (const event of retainedRawEvents({
          channelId,
          signerPubkey: member.providerAuthorityPubkey,
          targetKey: buildCodingSessionTargetKey(member.commandTarget),
        })) {
          relayEventsById.set(event.id, event);
        }
      }
      rememberCodingSessionPopoutBootstrap({
        channelId,
        generationId: entry.generationId,
        authorityIdentity,
        relayEvents: [...relayEventsById.values()],
      });
    }
  }, [authorityIdentity, channelId, entries, retainedRawEvents]);
}

/**
 * For each generation, the catalog records of every execution sharing its
 * umbrella (itself included). Implicit umbrellas map to themselves alone.
 */
function buildUmbrellaMemberIndex(
  entries: readonly CodingSessionCatalogRecord[],
): Map<string, CodingSessionCatalogRecord[]> {
  const index = new Map<string, CodingSessionCatalogRecord[]>();
  for (const umbrella of groupCodingSessionCatalog(entries)) {
    const members = umbrella.executions.flatMap((execution) => [
      ...execution.priorGenerations,
      execution.activeGeneration,
    ]);
    for (const member of members) {
      index.set(member.generationId, members);
    }
  }
  return index;
}

type TrustedRawEventReader = ReturnType<
  typeof useTrustedCodingSessionIngress
>["retainedRawEvents"];

/** The trusted session catalog across every source channel. */
export function useGlobalCodingSessionCatalog(
  channelIds: readonly string[],
  options: {
    authorityMode?: "config" | "open";
    /** Shelf-cache localStorage key (`codingSessionShelfCacheKey`) — set by
     * the sidebar so sessions paint before the relay answers. */
    persistenceCacheKey?: string;
  } = {},
): GlobalCodingSessionCatalogSnapshot {
  const trustedIngress = useTrustedCodingSessionIngress(
    channelIds,
    null,
    null,
    defaultRelayClient,
    null,
    options.authorityMode ?? "config",
    options.persistenceCacheKey,
  );
  const createObservations = useCodingSessionCreateObservations(channelIds);
  const projection = useRetainedCodingSessionCatalogProjection(
    catalogProjectionScopeKey(trustedIngress),
  );
  return React.useMemo(
    () => ({
      entries: channelIds.flatMap((channelId) =>
        projection
          .merge(channelId, trustedIngress.metadata, trustedIngress.transcripts)
          .map((session) => ({ channelId, session })),
      ),
      creates: createObservations.observations,
      geneses: createObservations.geneses,
      foundingIsLoading: createObservations.isLoading,
      isLoading: trustedIngress.isLoading,
      errorMessage: trustedIngress.errorMessage,
      authorityErrorMessage: trustedIngress.authorityErrorMessage,
      lifecycleFor: trustedIngress.lifecycleFor,
    }),
    [channelIds, createObservations, projection, trustedIngress],
  );
}

/**
 * Build one catalog record per (signer, target) a channel has verified events
 * for — the full rebuild, with no retained state. Callers that publish
 * repeatedly (the hooks above) hold a `CodingSessionCatalogProjection`
 * instead, which returns the same records while re-presenting only what each
 * publish changed (SV-118).
 */
export function mergeTrustedCodingSessionIngress(
  channelId: string | null,
  metadataEntries: readonly TrustedCodingSessionMetadataEntry[],
  transcriptEntries: readonly TrustedCodingSessionTranscriptEntry[],
): CodingSessionCatalogRecord[] {
  return createCodingSessionCatalogProjection().merge(
    channelId,
    metadataEntries,
    transcriptEntries,
  );
}
