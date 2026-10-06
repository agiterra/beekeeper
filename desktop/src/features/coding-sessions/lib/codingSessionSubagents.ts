/**
 * Subagents inside a coding-session seat (ledger 308).
 *
 * A lead may spawn Claude Code subagents with the Task/Agent tool. The
 * provider publishes each subagent-attributed item as the ordinary kind it
 * would be, plus a top-level `parentToolId` naming the Task/Agent call that
 * owns it, and stamps what the adapter reported about the run on that call's
 * result as `subagent`. This module is the consumer's whole reading of that:
 * the wire fields, which items belong to which call, the stream's grouping
 * label, and the Agents panel's row model.
 *
 * Pure and defensive like the projector it extends. Nothing here invents a
 * value: a token count, a model or a duration the producer did not send is
 * absent, and the surfaces omit it rather than show a zero.
 */

import type {
  TranscriptItem,
  TranscriptSubagentReport,
} from "@/features/agents/ui/agentSessionTypes";
import {
  isRecord,
  MAX_METADATA_FIELD_LENGTH,
  safeString,
} from "./codingSessionDefensive";
import { codingSessionModelDisplayName } from "./codingSessionModelDisplay";
import type { CodingSessionTurnSettlement } from "./codingSessionTranscriptModelTypes";

type ToolTranscriptItem = Extract<TranscriptItem, { type: "tool" }>;

// ---------------------------------------------------------------------------
// Wire fields
// ---------------------------------------------------------------------------

function readBoundedId(value: unknown): string | undefined {
  return typeof value === "string" && value.trim().length > 0
    ? safeString(value, MAX_METADATA_FIELD_LENGTH)
    : undefined;
}

function readCount(value: unknown): number | undefined {
  return typeof value === "number" && Number.isFinite(value) && value >= 0
    ? value
    : undefined;
}

/**
 * The `subagent` report on a Task/Agent result, keeping only well-typed
 * fields. `null` when the object is absent or carried nothing usable.
 */
export function readCodingSessionSubagentReport(
  value: unknown,
): TranscriptSubagentReport | null {
  if (!isRecord(value)) return null;
  const report: TranscriptSubagentReport = {};
  const type = readBoundedId(value.type);
  const model = readBoundedId(value.model);
  const totalTokens = readCount(value.totalTokens);
  const durationMs = readCount(value.durationMs);
  const toolUseCount = readCount(value.toolUseCount);
  if (type !== undefined) report.type = type;
  if (model !== undefined) report.model = model;
  if (totalTokens !== undefined) report.totalTokens = totalTokens;
  if (durationMs !== undefined) report.durationMs = durationMs;
  if (toolUseCount !== undefined) report.toolUseCount = toolUseCount;
  return Object.keys(report).length > 0 ? report : null;
}

/**
 * Copy the subagent wire fields of a raw 44225 item (and, for a paired tool
 * card, its raw result) onto the projected item.
 *
 * Returns the item unchanged when there is nothing to add, so items published
 * before ledger 308 project byte-identically.
 */
export function stampCodingSessionSubagentFields<T extends TranscriptItem>(
  projected: T,
  raw: unknown,
  rawResult?: unknown,
): T {
  const call = isRecord(raw) ? raw : {};
  const result = isRecord(rawResult) ? rawResult : {};
  const parentToolId =
    readBoundedId(call.parentToolId) ?? readBoundedId(result.parentToolId);
  let stamped: T = projected;
  if (parentToolId !== undefined) stamped = { ...stamped, parentToolId };
  if (stamped.type !== "tool") return stamped;

  const tool = isRecord(call.tool) ? call.tool : {};
  const toolCallId =
    readBoundedId(tool.toolId) ??
    readBoundedId(call.toolId) ??
    readBoundedId(result.toolId);
  const subagent =
    readCodingSessionSubagentReport(result.subagent) ??
    readCodingSessionSubagentReport(call.subagent);
  if (toolCallId !== undefined) stamped = { ...stamped, toolCallId };
  if (subagent !== null) stamped = { ...stamped, subagent };
  return stamped;
}

// ---------------------------------------------------------------------------
// Ownership
// ---------------------------------------------------------------------------

