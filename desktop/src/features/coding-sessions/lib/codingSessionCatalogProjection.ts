import * as React from "react";

import type { TranscriptItem } from "@/features/agents/ui/agentSessionTypes";
import {
  buildCodingSessionTargetKey,
  type CodingSessionCommandTarget,
} from "./codingSessionCommand";
import {
  buildCodingSessionTranscriptGenerationId,
  buildTrustedCodingSessionTranscriptProjectionContext,
  selectExactTrustedCodingSessionTranscriptEntries,
  type TrustedCodingSessionTranscriptEntry,
} from "./codingSessionTranscriptPresentation";
import {
  type CodingSessionTranscriptProjector,
  type CodingSessionTranscriptProjectorStats,
  createCodingSessionTranscriptProjector,
} from "./codingSessionTranscriptProjector";
import type { TrustedCodingSessionMetadataEntry } from "./codingSessionTrustedIngress";
import type { CodingSessionCatalogRecord } from "./codingSessionTypes";

/** Work done by every retained generation projector, summed (SV-118). */
export type CodingSessionCatalogProjectionStats =
  CodingSessionTranscriptProjectorStats & {
    /** Generation projectors currently held. */
    generations: number;
    /** Projectors dropped because their generation left the input. */
    evicted: number;
  };

/**
 * The catalog's retained projection: one transcript projector per
 * (channel, signer, execution target), kept across publishes so a streamed
 * event re-presents only what it changed (SV-118).
 *
 * An instance belongs to exactly one ingress scope — authority, channel set
 * and the React lifetime that owns it. The owner discards it when that scope
 * changes; nothing here is process-global, so two catalogs never share a
 * fold and a community switch (which remounts the tree) starts cold.
 *
 * Only the transcript projection is retained. Every catalog record is
 * rebuilt on every merge from the metadata and transcript entries as they
 * stand, so status, titles, conflict counts and timestamps can never be
 * served stale because transcript text happened not to change.
 */
export class CodingSessionCatalogProjection {
  private readonly projectors = new Map<
    string,
    { channelId: string; projector: CodingSessionTranscriptProjector }
  >();
  private evictedCount = 0;
  /** Counters of projectors already dropped, so totals never run backwards. */
  private readonly retired: CodingSessionTranscriptProjectorStats = {
    updates: 0,
    retained: 0,
    appended: 0,
    rebuilt: 0,
    presentedEntries: 0,
    prefixComparisons: 0,
    copiedItems: 0,
  };

