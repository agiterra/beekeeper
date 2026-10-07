/**
 * TEST-ONLY ORACLE (SV-118). A frozen copy of the full-rebuild transcript
 * projection and catalog merge exactly as they stood at `d698e41bf`, before
 * the retained projector existed. Parity tests and the replay benchmark
 * compare the production path against this file.
 *
 * It must never import the retained projector, the catalog projection owner
 * or `projectCodingSessionTranscript` — the fold below is its own copy, so a
 * bug introduced into the production fold cannot also hide in the oracle.
 * The item builders it calls (`codingSessionTranscriptItems`,
 * `codingSessionSubagents`) are shared on purpose: SV-118 does not change
 * how one item is presented, only how often.
 *
 * Not a test file itself (no `.test.`), so the runner never executes it, and
 * nothing in `src/` imports it, so it never reaches the app bundle. Delete it
 * with the last full-rebuild caller (plan § 5: kept through SV-100/101).
 */

import type { TranscriptItem } from "@/features/agents/ui/agentSessionTypes";
import { buildCodingSessionTargetKey } from "./codingSessionCommand";
import type { CodingSessionCommandTarget } from "./codingSessionCommand";
import { isRecord, safeString, toIsoTimestamp } from "./codingSessionDefensive";
import type { CodingSessionIngressSource } from "./codingSessionIngressAuthority";
import { encodeStructuredKey } from "./codingSessionKeys";
import { stampCodingSessionSubagentFields as stampSubagent } from "./codingSessionSubagents";
import {
  buildBaseTranscriptItem,
  buildPairedToolResultItem,
  buildPlanFromExitPlanModeToolCall,
  buildToolCallItem,
  buildToolResultItem,
  type CodingSessionBridgeSource,
  type CodingSessionItemIdentity as Identity,
  type CodingSessionProjectedTranscriptItem,
  finalizeCodingSessionItem as finalize,
  toolIdFromToolCall,
  toolIdFromToolResult,
} from "./codingSessionTranscriptItems";
import {
  buildCodingSessionTranscriptGenerationId,
  type TrustedCodingSessionTranscriptEntry,
} from "./codingSessionTranscriptPresentation";
import type { TrustedCodingSessionMetadataEntry } from "./codingSessionTrustedIngress";
import type { CodingSessionCatalogRecord } from "./codingSessionTypes";

/** The source commit this oracle was copied from. */
export const CODING_SESSION_PROJECTION_ORACLE_SOURCE_SHA =
  "d698e41bf00b33a3ebdec10bd2a7cf984bef1896";

type CodingSessionTranscriptTarget = {
  driver: string;
  instanceId: string;
  sessionId: string;
  generation: number;
};

type ProjectCodingSessionTranscriptOptions = {
  channelId?: string | null;
  generationId?: string | null;
  bridgeSource?: CodingSessionBridgeSource | null;
};

/** Oracle copy of `projectCodingSessionTranscript` at the source SHA. */
export function oracleProjectCodingSessionTranscript(
  envelopes: unknown,
  options: ProjectCodingSessionTranscriptOptions = {},
): CodingSessionProjectedTranscriptItem[] {
  if (!Array.isArray(envelopes)) {
    return [];
  }
  return projectCodingSessionTranscriptUnsafe(envelopes, options);
}