/** Which items a Task/Agent call owns, by the call's `toolCallId`. */
export type CodingSessionSubagentPartition = {
  childrenByToolId: ReadonlyMap<string, TranscriptItem[]>;
  /** Items that belong under an owning call present in the same transcript. */
  nested: ReadonlySet<TranscriptItem>;
  /**
   * Item ids of open tool calls (no result) whose turn is already over: the
   * turn reached its result or was interrupted, or a later turn began. Such a
   * call will not get a result in this transcript.
   */
  stoppedCallIds: ReadonlySet<string>;
};

const EMPTY_PARTITION: CodingSessionSubagentPartition = {
  childrenByToolId: new Map(),
  nested: new Set(),
  stoppedCallIds: new Set(),
};

function isTurnTerminal(item: TranscriptItem): boolean {
  return (
    item.type === "lifecycle" &&
    (item.title === "Turn result" || item.title === "Interrupted")
  );
}

function isTurnOpeningPrompt(item: TranscriptItem): boolean {
  return item.type === "message" && item.role === "user" && !item.steered;
}

/**
 * Open calls whose turn is over, from transcript data alone.
 *
 * With turn identity: a later terminal item in the call's own turn, or a
 * prompt that opened a different turn **after the call's turn went quiet** —
 * after its last item (SV-44, the rule `deriveCodingSessionTranscriptModel`
 * supersedes turns by). A prompt with its own turn id that arrives while the
 * call's turn is still publishing does not end it. Without turn identity: any
 * later terminal item or opening prompt — the only boundary the transcript
 * still offers.
 */
function findStoppedCalls(
  transcript: readonly TranscriptItem[],
): ReadonlySet<string> {
  const lastIndexByTurn = new Map<string, number>();
  transcript.forEach((item, index) => {
    if (item.turnId) lastIndexByTurn.set(item.turnId, index);
  });
  const stopped = new Set<string>();
  const terminatedTurns = new Set<string>();
  // Later opening prompts, newest first: [index, turn key].
  const laterPrompts: Array<readonly [number, string]> = [];
  let laterTerminal = false;
  for (let index = transcript.length - 1; index >= 0; index -= 1) {
    const item = transcript[index];
    const turnId = item.turnId ?? null;
    if (
      item.type === "tool" &&
      (item.status === "executing" || item.status === "pending")
    ) {
      const over =
        turnId === null
          ? laterTerminal || laterPrompts.length > 0
          : terminatedTurns.has(turnId) ||
            laterPrompts.some(
              ([at, other]) =>
                other !== turnId && at > (lastIndexByTurn.get(turnId) ?? index),
            );
      if (over) stopped.add(item.id);
    }
    if (isTurnTerminal(item)) {
      laterTerminal = true;
      if (turnId !== null) terminatedTurns.add(turnId);
    }
    if (isTurnOpeningPrompt(item)) {
      // A prompt with no turn identity still proves a later turn began.
      laterPrompts.push([index, turnId ?? `prompt:${item.id}`]);
    }
  }
  return stopped;
}

/**
 * Split a transcript into the lead's items and the items each subagent owns.
 *
 * Only an item whose `parentToolId` names a call that is itself in this
 * transcript is nested. An item whose owner is missing (a window that starts
 * mid-run, a quarantined call) stays in the reading order: hiding it would
 * turn something the provider published into nothing.
 */
export function partitionCodingSessionSubagentItems(
  transcript: readonly TranscriptItem[],
): CodingSessionSubagentPartition {
  const stoppedCallIds = findStoppedCalls(transcript);
  if (!transcript.some((item) => item.parentToolId)) {
    return { ...EMPTY_PARTITION, stoppedCallIds };
  }
  const callIds = new Set<string>();
  for (const item of transcript) {
    if (item.type === "tool" && item.toolCallId) callIds.add(item.toolCallId);
  }
  const childrenByToolId = new Map<string, TranscriptItem[]>();
  const nested = new Set<TranscriptItem>();
  for (const item of transcript) {
    const parent = item.parentToolId;
    if (!parent || !callIds.has(parent)) continue;
    if (item.type === "tool" && item.toolCallId === parent) continue;
    nested.add(item);
    const children = childrenByToolId.get(parent);
    if (children) children.push(item);
    else childrenByToolId.set(parent, [item]);
  }
  return { childrenByToolId, nested, stoppedCallIds };
}

/** The adapter's tool names for a subagent spawn. */
const SUBAGENT_TOOL_NAMES = new Set(["Task", "Agent"]);

