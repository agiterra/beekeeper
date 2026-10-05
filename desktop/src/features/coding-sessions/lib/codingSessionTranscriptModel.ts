import type { TranscriptItem } from "@/features/agents/ui/agentSessionTypes";
import {
  type CodingSessionSubagentPartition,
  partitionCodingSessionSubagentItems,
} from "@/features/coding-sessions/lib/codingSessionSubagents";
import { deriveCodingSessionChangedFiles } from "@/features/coding-sessions/lib/codingSessionTranscriptModelChanges";
import { deriveCodingSessionTurnFold } from "@/features/coding-sessions/lib/codingSessionTranscriptModelFold";
import { parseCodingSessionTurnResult } from "@/features/coding-sessions/lib/codingSessionTranscriptModelFormat";
import {
  isCeremonialCompletionValue,
  isCeremonialDiagnostic,
  isCompletedSuccessfulTool,
  isDiagnosticItem,
  isErrorItem,
  isCodingSessionSessionFactItem,
  isInterrupted,
  isSystemInitMetadata,
  isTurnResult,
  isTurnTerminal,
  leavesCodingSessionTranscript,
  normalizeContent,
} from "@/features/coding-sessions/lib/codingSessionTranscriptModelPredicates";
import { joinConsecutiveCodingSessionProse } from "@/features/coding-sessions/lib/codingSessionTranscriptModelText";
import { groupAdjacentTools } from "@/features/coding-sessions/lib/codingSessionTranscriptModelTools";
import type {
  CodingSessionTranscriptBlock,
  CodingSessionTranscriptModel,
  CodingSessionTranscriptTurn,
  CodingSessionTurnCompletion,
} from "@/features/coding-sessions/lib/codingSessionTranscriptModelTypes";

// One import path for every consumer: the model's pieces live in siblings
// (split for the 1000-line ceiling) and are re-exported from here.
export {
  deriveCodingSessionChangedFiles,
  deriveCodingSessionObservedChanges,
} from "@/features/coding-sessions/lib/codingSessionTranscriptModelChanges";
export {
  deriveCodingSessionTurnFold,
  formatFoldedFailures,
} from "@/features/coding-sessions/lib/codingSessionTranscriptModelFold";
export {
  formatCodingSessionCompletionOutcome,
  formatCodingSessionCost,
  formatCodingSessionCostBasis,
  formatCodingSessionDuration,
  parseCodingSessionTurnResult,
} from "@/features/coding-sessions/lib/codingSessionTranscriptModelFormat";
export {
  isCodingSessionBoundaryItem,
  isCodingSessionContinuityItem,
  isCodingSessionContinuityLoss,
  isCodingSessionSessionFactItem,
  leavesCodingSessionTranscript,
} from "@/features/coding-sessions/lib/codingSessionTranscriptModelPredicates";
export { stabilizeCodingSessionTranscriptModel } from "@/features/coding-sessions/lib/codingSessionTranscriptModelStability";
export {
  joinCodingSessionProseText,
  joinConsecutiveCodingSessionProse,
} from "@/features/coding-sessions/lib/codingSessionTranscriptModelText";
export { summarizeCodingSessionTools } from "@/features/coding-sessions/lib/codingSessionTranscriptModelTools";
export {
  codingSessionTranscriptEntryKey,
  type CodingSessionChangedFile,
  type CodingSessionChangedFileDiff,
  type CodingSessionObservedChanges,
  type CodingSessionTranscriptBlock,
  type CodingSessionTranscriptEntry,
  type CodingSessionTranscriptModel,
  type CodingSessionTranscriptStandalone,
  type CodingSessionTranscriptToolItem,
  type CodingSessionTranscriptTurn,
  type CodingSessionTurnCompletion,
  type CodingSessionTurnFold,
  type CodingSessionTurnRestingStatus,
  type CodingSessionTurnSettlement,
} from "@/features/coding-sessions/lib/codingSessionTranscriptModelTypes";
export {
  resolveCodingSessionExecutionRestingStatus,
  resolveCodingSessionTurnSettlement,
  resolveCodingSessionUmbrellaBlockRestingStatuses,
} from "@/features/coding-sessions/lib/codingSessionTranscriptModelSettlement";

