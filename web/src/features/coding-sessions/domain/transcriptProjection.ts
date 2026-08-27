/**
 * Pure adapter: verified 44225 envelopes -> {@link ProjectedTranscriptItem}[].
 *
 * The envelope layer of
 * `desktop/src/features/coding-sessions/lib/codingSessionTranscriptProjection.ts`,
 * rewritten against the web-local row type: ordering, turn reconstruction and
 * tool pairing are the desktop rules, per D9.
 *
 * TRUST BOUNDARY: envelopes arrive off a relay, authored by a provider
 * process, so the entry point accepts `unknown` and is defensive end to end.
 * Nothing throws past this module's boundary.
 */
import { isRecord } from "./defensive.ts";
import {
  buildCodingSessionTranscriptScopeKey,
  encodeStructuredKey,
} from "./keys.ts";
import {
  buildNonToolItem,
  buildOrphanToolResultItem,
  buildPairedToolItem,
  buildToolCallItem,
  toolIdFromToolCall,
  toolIdFromToolResult,
  type TranscriptItemIdentity,
} from "./transcriptItems.ts";
import type { CodingSessionTarget, ProjectedTranscriptItem } from "./types.ts";
import { hasOwnKey } from "./wireDecode.ts";

/** One verified 44225, as the projector consumes it. */
export type CodingSessionTranscriptEnvelope = {
  target: CodingSessionTarget;
  eventSeq: number;
  timestamp: number;
  /**
   * Tri-state on purpose. A native envelope always carries the key, so `null`
   * is the producer saying "this item belongs to no turn" and must not be
   * overwritten by a guess. Only an envelope with no `turnId` key at all falls
   * back to synthetic reconstruction.
   */
  turnId?: string | null;
  item: unknown;
  /** Relay event id — the ordering tie-break. Never lexicographic on seq. */
  eventId: string;
};

export type ProjectCodingSessionTranscriptOptions = {
  /** Stable identity for this (channel, signer, target) stream. */
  generationId?: string | null;
  /** The block this stream renders as. Defaults to the target scope key. */
  blockKey?: string | null;
};

/**
 * TOTAL over hostile input: a non-array `envelopes` value produces an empty
 * array rather than throwing.
 *
 * Order is by `eventSeq` **numerically**, ties broken by event id — a
 * lexicographic sort on the sequence would put item 10 before item 9.
 */
export function projectCodingSessionTranscript(
  envelopes: unknown,
  options: ProjectCodingSessionTranscriptOptions = {},
): ProjectedTranscriptItem[] {
  if (!Array.isArray(envelopes)) return [];
  const ordered = [...envelopes]
    .filter((envelope): envelope is CodingSessionTranscriptEnvelope =>
      isRecord(envelope),
    )
    .sort(
      (left, right) =>
        numericSeq(left.eventSeq) - numericSeq(right.eventSeq) ||
        String(left.eventId ?? "").localeCompare(String(right.eventId ?? "")),
    );
  const withTurns = assignTurnPresentation(ordered, options);
  const result: ProjectedTranscriptItem[] = [];
  const pendingToolCalls = new Map<string, number>();

  for (const entry of withTurns) {
    const item = entry.item;
    if (isRecord(item) && item.kind === "tool_call") {
      result.push(buildToolCallItem(item, entry.identity));
      const toolId = toolIdFromToolCall(item);
      if (toolId !== null) {
        pendingToolCalls.set(
          toolPairingKey(entry.identity.targetKey, toolId),
          result.length - 1,
        );
      }
      continue;
    }
    if (isRecord(item) && item.kind === "tool_result") {
      const toolId = toolIdFromToolResult(item);
      const pairingKey =
        toolId === null
          ? null
          : toolPairingKey(entry.identity.targetKey, toolId);
      const index =
        pairingKey === null ? undefined : pendingToolCalls.get(pairingKey);
      if (index !== undefined && pairingKey !== null) {
        result[index] = buildPairedToolItem(result[index], item);
        pendingToolCalls.delete(pairingKey);
        continue;
      }
      result.push(buildOrphanToolResultItem(item, entry.identity));
      continue;
    }
    result.push(buildNonToolItem(item, entry.identity));
  }
  return result;
}

