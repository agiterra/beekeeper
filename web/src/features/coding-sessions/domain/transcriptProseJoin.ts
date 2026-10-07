/**
 * Join consecutive same-attribution prose pieces back into the one message
 * the agent wrote, and say which message is still arriving.
 *
 * A coding-session answer reaches the wire as several immutable kind:44225
 * `assistant_text` items — cut at 24 KiB today, at every tool call, and at
 * paragraph boundaries once `BEEKEEPER_CSP_TRANSCRIPT_PARAGRAPH_FLUSH` is on. The
 * normative rule is NIP-CST amendment 3, paragraph **Join key**, restated with
 * executable vectors in `conformance/transcript-prose-join/CONTRACT.md`; this
 * module implements it and `transcriptProseJoin.test.mjs` binds every vector.
 *
 * T3 Code models the same thing as one message whose text grows, flagged
 * `streaming` (`apps/web/src/components/chat/MessagesTimeline.logic.ts:640-647`).
 * We cannot: a signed piece cannot grow, and a signed "still streaming" flag
 * could never be retracted by a provider that crashed. So `arriving` is
 * derived from turn state AND target liveness, never from the producer.
 *
 * Pure and total: nothing here throws, and no input is mutated.
 */
import type {
  CodingSessionGenerationRecord,
  CodingSessionReachabilityReport,
  ProjectedTranscriptItem,
} from "./types.ts";

/** What the join reads about one transcript item, in `eventSeq` order. */
export type ProseJoinEntry = {
  /**
   * Index of the row this item produced in the projection's row list, or null
   * when it produced none of its own (a `tool_result` folded into its call).
   * An item with no row still sits between two pieces and still ends a
   * message — that is the whole reason this list exists beside the rows.
   */
  rowIndex: number | null;
  /** Exact target scope: signer stream + driver + instance + session + generation. */
  targetKey: string;
  turnId: string | null;
  /** The item's wire `kind`, or null when it had none. */
  kind: string | null;
  parentToolId: string | null;
  messageId: string | null;
};

type OpenMessage = {
  headIndex: number;
  kind: string;
  turnId: string | null;
  parentToolId: string | null;
  messageId: string | null;
};

const PROSE_KINDS = new Set(["assistant_text", "reasoning"]);
const TURN_ENDING_KINDS = new Set(["result", "interrupted"]);

/**
 * The kind:44223 statuses that end a session's target (CONTRACT rule 7).
 * `interrupted` and `failed` are deliberately absent: the producer publishes
 * them at the end of an ordinary turn while the session goes on, and that
 * turn's own `result`/`interrupted` item already ends its message.
 */
export const SESSION_ENDING_STATUSES: ReadonlySet<string> = new Set([
  "completed",
  "stopped",
  "disconnected",
]);

/**
 * Rule 1-6 of the contract, over one projection's rows.
 *
 * `entries` must be in `eventSeq` order with repeated deliveries already
 * dropped (rule 0) — the projection does both. Two prose pieces join when
 * they are adjacent within one exact target, the same kind, the same
 * `turnId` (`null` equals `null`) and the same `parentToolId`, and their
 * `messageId`s do not differ. Anything else in that target between them ends
 * the message. Text is concatenated with nothing inserted or trimmed; the
 * joined row keeps the first piece's id, timestamp and `firstEventId`.
 *
 * Also marks `awaitingTurnEnd` on the message that is the last item of a turn
 * with no `result`/`interrupted` yet. `arriving` stays false here: whether
 * anyone is still writing is a liveness question this pass cannot answer
 * (see {@link applyCodingSessionTranscriptArriving}).
 */
