/**
 * The subagent page's reading (SV-79, SV-82): which items are the subagent's,
 * what its bar says, and the prompt it was given.
 *
 * Pure. Status always comes from the panel row, which is already read through
 * the spawn's turn settlement (`settleCodingSessionSubagentStatus`), so the
 * page never shows a subagent running that the stream and the Agents surface
 * call stopped. Nothing is invented: a model, token count, tool count or
 * duration the producer did not send is `null`, and the bar omits it.
 */

import type { TranscriptItem } from "@/features/agents/ui/agentSessionTypes";
import {
  type CodingSessionSettledSubagentStatus,
  type CodingSessionSubagentPanel,
  type CodingSessionSubagentRow,
  formatCodingSessionSubagentTokens,
} from "./codingSessionSubagents";
import type { CodingSessionTurnRestingStatus } from "./codingSessionTranscriptModelTypes";

type ToolTranscriptItem = Extract<TranscriptItem, { type: "tool" }>;

// ---------------------------------------------------------------------------
// Items
// ---------------------------------------------------------------------------

/**
 * The subagent's own items, in transcript order: every item whose
 * `parentToolId` is this call's `toolCallId`, except the call itself.
 */
export function selectCodingSessionSubagentItems(
  transcript: readonly TranscriptItem[],
  parentToolId: string,
): TranscriptItem[] {
  return transcript.filter(
    (item) =>
      item.parentToolId === parentToolId &&
      !(item.type === "tool" && item.toolCallId === parentToolId),
  );
}

// ---------------------------------------------------------------------------
// Prompt (SV-82)
// ---------------------------------------------------------------------------

/**
 * What the transcript kept of the Task call's input. `given` carries the
 * prompt as published; `truncated` is an input the provider bounded on the
 * way out (`bounded_input` in buzz-session-provider), so only a preview of
 * its serialized form survives; `absent` is a call whose input carries no
 * prompt at all.
 */
export type CodingSessionSubagentPrompt =
  | { kind: "given"; text: string; description: string | null }
  | {
      kind: "truncated";
      preview: string | null;
      byteCount: number | null;
      description: string | null;
    }
  | { kind: "absent"; description: string | null };

function nonEmptyString(value: unknown): string | null {
  return typeof value === "string" && value.trim().length > 0
    ? value.trim()
    : null;
}

export function readCodingSessionSubagentPrompt(
  call: ToolTranscriptItem,
): CodingSessionSubagentPrompt {
  const args = call.args ?? {};
  const description = nonEmptyString(args.description);
  if (args.truncated === true) {
    const byteCount =
      typeof args.byteCount === "number" && Number.isFinite(args.byteCount)
        ? args.byteCount
        : null;
    return {
      kind: "truncated",
      preview: nonEmptyString(args.preview),
      byteCount,
      description,
    };
  }
  const text = nonEmptyString(args.prompt);
  if (text !== null) return { kind: "given", text, description };
  return { kind: "absent", description };
}

// ---------------------------------------------------------------------------
// Page
// ---------------------------------------------------------------------------

export type CodingSessionSubagentPage =
  | {
      kind: "spawn";
      parentToolId: string;
      row: CodingSessionSubagentRow;
      items: TranscriptItem[];
      prompt: CodingSessionSubagentPrompt;
      /**
       * The call carries no `toolCallId`, so nothing can name it as a parent:
       * its subagent's steps were never attributed on the wire, which is not
       * the same as there being none.
       */
      unattributed: boolean;
    }
  | {
      /** No call in this view owns `parentToolId`: say so, show what remains. */
      kind: "missing-call";
      parentToolId: string;
      items: TranscriptItem[];
    };

/**
 * The page for `parentToolId`: the panel row whose call has that
 * `toolCallId` (or, for a call that never got one, that item id), and the
 * subagent's items from `transcript`.
 */
export function resolveCodingSessionSubagentPage(input: {
  parentToolId: string;
  panel: CodingSessionSubagentPanel;
  transcript: readonly TranscriptItem[];
}): CodingSessionSubagentPage {
  const { parentToolId, panel, transcript } = input;
  const row =
    panel.rows.find((r) => r.spawn.call.toolCallId === parentToolId) ??
    panel.rows.find((r) => r.id === parentToolId) ??
    null;
  if (row === null) {
    return {
      kind: "missing-call",
      parentToolId,
      items: selectCodingSessionSubagentItems(transcript, parentToolId),
    };
  }
  const toolCallId = row.spawn.call.toolCallId;
  return {
    kind: "spawn",
    parentToolId,
    row,
    items: toolCallId
      ? selectCodingSessionSubagentItems(transcript, toolCallId)
      : [],
    prompt: readCodingSessionSubagentPrompt(row.spawn.call),
    unattributed: !toolCallId,
  };
}

// ---------------------------------------------------------------------------
// Bar
// ---------------------------------------------------------------------------

export type CodingSessionSubagentBarModel = {
  title: string;
  type: string | null;
  model: string | null;
  status: CodingSessionSettledSubagentStatus;
  statusLabel: string;
  /** Ticks against the clock: only a subagent still running. */
  live: boolean;
  /** When the call started, ms; `null` when unreadable. */
  startedAtMs: number | null;
  /** A settled run's reported or observed duration; `null` when unknown. */
  durationMs: number | null;
  tokens: string | null;
  tools: string | null;
};

const STATUS_LABELS: Record<CodingSessionSettledSubagentStatus, string> = {
  running: "Running",
  done: "Completed",
  failed: "Failed",
  stopped: "Stopped",
  unknown: "Status unknown",
};

export function deriveCodingSessionSubagentBar(
  row: CodingSessionSubagentRow,
): CodingSessionSubagentBarModel {
  const started = Date.parse(row.startedAt);
  const startedAtMs = Number.isFinite(started) ? started : null;
  const live = row.status === "running" && startedAtMs !== null;
  return {
    title: row.title,
    type: row.type,
    model: row.model,
    status: row.status,
    statusLabel: STATUS_LABELS[row.status],
    live,
    startedAtMs,
    // A running call's report (if any) is not its final duration, and an
    // unknown one has no end to measure to.
    durationMs:
      row.status === "running" || row.status === "unknown"
        ? null
        : row.durationMs,
    tokens:
      row.totalTokens !== null
        ? `${formatCodingSessionSubagentTokens(row.totalTokens)} tok`
        : null,
    tools:
      row.toolCount !== null
        ? `${row.toolCount} ${row.toolCount === 1 ? "tool" : "tools"}`
        : null,
  };
}

/** The bar's elapsed figure at `nowMs`: live while running, else settled. */
export function codingSessionSubagentBarElapsedMs(
  bar: CodingSessionSubagentBarModel,
  nowMs: number,
): number | null {
  if (bar.live && bar.startedAtMs !== null) {
    return Math.max(0, nowMs - bar.startedAtMs);
  }
  return bar.durationMs;
}

/**
 * How the page's transcript reads a step that never ended: live while the
 * subagent runs, "did not finish" once it settled, and no verdict when its
 * status is unknown or its call is missing.
 */
export function codingSessionSubagentPageRestingStatus(
  status: CodingSessionSettledSubagentStatus | null,
): CodingSessionTurnRestingStatus {
  if (status === "running") return "running";
  if (status === "done" || status === "failed" || status === "stopped") {
    return "stopped";
  }
  return "unknown";
}
