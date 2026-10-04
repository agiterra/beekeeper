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
 * How a settled turn folds behind one "Worked for …" row (SV-02, decision D2).
 *
 * Nothing folds while the turn is live — that is when the work is being
 * watched. Once it settles, the work that produced the answer folds, as T3
 * Code's `deriveTurnFolds` does: tool calls, thoughts, plan snapshots and
 * intermediate prose — and, before the answer, failed and unfinished tool
 * calls too. A step that failed on the way to an answer is a step, not the
 * turn's outcome; the fold row names it ("· 1 step failed") so the folded
 * failure is never silent, and opening the fold shows it in place.
 *
 * The reader always keeps:
 *
 * - every prompt, including a steered one (attribution);
 * - the final assistant message (the answer);
 * - a failed or unfinished tool call *after* the answer, or in a turn that
 *   never answered — a failure the agent did not recover from;
 * - lifecycle errors, permission rows and every other non-error lifecycle
 *   row, which are continuity rather than work;
 * - subagent batches, which name who did the work.
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
    if (isAssistantAnswerEntry(entry) && !isResultBodyEntry(entry)) {
      terminalAnswerIndex = index;
    }
  });

  const hiddenIndexes: number[] = [];
  const hiddenTools: CodingSessionTranscriptToolItem[] = [];
  let hidesWork = false;
  let failedCount = 0;
  let unfinishedCount = 0;
  turn.entries.forEach((entry, index) => {
    if (index === terminalAnswerIndex || isResultBodyEntry(entry)) return;
    const beforeAnswer = index < terminalAnswerIndex;
    const fold = classifyFoldable(entry, beforeAnswer);
    if (fold === "keep") return;
    hiddenIndexes.push(index);
    if (fold !== "thought") hidesWork = true;
    if (entry.kind === "tool-group") hiddenTools.push(...entry.items);
    else if (entry.kind === "item" && entry.item.type === "tool") {
      hiddenTools.push(entry.item);
      if (isFailedTool(entry.item)) failedCount += 1;
      else if (!isSettledTool(entry.item)) unfinishedCount += 1;
    }
  });

  const anchorIndex = hiddenIndexes[0];
  if (!hidesWork || anchorIndex === undefined) return null;
  const workSummary = summarizeCodingSessionTools(hiddenTools);
  const failureSummary = formatFoldedFailures(failedCount, unfinishedCount);
  return {
    anchorIndex,
    hiddenIndexes,
    durationMs: turnDurationMs(turn),
    summary: [workSummary, failureSummary].filter(Boolean).join(" · "),
    workSummary,
    failureSummary,
    failedCount,
    unfinishedCount,
  };
}

/**
 * The muted clause naming what went wrong inside the fold: "1 step failed",
 * "2 steps failed and 1 did not finish". Empty when nothing did.
 */
export function formatFoldedFailures(
  failedCount: number,
  unfinishedCount: number,
): string {
  const steps = (count: number) => `${count} ${count === 1 ? "step" : "steps"}`;
  const failed = failedCount > 0 ? `${steps(failedCount)} failed` : "";
  if (unfinishedCount === 0) return failed;
  return failed
    ? `${failed} and ${unfinishedCount} did not finish`
    : `${steps(unfinishedCount)} did not finish`;
}

function classifyFoldable(
  entry: CodingSessionTranscriptEntry,
  beforeAnswer: boolean,
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
      // Before the answer every call folds, failed or unfinished included —
      // the fold row names those. After it (or in a turn that never
      // answered) only a call that settled successfully does.
      if (beforeAnswer) return "work";
      return isCompletedSuccessfulTool(item) && !isErrorItem(item)
        ? "work"
        : "keep";
    default:
      return "keep";
  }
}

function isFailedTool(item: CodingSessionTranscriptToolItem): boolean {
  return isErrorItem(item);
}

function isSettledTool(item: CodingSessionTranscriptToolItem): boolean {
  return item.status === "completed" || item.status === "failed";
}

function isResultBodyEntry(entry: CodingSessionTranscriptEntry) {
  return entry.kind === "item" && entry.item.id.endsWith(":assistant-result");
}

/**
 * An assistant message that says something. A blank or whitespace-only
 * message is not an answer — it matches `findCodingSessionAnswerIndex`, so a
 * turn that ended on an empty message keeps its failures on screen.
 */
function isAssistantAnswerEntry(entry: CodingSessionTranscriptEntry) {
  return (
    entry.kind === "item" &&
    entry.item.type === "message" &&
    entry.item.role === "assistant" &&
    entry.item.text.trim() !== ""
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