type MutableTurn = {
  id: string;
  items: TranscriptItem[];
};

export function deriveCodingSessionTranscriptModel(
  transcript: TranscriptItem[],
  options: { isWorking: boolean },
): CodingSessionTranscriptModel {
  const rawOrdered: Array<MutableTurn | TranscriptItem> = [];
  const turnsById = new Map<string, MutableTurn>();
  // A subagent's items render under the call that spawned it, never in the
  // lead's reading order. They still join their turn, so its changed files
  // and start time count them.
  const subagents = partitionCodingSessionSubagentItems(transcript);
  // Continuity and boundary rows describe the session, not a turn (SV-16):
  // all of them are kept for Details and the composer's sandbox chip, and
  // the routine ones leave the reading order.
  const sessionFacts: TranscriptItem[] = [];

  for (const item of transcript) {
    if (isSystemInitMetadata(item)) continue;
    if (isCodingSessionSessionFactItem(item)) {
      sessionFacts.push(item);
      if (leavesCodingSessionTranscript(item)) continue;
    }
    if (subagents.nested.has(item) && !item.turnId) continue;

    if (!item.turnId) {
      rawOrdered.push(item);
      continue;
    }

    let turn = turnsById.get(item.turnId);
    if (!turn) {
      turn = { id: item.turnId, items: [] };
      turnsById.set(item.turnId, turn);
      rawOrdered.push(turn);
    }
    turn.items.push(item);
  }

  const ordered = coalesceStandaloneSettledTurns(rawOrdered);
  let lastTurn: MutableTurn | undefined;
  for (const candidate of ordered) {
    if (isMutableTurn(candidate)) lastTurn = candidate;
  }
  const supersededTurns = findSupersededTurns(ordered, transcript);
  const blocks: CodingSessionTranscriptBlock[] = [];
  const diagnostics: TranscriptItem[] = [];

  for (const candidate of ordered) {
    if (isMutableTurn(candidate)) {
      const turn = deriveTurn(
        candidate,
        options.isWorking && candidate === lastTurn,
        supersededTurns.has(candidate),
        subagents,
      );
      if (
        turn.entries.length > 0 ||
        turn.diagnostics.length > 0 ||
        turn.completion ||
        turn.isWorking
      ) {
        blocks.push(turn);
      }
      continue;
    }

    if (isDiagnosticItem(candidate)) {
      if (!isCeremonialDiagnostic(candidate)) diagnostics.push(candidate);
      continue;
    }
    blocks.push({
      kind: "standalone",
      id: candidate.id,
      entry: groupAdjacentTools([candidate], subagents)[0] ?? {
        kind: "item",
        item: candidate,
      },
    });
  }

  return { blocks, diagnostics, sessionFacts };
}

/**
 * The turns a later turn followed (SV-44): a turn is superseded only when
 * some later turn **started after its last item**. Turns are ordered by first
 * appearance, so "not the last turn" alone would end a turn that is still
 * producing — a later prompt with its own turn id while the earlier turn
 * runs (a queued prompt, or a provider that does not join a steer to the
 * running turn) would read the earlier turn's in-flight calls as settled.
 * Interleaved items say the earlier turn is still going, so it is not
 * superseded; its own completion, or a turn that begins after it went quiet,
 * ends it.
 */
function findSupersededTurns(
  ordered: ReadonlyArray<MutableTurn | TranscriptItem>,
  transcript: readonly TranscriptItem[],
): ReadonlySet<MutableTurn> {
  const position = new Map<TranscriptItem, number>();
  transcript.forEach((item, index) => {
    position.set(item, index);
  });
  const superseded = new Set<MutableTurn>();
  let latestLaterStart = -1;
  for (let index = ordered.length - 1; index >= 0; index -= 1) {
    const candidate = ordered[index];
    if (!candidate || !isMutableTurn(candidate)) continue;
    let first = Number.POSITIVE_INFINITY;
    let last = -1;
    for (const item of candidate.items) {
      const at = position.get(item);
      if (at === undefined) continue;
      first = Math.min(first, at);
      last = Math.max(last, at);
    }
    if (last >= 0 && latestLaterStart > last) superseded.add(candidate);
    if (Number.isFinite(first)) {
      latestLaterStart = Math.max(latestLaterStart, first);
    }
  }
  return superseded;
}

