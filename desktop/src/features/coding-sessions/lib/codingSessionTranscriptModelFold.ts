import type { TranscriptItem } from "@/features/agents/ui/agentSessionTypes";
import {
  isCompletedSuccessfulTool,
  isErrorItem,
} from "@/features/coding-sessions/lib/codingSessionTranscriptModelPredicates";
import { summarizeCodingSessionTools } from "@/features/coding-sessions/lib/codingSessionTranscriptModelTools";
import type {
  CodingSessionTranscriptEntry,
  CodingSessionTranscriptToolItem,
  CodingSessionTranscriptTurn,
  CodingSessionTurnFold,
} from "@/features/coding-sessions/lib/codingSessionTranscriptModelTypes";

/**
 * How a settled turn folds behind one "Worked for …" row.
 *
 * Nothing folds while the turn is live — that is when the work is being
 * watched. Once it settles, the work that produced the answer (tool calls,
 * thoughts, plan snapshots, intermediate prose) folds; the reader keeps
 * what the turn *says* and anything that went wrong:
 *
 * - every prompt, including a steered one (attribution);
 * - the final assistant message (the answer);
 * - failures — a failed tool call, a lifecycle error;
 * - anything that asked the person something (permission rows) and every
 *   other non-error lifecycle row, which is continuity rather than work;
 * - subagent batches, which name who did the work;
 * - a tool call that never finished, which is a truth about the turn, not
 *   detail.
 *
 * A turn whose only foldable entries are thoughts does not fold: a "Worked
 * for" row that hides nothing but reasoning would be ceremony.
 *
 * Changed files and the stop/failure state live outside `entries` and are
 * never folded. Split out of `codingSessionTranscriptModel.ts`.
 */
export function deriveCodingSessionTurnFold(
  turn: Pick<
    CodingSessionTranscriptTurn,
    "completion" | "entries" | "isWorking" | "startedAt"
  >,
): CodingSessionTurnFold | null {
  if (turn.isWorking || !turn.completion) return null;

  // The answer is the last prose the agent wrote. A Turn result body that
  // repeats nothing the turn shows is synthesized as its own assistant row
  // (`:assistant-result`); it is the provider's closing word, so it stays
  // visible too, but it never displaces the agent's own last message.
  let terminalAnswerIndex = -1;
  turn.entries.forEach((entry, index) => {
    if (isAssistantMessageEntry(entry) && !isResultBodyEntry(entry)) {
      terminalAnswerIndex = index;
    }
  });

  const hiddenIndexes: number[] = [];
  const hiddenTools: CodingSessionTranscriptToolItem[] = [];
  let hidesWork = false;
  turn.entries.forEach((entry, index) => {
    if (index === terminalAnswerIndex || isResultBodyEntry(entry)) return;
    const fold = classifyFoldable(entry);
    if (fold === "keep") return;
    hiddenIndexes.push(index);
    if (fold !== "thought") hidesWork = true;
    if (entry.kind === "tool-group") hiddenTools.push(...entry.items);
    else if (entry.kind === "item" && entry.item.type === "tool") {
      hiddenTools.push(entry.item);
    }
  });

  const anchorIndex = hiddenIndexes[0];
  if (!hidesWork || anchorIndex === undefined) return null;
  return {
    anchorIndex,
    hiddenIndexes,
    durationMs: turnDurationMs(turn),
    summary: summarizeCodingSessionTools(hiddenTools),
  };
}

function classifyFoldable(
  entry: CodingSessionTranscriptEntry,
): "keep" | "work" | "thought" {
  if (entry.kind === "tool-group") return "work";
  if (entry.kind === "subagents") return "keep";
  const item: TranscriptItem = entry.item;
  switch (item.type) {
    case "message":
      return item.role === "assistant" ? "work" : "keep";
    case "thought":
      return "thought";
    case "plan":
      return "work";
    case "tool":
      return isCompletedSuccessfulTool(item) && !isErrorItem(item)
        ? "work"
        : "keep";
    default:
      return "keep";
  }
}

function isResultBodyEntry(entry: CodingSessionTranscriptEntry) {
  return entry.kind === "item" && entry.item.id.endsWith(":assistant-result");
}

function isAssistantMessageEntry(entry: CodingSessionTranscriptEntry) {
  return (
    entry.kind === "item" &&
    entry.item.type === "message" &&
    entry.item.role === "assistant"
  );
}

/**
 * The provider's measured duration, else prompt-to-terminal wall time.
 *
 * A stopped turn carries no measured duration; its start and its terminal
 * row's timestamp are both signed, so their difference is a fact rather than
 * an estimate. Anything unparseable or negative is `null`, never zero.
 */
function turnDurationMs(
  turn: Pick<CodingSessionTranscriptTurn, "completion" | "startedAt">,
): number | null {
  const completion = turn.completion;
  if (!completion) return null;
  if (completion.durationMs !== null) return completion.durationMs;
  if (!turn.startedAt) return null;
  const start = Date.parse(turn.startedAt);
  const end = Date.parse(completion.timestamp);
  if (!Number.isFinite(start) || !Number.isFinite(end) || end < start) {
    return null;
  }
  return end - start;
}