  /**
   * One catalog record per (signer, target) the channel has verified events
   * for, newest first. Equal, field for field, to the full rebuild this
   * replaced (`codingSessionProjectionOracle.testFixtures.ts`).
   *
   * A generation is discoverable from either side: metadata alone (a session
   * created but not yet spoken in) or transcripts alone (a provider whose
   * metadata has not arrived yet). Neither is required, so a session never
   * becomes invisible because one of its two streams is late.
   */
  merge(
    channelId: string | null,
    metadataEntries: readonly TrustedCodingSessionMetadataEntry[],
    transcriptEntries: readonly TrustedCodingSessionTranscriptEntry[],
  ): CodingSessionCatalogRecord[] {
    if (!channelId) return [];

    // One pass over each stream instead of a scan per generation. The store
    // mints `targetKey` from the payload's own session
    // (`codingSessionTrustedIngress.ts`, transcript classification), so
    // bucketing by it selects exactly what a per-target filter would.
    const identities = new Map<
      string,
      { target: CodingSessionCommandTarget; signerPubkey: string }
    >();
    const metadataByKey = new Map<string, TrustedCodingSessionMetadataEntry>();
    for (const entry of metadataEntries) {
      if (entry.channelId !== channelId) continue;
      const key = identityKey(entry.signerPubkey, entry.targetKey);
      identities.set(key, {
        target: entry.metadata.session,
        signerPubkey: entry.signerPubkey,
      });
      // The first match wins, as `find` did.
      if (!metadataByKey.has(key)) metadataByKey.set(key, entry);
    }
    const transcriptsByKey = new Map<
      string,
      TrustedCodingSessionTranscriptEntry[]
    >();
    for (const entry of transcriptEntries) {
      if (entry.channelId !== channelId) continue;
      const key = identityKey(entry.signerPubkey, entry.targetKey);
      identities.set(key, {
        target: entry.transcript.session,
        signerPubkey: entry.signerPubkey,
      });
      const bucket = transcriptsByKey.get(key);
      if (bucket) bucket.push(entry);
      else transcriptsByKey.set(key, [entry]);
    }

    const live = new Set<string>();
    const sessions = [...identities.values()].map(
      ({ target, signerPubkey }) => {
        const key = identityKey(
          signerPubkey,
          buildCodingSessionTargetKey(target),
        );
        const targetTranscripts = transcriptsByKey.get(key) ?? [];
        const projectorKey = `${channelId}\u0000${key}`;
        live.add(projectorKey);
        const transcript = this.project(
          projectorKey,
          channelId,
          signerPubkey,
          target,
          targetTranscripts,
        );
        return buildCodingSessionCatalogRecord(
          channelId,
          signerPubkey,
          target,
          // No `conflictCount === 0` gate here: metadata conflict counts are
          // same-signer by construction (the snapshot resolves per signer), and
          // a provider legitimately restates its metadata several times within
          // one second right after a create. The ingress store already picked a
          // deterministic winner; discarding it here threw away the session's
          // title on every fresh create. Receipts and transcripts keep their
          // fail-closed conflict handling — those are immutable facts, not
          // last-writer-wins state.
          metadataByKey.get(key),
          targetTranscripts,
          transcript,
        );
      },
    );
    this.evict(channelId, live);

    applyUmbrellaTurnBudget(sessions);

    sessions.sort(
      (left, right) =>
        Date.parse(right.lastEventAt) - Date.parse(left.lastEventAt) ||
        left.generationId.localeCompare(right.generationId),
    );
    return sessions;
  }

  /** Drop every retained projector; the next merge rebuilds from scratch. */
  reset(): void {
    for (const key of [...this.projectors.keys()]) this.drop(key);
  }

  /** Summed projector counters, for tests and the replay benchmark. */
  stats(): CodingSessionCatalogProjectionStats {
    const total: CodingSessionCatalogProjectionStats = {
      ...this.retired,
      generations: this.projectors.size,
      evicted: this.evictedCount,
    };
    for (const { projector } of this.projectors.values()) {
      total.updates += projector.stats.updates;
      total.retained += projector.stats.retained;
      total.appended += projector.stats.appended;
      total.rebuilt += projector.stats.rebuilt;
      total.presentedEntries += projector.stats.presentedEntries;
      total.prefixComparisons += projector.stats.prefixComparisons;
      total.copiedItems += projector.stats.copiedItems;
    }
    return total;
  }

  private project(
    projectorKey: string,
    channelId: string,
    signerPubkey: string,
    target: CodingSessionCommandTarget,
    targetTranscripts: readonly TrustedCodingSessionTranscriptEntry[],
  ): TranscriptItem[] {
    const exact = selectExactTrustedCodingSessionTranscriptEntries(
      targetTranscripts,
      channelId,
      signerPubkey,
      target,
    );
    let held = this.projectors.get(projectorKey);
    if (exact.length === 0) {
      // Nothing projectable (no transcript yet, or every entry conflicted):
      // hold no fold for it, so a later first entry starts clean.
      if (held) this.drop(projectorKey);
      return [];
    }
    if (!held) {
      held = { channelId, projector: createCodingSessionTranscriptProjector() };
      this.projectors.set(projectorKey, held);
    }
    // The published array is frozen. The record type predates that and says
    // mutable; nothing downstream writes to it (SV-118 audit), and a write
    // now throws instead of silently corrupting retained state.
    return held.projector.update(
      exact,
      buildTrustedCodingSessionTranscriptProjectionContext(
        channelId,
        signerPubkey,
        target,
      ),
    ) as TranscriptItem[];
  }

