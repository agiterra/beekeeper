import * as React from "react";

import { relayClient as defaultRelayClient } from "@/shared/api/relayClient";
import type { RelayEvent } from "@/shared/api/types";
import type { CodingSessionPopoutBootstrap } from "./lib/codingSessionBootstrap";
import { groupCodingSessionCatalog } from "./lib/codingSessionUmbrellaModel";
import { rememberCodingSessionPopoutBootstrap } from "./lib/codingSessionBootstrap";
import { buildCodingSessionTargetKey } from "./lib/codingSessionCommand";
import {
  buildCodingSessionTranscriptGenerationId,
  projectTrustedCodingSessionTranscriptsToTranscript,
} from "./lib/codingSessionTranscriptPresentation";
import type {
  TrustedCodingSessionMetadataEntry,
  TrustedCodingSessionTranscriptEntry,
} from "./lib/codingSessionTrustedIngress";
import type {
  CodingSessionCatalogRecord,
  CodingSessionCatalogSnapshot,
  GlobalCodingSessionCatalogSnapshot,
} from "./lib/codingSessionTypes";
import { useCodingSessionCreateObservations } from "./lib/useCodingSessionCreateObservations";
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
  const snapshot = React.useMemo(
    () => ({
      channelId,
      entries: mergeTrustedCodingSessionIngress(
        channelId,
        trustedIngress.metadata,
        trustedIngress.transcripts,
      ),
      creates: createObservations.observations,
      isLoading: trustedIngress.isLoading,
      errorMessage: trustedIngress.errorMessage,
      authorityErrorMessage: trustedIngress.authorityErrorMessage,
      rejectedAuthorCount: trustedIngress.rejectedAuthorCount,
      invalidSignatureCount: trustedIngress.invalidSignatureCount,
    }),
    [channelId, createObservations.observations, trustedIngress],
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
    return {
      ...snapshot,
      entries: [],
      creates: [],
      isLoading: false,
      authorityErrorMessage:
        "This pop-out did not receive an exact signed session snapshot. Reopen the generation from the main window.",
    };
  }
  // A snapshot minted under a different configured authority is stale trust,
  // not weaker trust; it is refused outright rather than merged.
  if (
    popoutBootstrap &&
    trustedIngress.authorityIdentity !== null &&
    popoutBootstrap.authorityIdentity !== trustedIngress.authorityIdentity
  ) {
    return {
      ...snapshot,
      entries: [],
      creates: [],
      isLoading: false,
      authorityErrorMessage:
        "This pop-out snapshot no longer matches the configured coding-session authority. Reopen the generation from the main window.",
    };
  }
  return snapshot;
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
  return React.useMemo(
    () => ({
      entries: channelIds.flatMap((channelId) =>
        mergeTrustedCodingSessionIngress(
          channelId,
          trustedIngress.metadata,
          trustedIngress.transcripts,
        ).map((session) => ({ channelId, session })),
      ),
      creates: createObservations.observations,
      isLoading: trustedIngress.isLoading,
      errorMessage: trustedIngress.errorMessage,
      authorityErrorMessage: trustedIngress.authorityErrorMessage,
      lifecycleFor: trustedIngress.lifecycleFor,
    }),
    [channelIds, createObservations.observations, trustedIngress],
  );
}

/**
 * Build one catalog record per (signer, target) a channel has verified events
 * for.
 *
 * A generation is discoverable from either side: metadata alone (a session
 * created but not yet spoken in) or transcripts alone (a provider whose
 * metadata has not arrived yet). Neither is required, so a session never
 * becomes invisible because one of its two streams is late.
 */