export function joinTranscriptProse(
  rows: readonly ProjectedTranscriptItem[],
  entries: readonly ProseJoinEntry[],
): ProjectedTranscriptItem[] {
  const merged: (ProjectedTranscriptItem | null)[] = [...rows];
  const open = new Map<string, OpenMessage>();
  const lastOfTurn = new Map<string, number | null>();
  const endedTurns = new Set<string>();

  for (const entry of entries) {
    const row = entry.rowIndex === null ? null : merged[entry.rowIndex];
    const isProse =
      entry.kind !== null &&
      PROSE_KINDS.has(entry.kind) &&
      entry.rowIndex !== null &&
      row !== null &&
      row !== undefined;
    let headIndex: number | null = null;

    if (isProse && entry.rowIndex !== null && entry.kind !== null) {
      const current = open.get(entry.targetKey);
      const head = current === undefined ? null : merged[current.headIndex];
      if (
        current !== undefined &&
        head !== null &&
        head !== undefined &&
        current.kind === entry.kind &&
        current.turnId === entry.turnId &&
        current.parentToolId === entry.parentToolId &&
        !(
          entry.messageId !== null &&
          current.messageId !== null &&
          entry.messageId !== current.messageId
        )
      ) {
        const piece = merged[entry.rowIndex];
        merged[current.headIndex] = {
          ...head,
          text: head.text + (piece?.text ?? ""),
          lastEventId: piece?.lastEventId ?? head.lastEventId,
        };
        merged[entry.rowIndex] = null;
        current.messageId ??= entry.messageId;
        headIndex = current.headIndex;
      } else {
        open.set(entry.targetKey, {
          headIndex: entry.rowIndex,
          kind: entry.kind,
          turnId: entry.turnId,
          parentToolId: entry.parentToolId,
          messageId: entry.messageId,
        });
        headIndex = entry.rowIndex;
      }
    } else {
      open.delete(entry.targetKey);
    }

    if (entry.turnId !== null) {
      const turnKey = scopedTurnKey(entry.targetKey, entry.turnId);
      lastOfTurn.set(turnKey, headIndex);
      if (entry.kind !== null && TURN_ENDING_KINDS.has(entry.kind)) {
        endedTurns.add(turnKey);
      }
    }
  }

  for (const [turnKey, headIndex] of lastOfTurn) {
    if (headIndex === null || endedTurns.has(turnKey)) continue;
    const head = merged[headIndex];
    if (head) merged[headIndex] = { ...head, awaitingTurnEnd: true };
  }

  return merged.filter((row): row is ProjectedTranscriptItem => row !== null);
}

function scopedTurnKey(targetKey: string, turnId: string): string {
  return JSON.stringify([targetKey, turnId]);
}

/** What a reader knows about whether one exact target can still be writing. */
export type TranscriptTargetLiveness = {
  /** The same signer + driver + instance + session has a higher generation. */
  superseded: boolean;
  /** The target's latest kind:44223 metadata status, or null with none. */
  status: string | null;
  /**
   * The reader's view, at its own `now`, of the target's winning kind:24223
   * lease; null when it holds none (or has not finished reading leases).
   */
  lease: "live" | "released" | "lapsed" | null;
};

/**
 * CONTRACT rule 7's target half. A target is live only with a live,
 * unexpired lease, no higher generation, and no session-ending status. A
 * reader holding no lease shows nothing as arriving — it never falls back to
 * turn state alone, because a provider that slept or crashed never publishes
 * the `result` that would clear the line.
 */
export function isTranscriptTargetLive(
  liveness: TranscriptTargetLiveness,
): boolean {
  return (
    !liveness.superseded &&
    liveness.lease === "live" &&
    !(liveness.status !== null && SESSION_ENDING_STATUSES.has(liveness.status))
  );
}

/**
 * Set `arriving` on one target's rows: exactly the rows awaiting their turn's
 * end, and only when the target is live. Rows are returned unchanged
 * otherwise (with `arriving` cleared, so a stale `true` cannot survive).
 */
export function applyCodingSessionTranscriptArriving(
  items: readonly ProjectedTranscriptItem[],
  targetLive: boolean,
): ProjectedTranscriptItem[] {
  return items.map((item) => {
    const arriving = targetLive && item.awaitingTurnEnd === true;
    return (item.arriving ?? false) === arriving ? item : { ...item, arriving };
  });
}

/**
 * The browser observer's liveness for one generation.
 *
 * `provider_reachable` is the D8 lease fold's verdict (`lease.ts`): a single
 * winning live lease, younger than the client TTL, naming the accepted
 * command, for the execution's CURRENT generation — so it already answers
 * "live lease" and "not superseded". `unknown` (leases not read yet) and
 * `no_provider_answering` both mean no lease the reader can stand on.
 */
export function codingSessionTranscriptLiveness(
  generation: Pick<CodingSessionGenerationRecord, "status">,
  report: CodingSessionReachabilityReport | null | undefined,
): TranscriptTargetLiveness {
  const reachable = report?.reachability === "provider_reachable";
  return {
    // A superseded generation is never `provider_reachable` (the fold demands
    // the current generation), so the lease verdict below already carries it.
    superseded: false,
    status: generation.status,
    lease: reachable ? "live" : null,
  };
}

/**
 * A generation's rows with `arriving` resolved — the one call a surface makes
 * once it holds the generation's reachability report.
 */
export function codingSessionGenerationTranscript(
  generation: Pick<CodingSessionGenerationRecord, "status" | "transcript">,
  report: CodingSessionReachabilityReport | null | undefined,
): ProjectedTranscriptItem[] {
  return applyCodingSessionTranscriptArriving(
    generation.transcript,
    isTranscriptTargetLive(codingSessionTranscriptLiveness(generation, report)),
  );
}
