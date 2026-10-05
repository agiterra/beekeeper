import * as React from "react";

import { useCodingSessionHandover } from "@/features/coding-sessions/hooks/useCodingSessionHandover";
import type { CodingSessionMinimapItem } from "@/features/coding-sessions/lib/codingSessionTranscriptMinimapItems";
import {
  codingSessionMinimapOpenDecisionRulings,
  codingSessionMinimapOpenDecisionSource,
  codingSessionMinimapRulingNote,
  type CodingSessionMinimapDecisionReads,
  deriveCodingSessionMinimapMarks,
  type CodingSessionMinimapGateInput,
  type CodingSessionMinimapHandoverInput,
  type CodingSessionMinimapMarks,
  type CodingSessionMinimapRulingInput,
} from "@/features/coding-sessions/lib/codingSessionTranscriptMinimapMarks";
import type { CodingSessionSurfaceCtx } from "./surfaces/codingSessionSurfaceContext";

/**
 * The signed facts the minimap marks (SV-27), read from the surface `ctx`
 * the workspace already built — the view's one observations read, the
 * fold's `decisions[]` and the `decision.request` rows that date them —
 * plus the umbrella's handover records (kind 44247), read through the same
 * query key the Handover panel uses, so the two share one cached read.
 *
 * `notes` are the disclosures the hover card prints when a source was not
 * read: a missing mark over an unread source is "not known", never "none".
 */
export type CodingSessionMinimapFacts = {
  marks: ReadonlyMap<string, CodingSessionMinimapMarks>;
  /** The gate source's state, for the card's gate line. */
  gates: { state: "read" } | { state: "unread"; reason: string };
  notes: readonly string[];
};

export function useCodingSessionTranscriptMinimapFacts(
  ctx: CodingSessionSurfaceCtx | null,
  items: readonly CodingSessionMinimapItem[],
): CodingSessionMinimapFacts {
  const umbrella = ctx?.umbrella ?? null;
  const handoverScope =
    ctx !== null &&
    umbrella !== null &&
    umbrella.sessionRef !== null &&
    umbrella.genesisRef !== null &&
    umbrella.founderPubkey !== null
      ? {
          channelRef: ctx.channelId,
          sessionRef: umbrella.sessionRef,
          genesisRef: umbrella.genesisRef,
          founderPubkey: umbrella.founderPubkey,
        }
      : null;
  const handover = useCodingSessionHandover(handoverScope);

  const observations = ctx?.observations ?? null;
  const gateInputs = React.useMemo<CodingSessionMinimapGateInput[]>(() => {
    if (observations?.state !== "read" || observations.result === null) {
      return [];
    }
    const signedAt = observations.result.signedAt;
    return observations.view.gates.map((gate) => {
      const seconds = signedAt.get(gate.sourceEventId);
      return {
        gate: gate.gate,
        outcome: gate.outcome,
        signedAtMs: seconds === undefined ? null : seconds * 1_000,
      };
    });
  }, [observations]);

  const handoverFold = handover.fold;
  const handoverInputs = React.useMemo<
    CodingSessionMinimapHandoverInput[]
  >(() => {
    if (handoverFold === null) return [];
    return [
      ...handoverFold.checkpoints.map((record) => ({
        type: "checkpoint" as const,
        signedAtMs: record.createdAt * 1_000,
      })),
      ...handoverFold.continuations.map((record) => ({
        type: "continuation" as const,
        signedAtMs: record.createdAt * 1_000,
      })),
    ];
  }, [handoverFold]);

  // The fold's `decisions[]` say which requests are open; the signed
  // `decision.request` rows date them (DB8).
  const decisions = ctx?.decisions ?? null;
  const decisionRequests = ctx?.decisionRequests ?? null;
  const hasCtx = ctx !== null;
  const decisionCtx = React.useMemo<CodingSessionMinimapDecisionReads | null>(
    () => (hasCtx ? { decisions, decisionRequests } : null),
    [hasCtx, decisions, decisionRequests],
  );
  const decisionSource = React.useMemo(
    () => codingSessionMinimapOpenDecisionSource(decisionCtx),
    [decisionCtx],
  );
  const rulingNote = React.useMemo(
    () => codingSessionMinimapRulingNote(decisionCtx),
    [decisionCtx],
  );
  const rulingInputs = React.useMemo<CodingSessionMinimapRulingInput[]>(
    () =>
      decisionSource === null
        ? []
        : codingSessionMinimapOpenDecisionRulings(decisionSource),
    [decisionSource],
  );

  const marks = React.useMemo(
    () =>
      deriveCodingSessionMinimapMarks({
        turns: items.map((item) => ({
          id: item.id,
          startedAtMs: item.startedAtMs,
          failed: item.failed,
        })),
        gates: gateInputs,
        handovers: handoverInputs,
        rulings: rulingInputs,
      }),
    [gateInputs, handoverInputs, items, rulingInputs],
  );

  const gates = React.useMemo<CodingSessionMinimapFacts["gates"]>(() => {
    if (observations === null) {
      return { state: "unread", reason: "Gate rows not read in this view" };
    }
    if (observations.state === "not-read") {
      return { state: "unread", reason: observations.reason };
    }
    if (observations.errorMessage !== null) {
      return {
        state: "unread",
        reason: `Gate rows could not be read: ${observations.errorMessage}`,
      };
    }
    if (observations.result === null) {
      return { state: "unread", reason: "Gate rows still loading" };
    }
    return { state: "read" };
  }, [observations]);

  const notes = React.useMemo(() => {
    const lines: string[] = [];
    if (handoverScope !== null && handover.errorMessage !== null) {
      lines.push("Handover records could not be read");
    }
    if (handoverScope !== null && rulingNote !== null) {
      lines.push(rulingNote);
    }
    return lines;
  }, [handover.errorMessage, handoverScope, rulingNote]);

  return { marks, gates, notes };
}