/**
 * Whether a tool card is a Task/Agent spawn.
 *
 * claude-agent-acp titles the call with its `description`, so the name alone
 * says nothing; the evidence is any one of: the call owns nested items, its
 * result carried a subagent report, its arguments name a `subagent_type`, or
 * the adapter fell back to the bare tool name for an undescribed spawn.
 */
export function isCodingSessionSubagentCall(
  item: TranscriptItem,
  partition: CodingSessionSubagentPartition = EMPTY_PARTITION,
): boolean {
  if (item.type !== "tool") return false;
  if (item.subagent) return true;
  if (item.toolCallId && partition.childrenByToolId.has(item.toolCallId)) {
    return true;
  }
  if (typeof item.args.subagent_type === "string") return true;
  return SUBAGENT_TOOL_NAMES.has(item.toolName);
}

// ---------------------------------------------------------------------------
// Stream
// ---------------------------------------------------------------------------

/** One Task/Agent call with the items its subagent published. */
export type CodingSessionSubagentSpawn = {
  call: ToolTranscriptItem;
  children: TranscriptItem[];
  status: CodingSessionSubagentStatus;
};

/**
 * `stopped` is a call that never got a result and never will: its turn ended
 * (or a later one began) without one. Settled, but not a success or a failure.
 */
export type CodingSessionSubagentStatus =
  | "running"
  | "stopped"
  | "done"
  | "failed";

/**
 * Status from data only: the result says done or failed; no result is running
 * until the transcript shows the call's turn is over, then stopped.
 */
export function codingSessionSubagentStatus(
  call: ToolTranscriptItem,
  partition: CodingSessionSubagentPartition = EMPTY_PARTITION,
): CodingSessionSubagentStatus {
  if (call.status === "executing" || call.status === "pending") {
    return partition.stoppedCallIds.has(call.id) ? "stopped" : "running";
  }
  return call.isError || call.status === "failed" ? "failed" : "done";
}

/** A spawn with its children and status, read against its transcript. */
export function buildCodingSessionSubagentSpawn(
  call: ToolTranscriptItem,
  partition: CodingSessionSubagentPartition,
): CodingSessionSubagentSpawn {
  const children = call.toolCallId
    ? partition.childrenByToolId.get(call.toolCallId)
    : undefined;
  return {
    call,
    children: children ?? [],
    status: codingSessionSubagentStatus(call, partition),
  };
}

/** What the person reads as the subagent's name: its Task description. */
export function codingSessionSubagentTitle(call: ToolTranscriptItem): string {
  const description = call.args.description;
  if (typeof description === "string" && description.trim().length > 0) {
    return safeString(description.trim(), MAX_METADATA_FIELD_LENGTH);
  }
  return call.title || call.toolName;
}

/**
 * The stream row's label, one sentence for any number of spawns, in T3 Code's
 * words (SV-06; `subagentGroupSummary` in
 * `packages/client-runtime/src/state/subagentDisplay.ts`): "Ran 1 subagent",
 * "Ran 4 subagents", and while any still runs "Kicked off 3 subagents".
 *
 * The sentence says how many started, not how many still work. That count,
 * and how the rest ended, is `summarizeCodingSessionSubagentStatuses`
 * ("1 working · 2 failed"), which `CodingSessionSubagentEntry` renders right
 * after this label whenever any spawn is not done (and always as the row's
 * accessible description), beside the group's status icon. The subagent's
 * own description is one click away, on the spawn's header inside the opened
 * row and in the Agents panel (`codingSessionSubagentTitle`).
 */
export function formatCodingSessionSubagentGroupLabel(
  spawns: ReadonlyArray<{ status: CodingSessionSettledSubagentStatus }>,
): string {
  const total = spawns.length;
  const live = spawns.some((spawn) => spawn.status === "running");
  return `${live ? "Kicked off" : "Ran"} ${total} ${total === 1 ? "subagent" : "subagents"}`;
}

/**
 * A spawn's status once its turn's settlement is known (SV-44 follow-up).
 *
 * The transcript alone cannot always tell that a spawn's turn is over: a turn
 * that a provider abandoned without a result, and that no later prompt
 * followed, keeps its open Task call "running" here, while the turn's own
 * tool rows read settled from the session's resting status
 * (`resolveCodingSessionTurnSettlement`). Reading the spawn through the same
 * settlement keeps the two agreeing: an open spawn in a `settled` turn is
 * `stopped`, in an `unknown` turn it is `unknown` — neither a spinner nor a
 * verdict — and in a `live` turn it is still running. A spawn that has its
 * result keeps it.
 */