export function mergeTrustedCodingSessionIngress(
  channelId: string | null,
  metadataEntries: readonly TrustedCodingSessionMetadataEntry[],
  transcriptEntries: readonly TrustedCodingSessionTranscriptEntry[],
): CodingSessionCatalogRecord[] {
  if (!channelId) return [];

  const identities = new Map<
    string,
    {
      target: TrustedCodingSessionTranscriptEntry["transcript"]["session"];
      signerPubkey: string;
    }
  >();
  for (const entry of metadataEntries) {
    if (entry.channelId !== channelId) continue;
    identities.set(`${entry.signerPubkey}\u0000${entry.targetKey}`, {
      target: entry.metadata.session,
      signerPubkey: entry.signerPubkey,
    });
  }
  for (const entry of transcriptEntries) {
    if (entry.channelId !== channelId) continue;
    identities.set(`${entry.signerPubkey}\u0000${entry.targetKey}`, {
      target: entry.transcript.session,
      signerPubkey: entry.signerPubkey,
    });
  }

  const sessions = [...identities.values()].map(({ target, signerPubkey }) => {
    const targetKey = buildCodingSessionTargetKey(target);
    // No `conflictCount === 0` gate here: metadata conflict counts are
    // same-signer by construction (the snapshot resolves per signer), and a
    // provider legitimately restates its metadata several times within one
    // second right after a create. The ingress store already picked a
    // deterministic winner; discarding it here threw away the session's
    // title on every fresh create. Receipts and transcripts keep their
    // fail-closed conflict handling — those are immutable facts, not
    // last-writer-wins state.
    const metadataEntry = metadataEntries.find(
      (entry) =>
        entry.channelId === channelId &&
        entry.targetKey === targetKey &&
        entry.signerPubkey === signerPubkey,
    );
    const targetTranscripts = transcriptEntries.filter(
      (entry) =>
        entry.channelId === channelId &&
        entry.targetKey === targetKey &&
        entry.signerPubkey === signerPubkey,
    );
    const transcript = projectTrustedCodingSessionTranscriptsToTranscript(
      targetTranscripts,
      channelId,
      signerPubkey,
      target,
    );
    const latestTimestamp = targetTranscripts.reduce(
      (latest, entry) => Math.max(latest, entry.transcript.timestamp),
      metadataEntry ? metadataEntry.createdAt * 1000 : 0,
    );
    const metadata = metadataEntry?.metadata;
    const driverLabel = formatDriverLabel(target.driver);
    return {
      generationId: buildCodingSessionTranscriptGenerationId(
        channelId,
        signerPubkey,
        target,
      ),
      label: `${driverLabel} · generation ${target.generation}`,
      title: metadata?.title ?? "Coding session",
      providerAuthorityPubkey: signerPubkey,
      metadataAuthorityPubkey: metadataEntry?.signerPubkey ?? null,
      lastEventAt: new Date(latestTimestamp).toISOString(),
      status: metadata?.status ?? inferTranscriptStatus(targetTranscripts),
      // When the status itself was observed (44223 created_at, ms). Kept
      // separate from lastEventAt (a max over both streams) so status
      // derivation can compare metadata freshness against the transcript.
      statusAt: metadataEntry ? metadataEntry.createdAt * 1000 : null,
      transcript,
      conflictCount: targetTranscripts.reduce(
        (count, entry) => count + entry.conflictCount,
        metadataEntry?.conflictCount ?? 0,
      ),
      commandTarget: target,
      projectRef: metadata?.projectRef ?? null,
      repoRef: metadata?.repoRef ?? null,
      sessionRef: metadata?.sessionRef ?? null,
      provider: metadata?.provider ?? null,
      runtime: metadata?.runtime ?? target.driver,
      model: metadata?.model ?? null,
      capabilities: metadata?.capabilities ?? null,
    } satisfies CodingSessionCatalogRecord;
  });

  sessions.sort(
    (left, right) =>
      Date.parse(right.lastEventAt) - Date.parse(left.lastEventAt) ||
      left.generationId.localeCompare(right.generationId),
  );
  return sessions;
}

function formatDriverLabel(driver: string): string {
  const normalized = driver.trim().replace(/[-_]+/g, " ");
  if (!normalized) return "Coding";
  return normalized.replace(/\b\p{L}/gu, (letter) => letter.toUpperCase());
}

/**
 * Infer a status from the transcript when metadata has not arrived.
 *
 * Only terminal items say anything definite; anything else means the provider
 * was still emitting, which is what "running" reports.
 */
function inferTranscriptStatus(
  entries: readonly TrustedCodingSessionTranscriptEntry[],
): CodingSessionCatalogRecord["status"] {
  const latest = entries
    .filter((entry) => entry.conflictCount === 0)
    .sort(
      (left, right) => right.transcript.eventSeq - left.transcript.eventSeq,
    )[0]?.transcript.item as Record<string, unknown> | undefined;
  if (!latest || typeof latest.kind !== "string") return "unknown";
  if (latest.kind === "interrupted") return "interrupted";
  if (latest.kind === "result") {
    if (latest.subtype === "cancelled") return "interrupted";
    if (latest.subtype === "error" || latest.isError === true) return "failed";
    if (latest.subtype === "success") return "completed";
  }
  return "running";
}
