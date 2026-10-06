/**
 * One subagent as a card (SV-80/SV-81): the row a person clicks to open the
 * subagent's own page, the finish card the parent timeline draws once it is
 * settled, and the hover card's facts. T3 Code's `SubagentTimelineLink`
 * (`apps/web/src/components/chat/V2LifecycleRow.tsx:395-525`) is the shape.
 *
 * Read from a spawn already settled through its turn
 * (`settleCodingSessionSubagentStatus`), so a card never ticks for a spawn
 * the stream calls stopped. Like the rest of the subagent reading, nothing is
 * invented: a model, a token count or a duration the producer did not send is
 * `null`, and the surfaces say "not reported" or omit it rather than show a
 * zero.
 */

import type { TranscriptItem } from "@/features/agents/ui/agentSessionTypes";
import {
  safeString,
  MAX_METADATA_FIELD_LENGTH,
} from "./codingSessionDefensive";
import {
  type CodingSessionSettledSubagentStatus,
  codingSessionSubagentElapsedMs,
  codingSessionSubagentFirstLine,
  codingSessionSubagentLatestActivity,
  codingSessionSubagentTitle,
} from "./codingSessionSubagents";

type ToolTranscriptItem = Extract<TranscriptItem, { type: "tool" }>;

/** A spawn read through its turn's settlement. */
export type CodingSessionSettledSubagentSpawnLike = {
  call: ToolTranscriptItem;
  children: readonly TranscriptItem[];
  status: CodingSessionSettledSubagentStatus;
};

/**
 * `live` ticks; `finished` is frozen at its outcome (done, failed or
 * stopped); `unknown` is neither — no timer and no verdict.
 */
export type CodingSessionSubagentCardPhase = "live" | "finished" | "unknown";

export type CodingSessionSubagentCard = {
  /** The call's projected item id — the stream's anchor for this spawn. */
  callItemId: string;
  /**
   * The producer's id for the call, which every item of the subagent names as
   * its `parentToolId`: the subagent page's key. `null` when the producer
   * sent none — then no page can be opened, and the card says so.
   */
  parentToolId: string | null;
  title: string;
  type: string | null;
  status: CodingSessionSettledSubagentStatus;
  phase: CodingSessionSubagentCardPhase;
  /** "Running", "Finished", "Failed", "Stopped", "Status unknown". */
  outcomeLabel: string;
  model: string | null;
  totalTokens: number | null;
  toolCount: number | null;
  /** Frozen duration of a finished run; `null` while it runs or when unknown. */
  durationMs: number | null;
  /** Epoch ms the call started, for the live timer; `null` when unparseable. */
  startedAtMs: number | null;
  /** First line of the result once it has one, else the latest activity. */
  detail: string | null;
  /** How many of the subagent's own items reached this client. */
  childCount: number;
};

const OUTCOME_LABELS: Record<CodingSessionSettledSubagentStatus, string> = {
  running: "Running",
  done: "Finished",
  failed: "Failed",
  stopped: "Stopped",
  unknown: "Status unknown",
};

export function codingSessionSubagentCardPhase(
  status: CodingSessionSettledSubagentStatus,
): CodingSessionSubagentCardPhase {
  if (status === "running") return "live";
  if (status === "unknown") return "unknown";
  return "finished";
}

/** One card from a settled spawn. */
export function deriveCodingSessionSubagentCard(
  spawn: CodingSessionSettledSubagentSpawnLike,
): CodingSessionSubagentCard {
  const { call, status } = spawn;
  const report = call.subagent;
  const argType = call.args.subagent_type;
  const nestedTools = spawn.children.filter(
    (child) => child.type === "tool",
  ).length;
  const startedAtMs = Date.parse(call.startedAt);
  const settledWithResult = status === "done" || status === "failed";
  return {
    callItemId: call.id,
    parentToolId: call.toolCallId ?? null,
    title: codingSessionSubagentTitle(call),
    type:
      report?.type ??
      (typeof argType === "string" && argType.trim()
        ? safeString(argType.trim(), MAX_METADATA_FIELD_LENGTH)
        : null),
    status,
    phase: codingSessionSubagentCardPhase(status),
    outcomeLabel: OUTCOME_LABELS[status],
    model: report?.model ?? null,
    totalTokens: report?.totalTokens ?? null,
    toolCount: nestedTools > 0 ? nestedTools : (report?.toolUseCount ?? null),
    // Only a run that returned has a duration; a stopped one never finished.
    durationMs: settledWithResult
      ? (report?.durationMs ?? codingSessionSubagentElapsedMs(call))
      : null,
    startedAtMs: Number.isFinite(startedAtMs) ? startedAtMs : null,
    detail:
      (settledWithResult
        ? codingSessionSubagentFirstLine(call.result ?? "")
        : null) ?? codingSessionSubagentLatestActivity(spawn),
    childCount: spawn.children.length,
  };
}

/** Live elapsed from the start to `nowMs`; `null` without a start. */
export function codingSessionSubagentLiveElapsedMs(
  startedAtMs: number | null,
  nowMs: number,
): number | null {
  return startedAtMs === null ? null : Math.max(0, nowMs - startedAtMs);
}