export type CodingSessionSettledSubagentStatus =
  | CodingSessionSubagentStatus
  | "unknown";

export function settleCodingSessionSubagentStatus(
  status: CodingSessionSubagentStatus,
  settlement: CodingSessionTurnSettlement,
): CodingSessionSettledSubagentStatus {
  if (status !== "running") return status;
  if (settlement === "settled") return "stopped";
  if (settlement === "unknown") return "unknown";
  return status;
}

/** A turn's spawns read through its settlement; the same array when unchanged. */
export function settleCodingSessionSubagentSpawns(
  spawns: readonly CodingSessionSubagentSpawn[],
  settlement: CodingSessionTurnSettlement,
): ReadonlyArray<
  Omit<CodingSessionSubagentSpawn, "status"> & {
    status: CodingSessionSettledSubagentStatus;
  }
> {
  if (!spawns.some((spawn) => spawn.status === "running")) return spawns;
  if (settlement === "live") return spawns;
  return spawns.map((spawn) => ({
    ...spawn,
    status: settleCodingSessionSubagentStatus(spawn.status, settlement),
  }));
}

/** T3's status words, in the order a reader scans them: live work first. */
const SUBAGENT_STATUS_WORDS: ReadonlyArray<
  readonly [CodingSessionSettledSubagentStatus, string]
> = [
  ["running", "working"],
  ["done", "done"],
  ["failed", "failed"],
  ["stopped", "stopped"],
  ["unknown", "status unknown"],
];

/**
 * Counts a group's states, T3's `summarizeSubagentStatuses`: "2 working · 1
 * done", "3 done", "1 done · 1 failed". Every spawn is counted under exactly
 * one word, so the parts always add up to the label's number.
 */
export function summarizeCodingSessionSubagentStatuses(
  spawns: ReadonlyArray<{ status: CodingSessionSettledSubagentStatus }>,
): string {
  return SUBAGENT_STATUS_WORDS.flatMap(([status, word]) => {
    const count = spawns.filter((spawn) => spawn.status === status).length;
    return count > 0 ? [`${count} ${word}`] : [];
  }).join(" · ");
}

// ---------------------------------------------------------------------------
// Agents panel
// ---------------------------------------------------------------------------

export type CodingSessionSubagentRow = {
  /** The call's projected item id — the stream's anchor for this spawn. */
  id: string;
  title: string;
  /** `subagent_type` (`general-purpose`, `Explore`), when known. */
  type: string | null;
  /**
   * Read through the spawn's turn settlement (`settleCodingSessionSubagentStatus`)
   * when the caller knows it, so the panel says what the stream row says.
   * `spawn.status` keeps the transcript-only reading.
   */
  status: CodingSessionSettledSubagentStatus;
  durationMs: number | null;
  model: string | null;
  totalTokens: number | null;
  /** Tools the subagent used, when anything says how many. */
  toolCount: number | null;
  /** The latest thing it said or ran, first line only. */
  latest: string | null;
  startedAt: string;
  spawn: CodingSessionSubagentSpawn;
};

export type CodingSessionSubagentPanel = {
  rows: CodingSessionSubagentRow[];
  /** Done, failed and stopped alike: everything known to be no longer running. */
  settled: number;
  running: number;
  /** Open spawns whose turn nothing on screen can vouch for: neither of the above. */
  unknown: number;
  /** Σ over the rows that reported tokens; `null` when none did. */
  totalTokens: number | null;
};

/** The first non-blank line of `text`, bounded; `null` when there is none. */
export function codingSessionSubagentFirstLine(text: string): string | null {
  const line = text.split("\n").find((candidate) => candidate.trim());
  return line ? safeString(line.trim(), MAX_METADATA_FIELD_LENGTH) : null;
}

const firstLine = codingSessionSubagentFirstLine;

/**
 * The latest thing a subagent said or ran, first line only. The call's result
 * stands in only once the spawn has one: a spawn that is running, stopped or
 * of unknown status has no result to quote.
 */
export function codingSessionSubagentLatestActivity(spawn: {
  call: ToolTranscriptItem;
  children: readonly TranscriptItem[];
  status: CodingSessionSettledSubagentStatus;
}): string | null {
  return latestActivity(spawn);
}