function projectCodingSessionTranscriptUnsafe(
  envelopes: unknown[],
  options: ProjectCodingSessionTranscriptOptions,
): CodingSessionProjectedTranscriptItem[] {
  const projected = assignSyntheticTurnPresentation(
    envelopes.map((envelope) => toIntermediate(envelope, options)),
  );
  const result: CodingSessionProjectedTranscriptItem[] = [];
  const pendingToolCalls = new Map<
    string,
    {
      index: number;
      item: Record<string, unknown>;
      ctx: Identity;
    }
  >();

  for (const entry of projected) {
    if (entry.kind === "fallback") {
      result.push(entry.item);
      continue;
    }

    const item = entry.item;
    if (!isRecord(item) || typeof item.kind !== "string") {
      result.push(
        finalize(buildBaseTranscriptItem(item, entry.ctx), entry.ctx),
      );
      continue;
    }

    if (item.kind === "tool_call") {
      const plan = buildPlanFromExitPlanModeToolCall(item, entry.ctx);
      if (plan) {
        result.push(stampSubagent(finalize(plan, entry.ctx), item));
        continue;
      }

      result.push(
        stampSubagent(
          finalize(buildToolCallItem(item, entry.ctx), entry.ctx),
          item,
        ),
      );
      const toolId = toolIdFromToolCall(item);
      if (toolId !== null) {
        pendingToolCalls.set(toolCallPairingKey(entry.ctx.targetKey, toolId), {
          index: result.length - 1,
          item,
          ctx: entry.ctx,
        });
      }
      continue;
    }

    if (item.kind === "tool_result") {
      const toolId = toolIdFromToolResult(item);
      const pending =
        toolId === null
          ? null
          : pendingToolCalls.get(
              toolCallPairingKey(entry.ctx.targetKey, toolId),
            );
      if (pending) {
        result[pending.index] = stampSubagent(
          finalize(
            buildPairedToolResultItem(
              pending.item,
              pending.ctx,
              item,
              entry.ctx,
            ),
            pending.ctx,
          ),
          pending.item,
          item,
        );
        if (toolId !== null) {
          pendingToolCalls.delete(
            toolCallPairingKey(entry.ctx.targetKey, toolId),
          );
        }
        continue;
      }
      result.push(
        stampSubagent(
          finalize(buildToolResultItem(item, entry.ctx), entry.ctx),
          item,
        ),
      );
      continue;
    }

    result.push(
      stampSubagent(
        finalize(buildBaseTranscriptItem(item, entry.ctx), entry.ctx),
        item,
      ),
    );
  }

  return result;
}

/**
 * Turn reconstruction for producers that do not name their own turns.
 *
 * When an envelope carries a `turnId` that identity wins outright. Otherwise a
 * fresh, non-steered user prompt is the only safe boundary from which a turn
 * can be derived for display grouping, and its immutable target + event
 * identity becomes the stable key. A target change fences the open turn, and
 * terminal entries close it. Orphan/pre-prompt telemetry deliberately stays
 * ungrouped instead of being guessed into a turn.
 */
function assignSyntheticTurnPresentation(
  entries: IntermediateEnvelope[],
): IntermediateEnvelope[] {
  let active:
    | {
        targetKey: string;
        turnId: string;
      }
    | undefined;

  return entries.map((entry) => {
    if (entry.kind === "fallback") {
      active = undefined;
      return entry;
    }

    const item = entry.item;
    const kind =
      isRecord(item) && typeof item.kind === "string" ? item.kind : null;

    if (active?.targetKey !== entry.ctx.targetKey) {
      active = undefined;
    }

    let acpSource: string | undefined;
    if (kind === "user_prompt") {
      const steered = isRecord(item) && item.steered === true;
      acpSource = steered ? "session/steer:user" : "session/prompt:user";
      if (!steered) {
        active = {
          targetKey: entry.ctx.targetKey,
          turnId: buildSyntheticTurnId(entry.ctx.targetKey, entry.ctx.id),
        };
      }
    }

    const withPresentation: IntermediateEnvelope = {
      ...entry,
      ctx: {
        ...entry.ctx,
        turnId: entry.ctx.hasDeclaredTurn
          ? entry.ctx.declaredTurnId
          : active?.turnId,
        acpSource,
      },
    };

    if (kind === "result" || kind === "interrupted") {
      active = undefined;
    }

    return withPresentation;
  });
}

function buildSyntheticTurnId(targetKey: string, promptItemId: string): string {
  return encodeStructuredKey(
    "coding-session-presentation-turn/v1",
    targetKey,
    promptItemId,
  );
}

function toolCallPairingKey(targetKey: string, toolId: string): string {
  return encodeStructuredKey(
    "coding-session-tool-call-pairing/v1",
    targetKey,
    toolId,
  );
}

/**
 * Collision-free presentation scope for a target tuple.
 *
 * Deliberately its own domain rather than the wire `cs-target` key that
 * `codingSessionCommand` mints: this one only ever fences tool pairing and
 * turn reconstruction inside one projection pass, and giving it the wire key's
 * name would invite someone to sign it.
 */
