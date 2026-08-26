/**
 * Pure adapter: signed 44225 transcript envelopes ->
 * `CodingSessionProjectedTranscriptItem[]` (the renderer's `TranscriptItem`
 * plus the coding-session `commandId` join key).
 *
 * This is the seam between the coding-session wire contract and Buzz's mature
 * agent-transcript renderer. No React, no I/O, no side effects.
 *
 * TRUST BOUNDARY: these envelopes arrive off a relay, authored by a provider
 * process, so both public entry points accept `unknown` — NOT the typed
 * envelope — and are defensive end to end: no field is assumed present or
 * well-typed, nothing is blindly cast, and nothing throws past this module's
 * boundary. Any input that doesn't match the expected shape (a non-object
 * envelope, a malformed target, a non-object item, even a cyclic or
 * BigInt-bearing value) degrades to a bounded fallback `TranscriptItem`
 * instead of throwing or being dropped.
 */

import { isRecord, safeString, toIsoTimestamp } from "./codingSessionDefensive";
import { encodeStructuredKey } from "./codingSessionKeys";
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

export type { CodingSessionBridgeSource, CodingSessionProjectedTranscriptItem };

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/** The `cs-target` tuple a transcript item belongs to. */
export type CodingSessionTranscriptTarget = {
  driver: string;
  instanceId: string;
  sessionId: string;
  generation: number;
};

/**
 * One decoded 44225 envelope.
 *
 * `turnId` is the producer's own turn identity when it has one. When it is
 * absent the projector falls back to reconstructing a synthetic turn from
 * prompt boundaries, exactly as it must for any producer that cannot name its
 * turns.
 */
export type CodingSessionTranscriptEnvelope = {
  target: CodingSessionTranscriptTarget;
  eventSeq: number;
  timestamp: number;
  item: unknown;
  turnId?: string | null;
};

/**
 * Caller-supplied scope. `channelId` is a Buzz channel id and `generationId`
 * the catalog's stable identity for this (channel, signer, target) — both are
 * the caller's to know, never derived from event content. `bridgeSource` comes
 * from the resolved ingress authority: it says whose claim is on screen.
 */
export type ProjectCodingSessionTranscriptOptions = {
  channelId?: string | null;
  generationId?: string | null;
  bridgeSource?: CodingSessionBridgeSource | null;
};

/**
 * TOTAL over hostile input, not just well-typed input: a non-array
 * `envelopes` value produces an empty array (nothing to iterate, no throw)
 * rather than crashing. Well-formed `tool_call` + later `tool_result` pairs
 * collapse into one completed tool card; malformed or orphaned elements still
 * degrade to bounded items instead of throwing.
 */
export function projectCodingSessionTranscript(
  envelopes: unknown,
  options: ProjectCodingSessionTranscriptOptions = {},
): CodingSessionProjectedTranscriptItem[] {
  if (!Array.isArray(envelopes)) {
    return [];
  }
  return projectCodingSessionTranscriptUnsafe(envelopes, options);
}

/**
 * TOTAL: every input produces exactly one `TranscriptItem`. Never throws and
 * never drops an input — even for `null`, `undefined`, a non-object, or a
 * target that is missing, malformed, or carries a BigInt `generation`.
 */
export function projectCodingSessionTranscriptItem(
  envelope: unknown,
  options: ProjectCodingSessionTranscriptOptions = {},
): CodingSessionProjectedTranscriptItem {
  try {
    const intermediate = toIntermediate(envelope, options);
    if (intermediate.kind === "fallback") {
      return intermediate.item;
    }
    const ctx = withoutBatchContext(intermediate.ctx, intermediate.item);
    return finalize(buildBaseTranscriptItem(intermediate.item, ctx), ctx);
  } catch {
    // Defense in depth. Every extraction below is already defensive
    // (typeof-guarded, never a blind cast), so this should be unreachable —
    // but a hostile input (a throwing getter, a Symbol.toPrimitive trap,
    // etc.) must still degrade to a bounded fallback item rather than take
    // down the whole stream. This is a backstop, not a substitute for the
    // validation above it.
    return buildFallbackItem(
      envelope,
      null,
      toIsoTimestamp(Number.NaN),
      options.channelId ?? null,
    );
  }
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
        result.push(finalize(plan, entry.ctx));
        continue;
      }

      result.push(finalize(buildToolCallItem(item, entry.ctx), entry.ctx));
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
        result[pending.index] = finalize(
          buildPairedToolResultItem(pending.item, pending.ctx, item, entry.ctx),
          pending.ctx,
        );
        if (toolId !== null) {
          pendingToolCalls.delete(
            toolCallPairingKey(entry.ctx.targetKey, toolId),
          );
        }
        continue;
      }
      result.push(finalize(buildToolResultItem(item, entry.ctx), entry.ctx));
      continue;
    }

    result.push(finalize(buildBaseTranscriptItem(item, entry.ctx), entry.ctx));
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

/** Single-envelope turn/source presentation, without any batch history. */
function withoutBatchContext(ctx: Identity, item: unknown): Identity {
  const isPrompt = isRecord(item) && item.kind === "user_prompt";
  const steered = isPrompt && item.steered === true;
  return {
    ...ctx,
    turnId: ctx.hasDeclaredTurn
      ? ctx.declaredTurnId
      : isPrompt && !steered
        ? buildSyntheticTurnId(ctx.targetKey, ctx.id)
        : undefined,
    acpSource: isPrompt
      ? steered
        ? "session/steer:user"
        : "session/prompt:user"
      : undefined,
  };
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
export function buildCodingSessionTranscriptScopeKey(
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
        targetKey,
        channelId,
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
