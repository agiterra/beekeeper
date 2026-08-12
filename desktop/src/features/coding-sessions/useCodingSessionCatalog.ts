import * as React from "react";

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
import { useTrustedCodingSessionIngress } from "./lib/useTrustedCodingSessionIngress";

/**
 * The coding-session catalog for one channel.
 *
 * Trusted ingress is the only source. The donor merged a second, Hive-backed
 * projection catalog in beside it; with the compatibility transport gone there
 * is nothing to merge — a session exists here if and only if a trusted signer
 * published signed 442xx events for it.
 */
export function useCodingSessionCatalog(
  channelId: string | null,
): CodingSessionCatalogSnapshot {
  const ingressChannelIds = React.useMemo(
    () => (channelId ? [channelId] : []),
    [channelId],
  );
  const trustedIngress = useTrustedCodingSessionIngress(ingressChannelIds);
  return React.useMemo(
    () => ({
      channelId,
      entries: mergeTrustedCodingSessionIngress(
        channelId,
        trustedIngress.metadata,
        trustedIngress.transcripts,
      ),
      isLoading: trustedIngress.isLoading,
      errorMessage: trustedIngress.errorMessage,
      authorityErrorMessage: trustedIngress.authorityErrorMessage,
      rejectedAuthorCount: trustedIngress.rejectedAuthorCount,
      invalidSignatureCount: trustedIngress.invalidSignatureCount,
    }),
    [channelId, trustedIngress],
  );
}

/** The trusted session catalog across every source channel. */
export function useGlobalCodingSessionCatalog(
  channelIds: readonly string[],
): GlobalCodingSessionCatalogSnapshot {
  const trustedIngress = useTrustedCodingSessionIngress(channelIds);
  return React.useMemo(
    () => ({
      entries: channelIds.flatMap((channelId) =>
        mergeTrustedCodingSessionIngress(
          channelId,
          trustedIngress.metadata,
          trustedIngress.transcripts,
        ).map((session) => ({ channelId, session })),
      ),
      isLoading: trustedIngress.isLoading,
      errorMessage: trustedIngress.errorMessage,
      authorityErrorMessage: trustedIngress.authorityErrorMessage,
    }),
    [channelIds, trustedIngress],
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
    const metadataEntry = metadataEntries.find(
      (entry) =>
        entry.channelId === channelId &&
        entry.targetKey === targetKey &&
        entry.signerPubkey === signerPubkey &&
        entry.conflictCount === 0,
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
      title: metadata?.title ?? `${driverLabel} session`,
      providerAuthorityPubkey: signerPubkey,
      metadataAuthorityPubkey: metadataEntry?.signerPubkey ?? null,
      lastEventAt: new Date(latestTimestamp).toISOString(),
      status: metadata?.status ?? inferTranscriptStatus(targetTranscripts),
      transcript,
      conflictCount: targetTranscripts.reduce(
        (count, entry) => count + entry.conflictCount,
        metadataEntry?.conflictCount ?? 0,
      ),
      commandTarget: target,
      projectRef: metadata?.projectRef ?? null,
      repoRef: metadata?.repoRef ?? null,
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