function buildCodingSessionTranscriptScopeKey(
  target: CodingSessionTranscriptTarget,
): string {
  return encodeStructuredKey(
    "coding-session-transcript-scope/v1",
    target.driver,
    target.instanceId,
    target.sessionId,
    String(target.generation),
  );
}

type IntermediateEnvelope =
  | {
      kind: "projected";
      item: unknown;
      ctx: Identity;
    }
  | { kind: "fallback"; item: CodingSessionProjectedTranscriptItem };

function toIntermediate(
  envelopeRaw: unknown,
  options: ProjectCodingSessionTranscriptOptions,
): IntermediateEnvelope {
  try {
    const channelId = options.channelId ?? null;
    if (!isRecord(envelopeRaw)) {
      return {
        kind: "fallback",
        item: buildFallbackItem(
          envelopeRaw,
          null,
          toIsoTimestamp(Number.NaN),
          channelId,
        ),
      };
    }

    const eventSeq = extractEventSeq(envelopeRaw.eventSeq);
    const timestamp = toIsoTimestamp(
      typeof envelopeRaw.timestamp === "number"
        ? envelopeRaw.timestamp
        : Number.NaN,
    );
    const target = extractTarget(envelopeRaw.target);
    if (target === null) {
      return {
        kind: "fallback",
        item: buildFallbackItem(envelopeRaw, eventSeq, timestamp, channelId),
      };
    }

    const targetKey = buildCodingSessionTranscriptScopeKey(target);
    const sessionId = options.generationId ?? targetKey;
    return {
      kind: "projected",
      item: envelopeRaw.item,
      ctx: {
        // Both the caller's generation scope and the target tuple go into the
        // item id: `generationId` alone would collide if one batch ever mixed
        // targets, and `targetKey` alone would collide across channels.
        id: encodeStructuredKey(
          "coding-session-transcript-item/v1",
          sessionId,
          targetKey,
          eventSeq === null ? "unknown-seq" : String(eventSeq),
        ),
        sessionId,
        providerSessionId: target.sessionId,
        targetKey,
        channelId,
        sourceEventId: isExactSourceEventId(envelopeRaw.sourceEventId)
          ? envelopeRaw.sourceEventId
          : undefined,
        sourceEventSeq: eventSeq ?? undefined,
        timestamp,
        // Tri-state on purpose. A native envelope always carries the key, so
        // `null` is the producer saying "this item belongs to no turn" and
        // must not be overwritten by a guess. Only an envelope with no
        // `turnId` key at all falls back to synthetic reconstruction.
        hasDeclaredTurn: "turnId" in envelopeRaw,
        declaredTurnId:
          typeof envelopeRaw.turnId === "string" &&
          envelopeRaw.turnId.length > 0
            ? envelopeRaw.turnId
            : undefined,
        bridgeSource: options.bridgeSource ?? null,
      },
    };
  } catch {
    return {
      kind: "fallback",
      item: buildFallbackItem(
        envelopeRaw,
        null,
        toIsoTimestamp(Number.NaN),
        options.channelId ?? null,
      ),
    };
  }
}

function isExactSourceEventId(value: unknown): value is string {
  return typeof value === "string" && /^[0-9a-f]{64}$/.test(value);
}

function extractTarget(raw: unknown): CodingSessionTranscriptTarget | null {
  if (!isRecord(raw)) {
    return null;
  }
  const driver = typeof raw.driver === "string" ? raw.driver : null;
  const instanceId = typeof raw.instanceId === "string" ? raw.instanceId : null;
  const sessionId = typeof raw.sessionId === "string" ? raw.sessionId : null;
  const generation =
    typeof raw.generation === "number" && Number.isFinite(raw.generation)
      ? raw.generation
      : null;
  if (
    driver === null ||
    instanceId === null ||
    sessionId === null ||
    generation === null
  ) {
    return null;
  }
  return {
    driver,
    instanceId,
    sessionId,
    // Normalized so `-0` cannot silently collide with a real `+0` generation.
    generation: Object.is(generation, -0) ? 0 : generation,
  };
}