type TurnedEnvelope = {
  item: unknown;
  identity: TranscriptItemIdentity;
};

/**
 * Turn reconstruction.
 *
 * When an envelope carries a `turnId` that identity wins outright — including
 * an explicit `null`, which means "belongs to no turn". Otherwise a fresh,
 * non-steered user prompt is the only safe boundary from which a turn can be
 * derived, a target change fences the open turn, and `result`/`interrupted`
 * close it. Orphan telemetry stays ungrouped rather than being guessed into a
 * turn.
 */
function assignTurnPresentation(
  envelopes: readonly CodingSessionTranscriptEnvelope[],
  options: ProjectCodingSessionTranscriptOptions,
): TurnedEnvelope[] {
  let active: { targetKey: string; turnId: string } | undefined;
  return envelopes.map((envelope) => {
    const target = normalizeTarget(envelope.target);
    const targetKey =
      target === null
        ? "coding-session-transcript-scope/v1|unknown"
        : buildCodingSessionTranscriptScopeKey(target);
    const item = envelope.item;
    const kind =
      isRecord(item) && typeof item.kind === "string" ? item.kind : null;
    if (active?.targetKey !== targetKey) active = undefined;

    const eventSeq = numericSeq(envelope.eventSeq);
    const id = encodeStructuredKey(
      "coding-session-transcript-item/v1",
      options.generationId ?? targetKey,
      targetKey,
      Number.isFinite(eventSeq) ? String(eventSeq) : "unknown-seq",
      String(envelope.eventId ?? ""),
    );

    if (kind === "user_prompt" && !(isRecord(item) && item.steered === true)) {
      active = {
        targetKey,
        turnId: encodeStructuredKey(
          "coding-session-presentation-turn/v1",
          targetKey,
          id,
        ),
      };
    }
    const declared = hasOwnKey(envelope, "turnId");
    const identity: TranscriptItemIdentity = {
      id,
      blockKey: options.blockKey ?? targetKey,
      targetKey,
      turnId: declared
        ? typeof envelope.turnId === "string" && envelope.turnId.length > 0
          ? envelope.turnId
          : null
        : (active?.turnId ?? null),
      timestamp:
        typeof envelope.timestamp === "number" &&
        Number.isFinite(envelope.timestamp)
          ? envelope.timestamp
          : 0,
      eventSeq: Number.isFinite(eventSeq) ? eventSeq : 0,
    };
    if (kind === "result" || kind === "interrupted") active = undefined;
    return { item, identity };
  });
}

function numericSeq(value: unknown): number {
  return typeof value === "number" && Number.isFinite(value)
    ? value
    : Number.NaN;
}

function normalizeTarget(raw: unknown): CodingSessionTarget | null {
  if (!isRecord(raw)) return null;
  const { driver, instanceId, sessionId, generation } = raw;
  if (
    typeof driver !== "string" ||
    typeof instanceId !== "string" ||
    typeof sessionId !== "string" ||
    typeof generation !== "number" ||
    !Number.isFinite(generation)
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

function toolPairingKey(targetKey: string, toolId: string): string {
  return encodeStructuredKey(
    "coding-session-tool-call-pairing/v1",
    targetKey,
    toolId,
  );
}

/** One execution's rows, labelled. Sessions interleave these, never items. */
export type CodingSessionTranscriptBlock = {
  blockKey: string;
  /** "runtime · model", or the agentRef when the provider named one. */
  label: string;
  items: ProjectedTranscriptItem[];
  /** Earliest item timestamp, for block ordering. */
  startedAt: number;
};

/**
 * Order a multi-execution session's blocks.
 *
 * Blocks interleave, individual items never do: two providers' streams have
 * no shared clock, so merging them item-by-item would invent an ordering the
 * relay never proved.
 */
export function orderCodingSessionTranscriptBlocks(
  blocks: readonly CodingSessionTranscriptBlock[],
): CodingSessionTranscriptBlock[] {
  return [...blocks].sort(
    (left, right) =>
      left.startedAt - right.startedAt ||
      left.blockKey.localeCompare(right.blockKey),
  );
}