/**
 * Reconstructs only presentation-safe settled turns when the projection has
 * no authoritative provider turn id.
 *
 * Historical Hive streams can begin after a user prompt has already crossed
 * the bridge, leaving a same-generation sequence such as:
 *
 *   system init -> assistant text -> context telemetry -> terminal result
 *
 * All four entries are otherwise standalone, so the terminal result repeats
 * the assistant prose as a second visible row. A signed terminal event is the
 * one safe boundary available here: buffer only unscoped items from the same
 * session generation, promote them to one display turn when that terminal
 * arrives, and leave unterminated buffers exactly as standalone items. Never
 * cross a generation or an existing explicit turn.
 */
function coalesceStandaloneSettledTurns(
  ordered: Array<MutableTurn | TranscriptItem>,
): Array<MutableTurn | TranscriptItem> {
  const coalesced: Array<MutableTurn | TranscriptItem> = [];

  for (let index = 0; index < ordered.length; index += 1) {
    const candidate = ordered[index];
    if (!candidate) continue;
    if (isMutableTurn(candidate)) {
      coalesced.push(candidate);
      continue;
    }

    const sessionId = candidate.sessionId?.trim() ?? "";
    if (!sessionId) {
      coalesced.push(candidate);
      continue;
    }

    const items = [candidate];
    let terminalIndex = isTurnTerminal(candidate) ? index : -1;
    let cursor = index + 1;
    while (terminalIndex < 0 && cursor < ordered.length) {
      const next = ordered[cursor];
      if (
        !next ||
        isMutableTurn(next) ||
        next.sessionId?.trim() !== sessionId
      ) {
        break;
      }
      items.push(next);
      if (isTurnTerminal(next)) terminalIndex = cursor;
      cursor += 1;
    }

    if (terminalIndex >= 0) {
      const terminal = items.at(-1);
      if (!terminal) {
        coalesced.push(candidate);
        continue;
      }
      coalesced.push({
        id: `settled:${terminal.id}`,
        items,
      });
      index = terminalIndex;
    } else {
      coalesced.push(candidate);
    }
  }

  return coalesced;
}