function latestActivity(spawn: {
  call: ToolTranscriptItem;
  children: readonly TranscriptItem[];
  status: CodingSessionSettledSubagentStatus;
}): string | null {
  for (let index = spawn.children.length - 1; index >= 0; index -= 1) {
    const child = spawn.children[index];
    if (child.type === "message" && child.role === "assistant") {
      const line = firstLine(child.text);
      if (line) return line;
    }
    if (child.type === "tool") return `▸ ${child.title || child.toolName}`;
  }
  return spawn.status === "done" || spawn.status === "failed"
    ? firstLine(spawn.call.result)
    : null;
}

/** Start to result, from the call's own timestamps; `null` until it has one. */
export function codingSessionSubagentElapsedMs(
  call: ToolTranscriptItem,
): number | null {
  return elapsedMs(call);
}

function elapsedMs(call: ToolTranscriptItem): number | null {
  if (!call.completedAt) return null;
  const start = Date.parse(call.startedAt);
  const end = Date.parse(call.completedAt);
  return Number.isFinite(start) && Number.isFinite(end) && end >= start
    ? end - start
    : null;
}

/** One panel row, from the call and its nested items only. */
export function deriveCodingSessionSubagentRow(
  spawn: CodingSessionSubagentSpawn,
  settlement: CodingSessionTurnSettlement = "live",
): CodingSessionSubagentRow {
  const { call } = spawn;
  const report = call.subagent;
  const argType = call.args.subagent_type;
  const nestedTools = spawn.children.filter(
    (child) => child.type === "tool",
  ).length;
  return {
    id: call.id,
    title: codingSessionSubagentTitle(call),
    type:
      report?.type ??
      (typeof argType === "string" && argType.trim()
        ? safeString(argType.trim(), MAX_METADATA_FIELD_LENGTH)
        : null),
    status: settleCodingSessionSubagentStatus(spawn.status, settlement),
    durationMs: report?.durationMs ?? elapsedMs(call),
    model: report?.model ?? null,
    totalTokens: report?.totalTokens ?? null,
    // Counted from what was published; the adapter's own count stands in only
    // when no nested tool reached this client, and an unknown stays unknown.
    toolCount: nestedTools > 0 ? nestedTools : (report?.toolUseCount ?? null),
    latest: latestActivity(spawn),
    startedAt: call.startedAt,
    spawn,
  };
}

/**
 * The turn settlement of one spawn's call, for the panel: `transcriptIndex`
 * names which of the panel's transcripts the call came from.
 */
export type CodingSessionSubagentSettlementOf = (
  call: ToolTranscriptItem,
  transcriptIndex: number,
) => CodingSessionTurnSettlement;

/**
 * Every Task/Agent call across the given transcripts, newest last, with the
 * footer tally. Several transcripts because the umbrella lists every
 * execution's spawns in one panel.
 *
 * `settlementOf` is the turn settlement the stream reads the same spawn
 * through (SV-44 follow-up). Omitted, every open spawn reads `running` —
 * the transcript-only reading — so a caller that knows the session's
 * resting status must pass it, or the panel shows a spinner the stream row
 * beside it has already settled.
 */
export function deriveCodingSessionSubagentPanel(
  transcripts: readonly (readonly TranscriptItem[])[],
  settlementOf?: CodingSessionSubagentSettlementOf,
): CodingSessionSubagentPanel {
  const rows: CodingSessionSubagentRow[] = [];
  for (const [transcriptIndex, transcript] of transcripts.entries()) {
    const partition = partitionCodingSessionSubagentItems(transcript);
    for (const item of transcript) {
      if (
        item.type !== "tool" ||
        !isCodingSessionSubagentCall(item, partition)
      ) {
        continue;
      }
      const spawn = buildCodingSessionSubagentSpawn(item, partition);
      rows.push(
        deriveCodingSessionSubagentRow(
          spawn,
          spawn.status === "running" && settlementOf
            ? settlementOf(item, transcriptIndex)
            : "live",
        ),
      );
    }
  }
  rows.sort(
    (left, right) =>
      (Date.parse(left.startedAt) || 0) - (Date.parse(right.startedAt) || 0),
  );
  return tallyCodingSessionSubagentRows(rows);
}

/**
 * A panel derived without settlement, read through each open spawn's turn
 * settlement afterwards — for a caller that derives the panel before it
 * knows the session's resting status. The same panel when nothing changes.
 */