  /** Forget generations of this channel that the latest input no longer has. */
  private evict(channelId: string, live: ReadonlySet<string>): void {
    for (const [key, held] of [...this.projectors]) {
      if (held.channelId !== channelId || live.has(key)) continue;
      this.drop(key);
    }
  }

  private drop(key: string): void {
    const held = this.projectors.get(key);
    if (!held) return;
    for (const counter of Object.keys(this.retired) as Array<
      keyof CodingSessionTranscriptProjectorStats
    >) {
      this.retired[counter] += held.projector.stats[counter];
    }
    this.projectors.delete(key);
    this.evictedCount += 1;
  }
}

/** A fresh, empty catalog projection. */
export function createCodingSessionCatalogProjection(): CodingSessionCatalogProjection {
  return new CodingSessionCatalogProjection();
}

function identityKey(signerPubkey: string, targetKey: string): string {
  return `${signerPubkey}\u0000${targetKey}`;
}

function buildCodingSessionCatalogRecord(
  channelId: string,
  signerPubkey: string,
  target: CodingSessionCommandTarget,
  metadataEntry: TrustedCodingSessionMetadataEntry | undefined,
  targetTranscripts: readonly TrustedCodingSessionTranscriptEntry[],
  transcript: TranscriptItem[],
): CodingSessionCatalogRecord {
  // Two maxima with different floors, as before SV-118: the transcript's
  // starts at zero, the event's at the metadata time (which a signer may set
  // before the epoch).
  let lastTranscriptAt = 0;
  let latestTimestamp = metadataEntry ? metadataEntry.createdAt * 1000 : 0;
  let transcriptConflicts = 0;
  for (const entry of targetTranscripts) {
    lastTranscriptAt = Math.max(lastTranscriptAt, entry.transcript.timestamp);
    latestTimestamp = Math.max(latestTimestamp, entry.transcript.timestamp);
    transcriptConflicts += entry.conflictCount;
  }
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
    lastTranscriptAt: targetTranscripts.length > 0 ? lastTranscriptAt : null,
    status: metadata?.status ?? inferTranscriptStatus(targetTranscripts),
    // When the status itself was observed (44223 created_at, ms). Kept
    // separate from lastEventAt (a max over both streams) so status
    // derivation can compare metadata freshness against the transcript.
    statusAt: metadataEntry ? metadataEntry.createdAt * 1000 : null,
    statusEventId: metadataEntry?.eventId ?? null,
    transcript,
    conflictCount: transcriptConflicts + (metadataEntry?.conflictCount ?? 0),
    commandTarget: target,
    projectRef: metadata?.projectRef ?? null,
    repoRef: metadata?.repoRef ?? null,
    sessionRef: metadata?.sessionRef ?? null,
    provider: metadata?.provider ?? null,
    runtime: metadata?.runtime ?? target.driver,
    model: metadata?.model ?? null,
    agentRef: metadata?.agentRef ?? null,
    // The role key only ever accompanies an actor (the decoder enforces
    // it), so an execution with no agent can never carry one.
    role: metadata?.agentRef ? (metadata.role ?? null) : null,
    // Published only for a budgeted umbrella, so absence is "the provider
    // disclosed no budget" — never a locally assumed unlimited. Raised to
    // the umbrella's furthest count below, because the provider only ever
    // publishes it on the acting execution.
    turnBudget: metadata?.turnBudget ?? null,
    // The router's decision, straight off the 44223 the provider signed.
    // Nothing here re-derives it: a seat's routing is a fact the wire
    // carries or does not.
    routing: metadata?.routing ?? null,
    capabilities: metadata?.capabilities ?? null,
    // Which `bee` this exact generation's seat was observed running,
    // straight off the 44223 the provider signed. Null means this record's
    // own metadata carried no `beeStamp` — an older host, not an unknown
    // build (`codingSessionSeatBee.ts`).
    beeStamp: metadata?.beeStamp ?? null,
    // Which persona pack this exact generation's seat was observed
    // staging, straight off the same 44223. Null means this record's own
    // metadata carried no `packRef` — no 30624 source for the project, or
    // an older host (`codingSessionPackRef.ts`).
    packRef: metadata?.packRef ?? null,
    // How that pack was composed, off the same 44223; null when the host
    // staged an uncomposed pack or predates the key (spec § 4.6).
    composeRef: metadata?.composeRef ?? null,
  } satisfies CodingSessionCatalogRecord;
}