function deriveTurn(
  turn: MutableTurn,
  canBeWorking: boolean,
  superseded: boolean,
  subagents: CodingSessionSubagentPartition,
): CodingSessionTranscriptTurn {
  const visible: TranscriptItem[] = [];
  const diagnostics: TranscriptItem[] = [];
  // Assistant rows synthesized from a Turn result's body. They are dropped
  // below when the body only echoes prose the turn already shows, and they
  // never join that prose: a body that is *not* an echo is its own words.
  const resultBodies = new Set<TranscriptItem>();
  let completion: CodingSessionTurnCompletion | null = null;

  for (const item of turn.items) {
    if (subagents.nested.has(item)) continue;
    if (isTurnResult(item)) {
      // Structured `durationMs`/`costUsd` on the item are authoritative. The
      // regex parse remains only for already-published events whose builders
      // baked the metrics into the display text; on a structured item the
      // text carries no suffixes, so parsing it is a harmless trim.
      const result = parseCodingSessionTurnResult(item.text);
      completion = {
        durationMs:
          typeof item.durationMs === "number"
            ? item.durationMs
            : result.durationMs,
        costUsd:
          typeof item.costUsd === "number" ? item.costUsd : result.costUsd,
        costBasis:
          typeof item.costUsd === "number" ? (item.costBasis ?? null) : null,
        outcome: item.outcome?.trim() || null,
        timestamp: item.timestamp,
        state: isErrorItem(item) ? "failed" : "completed",
      };

      if (isErrorItem(item)) {
        visible.push({
          ...item,
          text: result.body || item.text,
        });
      } else if (result.body && !isCeremonialCompletionValue(result.body)) {
        const body: TranscriptItem = {
          id: `${item.id}:assistant-result`,
          type: "message",
          renderClass: "message",
          role: "assistant",
          title: "Assistant",
          text: result.body,
          timestamp: item.timestamp,
          turnId: item.turnId,
          sessionId: item.sessionId,
          channelId: item.channelId,
          bridgeSource: item.bridgeSource,
        };
        resultBodies.add(body);
        visible.push(body);
      }
      continue;
    }

    if (isInterrupted(item)) {
      completion = {
        durationMs: null,
        costUsd: null,
        costBasis: null,
        outcome: "interrupted",
        timestamp: item.timestamp,
        state: "interrupted",
      };
      continue;
    }

    if (isDiagnosticItem(item)) {
      if (isErrorItem(item)) {
        visible.push(item);
      } else if (!isCeremonialDiagnostic(item)) {
        diagnostics.push(item);
      }
      continue;
    }

    visible.push(item);
  }

  const joined = joinConsecutiveCodingSessionProse(visible, resultBodies);
  const echoes = assistantResultEchoes(turn.items, joined, resultBodies);
  const narrative = joined.filter(
    (item) =>
      !resultBodies.has(item) ||
      item.type !== "message" ||
      !echoes.has(normalizeContent(item.text)),
  );
  const isWorking = canBeWorking && completion === null;
  // Only a completed turn folds (a failed or stopped one stays open), so only
  // there may a failed step before the answer join its neighbours' group.
  const entries = groupAdjacentTools(narrative, subagents, {
    foldsSettledWork: !isWorking && completion?.state === "completed",
  });
  const startedAt = deriveTurnStartedAt(turn.items);

  return {
    kind: "turn",
    id: turn.id,
    entries,
    changedFiles: deriveCodingSessionChangedFiles(turn.items),
    diagnostics,
    completion,
    isWorking,
    // Only a turn with no completion of its own needs a later turn to end it.
    // Keeping the flag off a completed turn keeps that turn's object stable
    // when the next prompt arrives.
    superseded: superseded && completion === null,
    startedAt,
    fold: deriveCodingSessionTurnFold({
      completion,
      entries,
      isWorking,
      startedAt,
    }),
  };
}

/**
 * Every rendering of the turn's prose a result body could be repeating.
 *
 * The body of a Turn result is usually the answer again. With the answer now
 * published a paragraph at a time, no single item equals it: the joined run
 * does, and so — for an answer split by a tool call — may the whole turn's
 * prose read end to end. Each raw item stays a candidate too, as before.
 */
function assistantResultEchoes(
  raw: readonly TranscriptItem[],
  joined: readonly TranscriptItem[],
  resultBodies: ReadonlySet<TranscriptItem>,
): Set<string> {
  const echoes = new Set<string>();
  let wholeTurn = "";
  for (const item of raw) {
    if (item.type !== "message" || item.role !== "assistant") continue;
    echoes.add(normalizeContent(item.text));
    wholeTurn += item.text;
  }
  for (const item of joined) {
    if (resultBodies.has(item)) continue;
    if (item.type === "message" && item.role === "assistant") {
      echoes.add(normalizeContent(item.text));
    }
  }
  if (wholeTurn) echoes.add(normalizeContent(wholeTurn));
  return echoes;
}

function deriveTurnStartedAt(items: TranscriptItem[]): string | null {
  const prompt = items.find(
    (item) => item.type === "message" && item.role === "user",
  );
  if (prompt) return prompt.timestamp;

  const first = items[0];
  if (!first) return null;
  if (first.type === "tool") return first.startedAt || first.timestamp;
  return first.timestamp;
}

function isMutableTurn(
  value: MutableTurn | TranscriptItem,
): value is MutableTurn {
  return "items" in value;
}

export function isCompletedSuccessfulCodingSessionTool(
  item: TranscriptItem,
): item is Extract<TranscriptItem, { type: "tool" }> {
  return isCompletedSuccessfulTool(item);
}

export function isCodingSessionTranscriptError(item: TranscriptItem): boolean {
  return isErrorItem(item);
}
