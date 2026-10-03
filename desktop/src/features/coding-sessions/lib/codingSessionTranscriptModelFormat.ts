import type { CodingSessionCostBasis } from "@/features/agents/ui/agentSessionTypes";
import { isCeremonialCompletionValue } from "@/features/coding-sessions/lib/codingSessionTranscriptModelPredicates";
import type { CodingSessionTurnCompletion } from "@/features/coding-sessions/lib/codingSessionTranscriptModelTypes";

/*
 * Turn-result parsing and the transcript's number formats, split out of
 * `codingSessionTranscriptModel.ts` for the 1000-line ceiling. That module
 * re-exports every public name here.
 */

export type ParsedTurnResult = {
  body: string;
  durationMs: number | null;
  costUsd: number | null;
};

export function parseCodingSessionTurnResult(text: string): ParsedTurnResult {
  let body = text.trim();
  let costUsd: number | null = null;
  let durationMs: number | null = null;

  const costMatch = body.match(/\s*\(\$([0-9]+(?:\.[0-9]+)?)\)\s*$/);
  if (costMatch) {
    costUsd = Number(costMatch[1]);
    body = body.slice(0, costMatch.index).trimEnd();
  }

  const durationMatch = body.match(/\s*\(([0-9]+(?:\.[0-9]+)?)ms\)\s*$/);
  if (durationMatch) {
    durationMs = Number(durationMatch[1]);
    body = body.slice(0, durationMatch.index).trimEnd();
  }

  return {
    body,
    durationMs: Number.isFinite(durationMs) ? durationMs : null,
    costUsd: Number.isFinite(costUsd) ? costUsd : null,
  };
}

export function formatCodingSessionDuration(durationMs: number): string {
  const seconds = Math.max(0, durationMs) / 1_000;
  if (seconds < 1) return `${Math.round(durationMs)}ms`;
  if (seconds < 10) return `${seconds.toFixed(1)}s`;
  if (seconds < 60) return `${Math.round(seconds)}s`;
  const minutes = Math.floor(seconds / 60);
  const remainder = Math.round(seconds % 60);
  return remainder > 0 ? `${minutes}m ${remainder}s` : `${minutes}m`;
}

export function formatCodingSessionCompletionOutcome(
  completion: CodingSessionTurnCompletion,
): string | null {
  const outcome = completion.outcome?.trim();
  if (!outcome) return null;
  const normalized = outcome.toLowerCase();
  if (isCeremonialCompletionValue(normalized)) return null;
  return normalized.replace(/[_-]+/g, " ");
}

export function formatCodingSessionCost(costUsd: number): string {
  if (costUsd < 0.01) return `$${costUsd.toFixed(4)}`;
  return `$${costUsd.toFixed(2)}`;
}

/**
 * The words that go beside a turn's dollar figure (ledger 272(d)). No figure
 * on the wire is an invoice: the adapter's is its own client-side estimate,
 * the table's is this project's rates applied to reported tokens, and a
 * record that names no basis is still an estimate.
 */
export function formatCodingSessionCostBasis(
  basis: CodingSessionCostBasis | null,
): string {
  switch (basis) {
    case "adapter_estimate":
      return "adapter estimate";
    case "table_estimate":
      return "price-table estimate";
    default:
      return "estimate";
  }
}