export function settleCodingSessionSubagentPanel(
  panel: CodingSessionSubagentPanel,
  settlementOf: (call: ToolTranscriptItem) => CodingSessionTurnSettlement,
): CodingSessionSubagentPanel {
  let changed = false;
  const rows = panel.rows.map((row) => {
    if (row.spawn.status !== "running") return row;
    const status = settleCodingSessionSubagentStatus(
      row.spawn.status,
      settlementOf(row.spawn.call),
    );
    if (status === row.status) return row;
    changed = true;
    return { ...row, status };
  });
  return changed ? tallyCodingSessionSubagentRows(rows) : panel;
}

function tallyCodingSessionSubagentRows(
  rows: CodingSessionSubagentRow[],
): CodingSessionSubagentPanel {
  let totalTokens: number | null = null;
  let running = 0;
  let unknown = 0;
  for (const row of rows) {
    if (row.status === "running") running += 1;
    if (row.status === "unknown") unknown += 1;
    if (row.totalTokens !== null) {
      totalTokens = (totalTokens ?? 0) + row.totalTokens;
    }
  }
  return {
    rows,
    settled: rows.length - running - unknown,
    running,
    unknown,
    totalTokens,
  };
}

/**
 * The footer line: settled, Σ tokens when any were reported, running, and
 * the spawns whose status nobody can vouch for — never folded into settled.
 */
export function formatCodingSessionSubagentFooter(
  panel: CodingSessionSubagentPanel,
): string {
  return [
    `${panel.settled} settled`,
    panel.totalTokens !== null
      ? `Σ ${formatCodingSessionSubagentTokens(panel.totalTokens)} tok`
      : null,
    panel.running > 0 ? `${panel.running} running` : null,
    panel.unknown > 0 ? `${panel.unknown} status unknown` : null,
  ]
    .filter(Boolean)
    .join(" · ");
}

/**
 * The one name a subagent's model goes by on every subagent surface (card,
 * hover card, page bar, Agents panel): the session's derived display name
 * (`claude-opus-5-5` → `Claude Opus 5.5`), so the same model never reads as a
 * slug in one place and a name in another. `null` stays `null`: a model the
 * producer did not report is never given a name. Surfaces keep the raw id on
 * a `title` beside it.
 */
export function codingSessionSubagentModelName(
  model: string | null,
): string | null {
  return model === null ? null : codingSessionModelDisplayName(model);
}

/**
 * SV-97: a subagent's children without the prose that only repeats its
 * report. The provider publishes the subagent's last answer twice — as an
 * `assistant_text` item attributed to the call, then as the call's result —
 * and a surface that shows the report would otherwise print it twice. Only
 * the *final* assistant message is dropped, and only when its trimmed text is
 * the report's; an empty report (none shown) drops nothing.
 */
export function withoutCodingSessionSubagentEchoedReport<
  T extends TranscriptItem,
>(children: readonly T[], report: string): readonly T[] {
  const shown = report.trim();
  if (!shown) return children;
  for (let index = children.length - 1; index >= 0; index -= 1) {
    const child = children[index];
    if (child.type !== "message" || child.role !== "assistant") continue;
    if (child.text.trim() !== shown) return children;
    return [...children.slice(0, index), ...children.slice(index + 1)];
  }
  return children;
}

/** `<model> · <tokens> tok · <N> tools`, each part only when known. */
export function formatCodingSessionSubagentMeta(
  row: CodingSessionSubagentRow,
): string {
  return [
    codingSessionSubagentModelName(row.model),
    row.totalTokens !== null
      ? `${formatCodingSessionSubagentTokens(row.totalTokens)} tok`
      : null,
    row.toolCount !== null
      ? `${row.toolCount} ${row.toolCount === 1 ? "tool" : "tools"}`
      : null,
  ]
    .filter(Boolean)
    .join(" · ");
}

/** `812`, `48.2k`, `1.3M` — the panel's compact token figure. */
export function formatCodingSessionSubagentTokens(tokens: number): string {
  if (tokens < 1_000) return String(Math.round(tokens));
  if (tokens < 1_000_000) return `${trimDecimal(tokens / 1_000)}k`;
  return `${trimDecimal(tokens / 1_000_000)}M`;
}

function trimDecimal(value: number): string {
  return value >= 100 ? String(Math.round(value)) : value.toFixed(1);
}