/**
 * Raise every seat of an umbrella to that umbrella's furthest turn budget.
 *
 * The provider publishes `turnBudget` on the metadata of whichever execution
 * is acting, so a sibling that has been idle keeps echoing whatever the count
 * was when it last spoke. The budget is one number per umbrella, and counts
 * only rise, so the highest `used` any seat has published is the newest fact
 * about it — showing a seat's own stale copy would tell the operator there is
 * room at the moment the next agent turn is refused. An execution that claimed
 * no `sessionRef` belongs to no umbrella and keeps exactly what it published.
 */
function applyUmbrellaTurnBudget(sessions: CodingSessionCatalogRecord[]): void {
  const furthest = new Map<
    string,
    NonNullable<CodingSessionCatalogRecord["turnBudget"]>
  >();
  for (const session of sessions) {
    const { sessionRef, turnBudget } = session;
    if (!sessionRef || !turnBudget) continue;
    const held = furthest.get(sessionRef);
    if (
      !held ||
      turnBudget.used > held.used ||
      (turnBudget.used === held.used && turnBudget.limit > held.limit)
    ) {
      furthest.set(sessionRef, turnBudget);
    }
  }
  for (const session of sessions) {
    if (!session.sessionRef) continue;
    const budget = furthest.get(session.sessionRef);
    if (budget) session.turnBudget = budget;
  }
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
 * was still emitting, which is what "running" reports. The latest
 * non-conflicting entry by `eventSeq` decides; on a tie the earlier entry in
 * input order wins, as the stable descending sort this replaced did.
 */
function inferTranscriptStatus(
  entries: readonly TrustedCodingSessionTranscriptEntry[],
): CodingSessionCatalogRecord["status"] {
  let newest: TrustedCodingSessionTranscriptEntry | undefined;
  for (const entry of entries) {
    if (entry.conflictCount !== 0) continue;
    if (!newest || entry.transcript.eventSeq > newest.transcript.eventSeq) {
      newest = entry;
    }
  }
  const latest = newest?.transcript.item as Record<string, unknown> | undefined;
  if (!latest || typeof latest.kind !== "string") return "unknown";
  if (latest.kind === "interrupted") return "interrupted";
  if (latest.kind === "result") {
    if (latest.subtype === "cancelled") return "interrupted";
    if (latest.subtype === "error" || latest.isError === true) return "failed";
    if (latest.subtype === "success") return "completed";
  }
  return "running";
}

/**
 * The retained catalog projection for one mounted catalog (SV-118).
 *
 * Owned by the calling component's lifetime and keyed by `scopeKey`: a
 * changed key starts from an empty projection. Generations are keyed inside
 * the projection by channel, signer and full target. A community switch
 * remounts the whole tree and takes this with it.
 */
export function useRetainedCodingSessionCatalogProjection(
  scopeKey: string,
): CodingSessionCatalogProjection {
  const held = React.useRef<{
    scopeKey: string;
    projection: CodingSessionCatalogProjection;
  } | null>(null);
  // Render-time lazy init, keyed. A render React discards (Strict Mode,
  // concurrent rendering) can at worst mint a projection the next render
  // replaces: one rebuild, never a different answer, because a merge's output
  // depends only on its input.
  if (held.current === null || held.current.scopeKey !== scopeKey) {
    held.current = {
      scopeKey,
      projection: createCodingSessionCatalogProjection(),
    };
  }
  return held.current.projection;
}