function extractEventSeq(raw: unknown): number | null {
  return typeof raw === "number" && Number.isFinite(raw) ? raw : null;
}

function buildFallbackId(raw: unknown, eventSeq: number | null): string {
  if (eventSeq !== null) {
    return `coding-session:unrecoverable:seq-${eventSeq}`;
  }
  let preview: string;
  try {
    preview = JSON.stringify(raw) ?? "unknown";
  } catch {
    // Cyclic or otherwise unstringifiable — still deterministic per call,
    // never a throw.
    preview = "unstringifiable";
  }
  return `coding-session:unrecoverable:${safeString(preview, 64)}`;
}

function buildFallbackItem(
  raw: unknown,
  eventSeq: number | null,
  timestamp: string,
  channelId: string | null,
): CodingSessionProjectedTranscriptItem {
  return {
    id: buildFallbackId(raw, eventSeq),
    type: "lifecycle",
    renderClass: "status",
    title: "Unrecognized transcript event",
    text: "Received a transcript event that could not be interpreted (missing/invalid target identity, or not an object). No content is surfaced.",
    timestamp,
    sessionId: null,
    channelId,
  };
}

/** Oracle copy of `projectTrustedCodingSessionTranscriptsToTranscript`. */
export function oracleProjectTrustedCodingSessionTranscriptsToTranscript(
  entries: readonly TrustedCodingSessionTranscriptEntry[],
  channelId: string,
  signerPubkey: string,
  target: CodingSessionCommandTarget,
  source?: CodingSessionIngressSource | null,
): TranscriptItem[] {
  const exact = entries
    .filter(
      (entry) =>
        entry.channelId === channelId &&
        entry.signerPubkey === signerPubkey &&
        entry.conflictCount === 0 &&
        sameTarget(entry.transcript.session, target),
    )
    .sort(
      (left, right) =>
        left.transcript.eventSeq - right.transcript.eventSeq ||
        left.eventId.localeCompare(right.eventId),
    );
  if (exact.length === 0) return [];

  return oracleProjectCodingSessionTranscript(
    exact.map((entry) => ({
      target: entry.transcript.session,
      eventSeq: entry.transcript.eventSeq,
      timestamp: entry.transcript.timestamp,
      turnId: entry.transcript.turnId,
      item: entry.transcript.item,
      sourceEventId: entry.eventId,
    })),
    {
      channelId,
      generationId: buildCodingSessionTranscriptGenerationId(
        channelId,
        signerPubkey,
        target,
      ),
      bridgeSource: {
        pubkey: signerPubkey,
        label: source?.label ?? "Trusted coding-session provider",
      },
    },
  );
}

function sameTarget(
  left: CodingSessionCommandTarget,
  right: CodingSessionCommandTarget,
): boolean {
  return (
    left.driver === right.driver &&
    left.instanceId === right.instanceId &&
    left.sessionId === right.sessionId &&
    left.generation === right.generation
  );
}

/** Oracle copy of `mergeTrustedCodingSessionIngress` at the source SHA. */
export function oracleMergeTrustedCodingSessionIngress(
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
    const transcript = oracleProjectTrustedCodingSessionTranscriptsToTranscript(
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
      lastTranscriptAt:
        targetTranscripts.length > 0
          ? targetTranscripts.reduce(
              (latest, entry) => Math.max(latest, entry.transcript.timestamp),
              0,
            )
          : null,
      status: metadata?.status ?? inferTranscriptStatus(targetTranscripts),
      // When the status itself was observed (44223 created_at, ms). Kept
      // separate from lastEventAt (a max over both streams) so status
      // derivation can compare metadata freshness against the transcript.
      statusAt: metadataEntry ? metadataEntry.createdAt * 1000 : null,
      statusEventId: metadataEntry?.eventId ?? null,
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
  });

  applyUmbrellaTurnBudget(sessions);

  sessions.sort(
    (left, right) =>
      Date.parse(right.lastEventAt) - Date.parse(left.lastEventAt) ||
      left.generationId.localeCompare(right.generationId),
  );
  return sessions;
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
