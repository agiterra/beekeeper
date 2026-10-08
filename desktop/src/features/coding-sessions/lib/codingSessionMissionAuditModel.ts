/**
 * The Audit tab's model: what a session cost, read off its own signed items.
 *
 * The 2026-09-01 TeamRolesV1 run was measured by hand — eight turns pulled out
 * of the 44225 stream with a shell pipeline — because nothing in the product
 * renders per-turn usage. The numbers that came back were the interesting part
 * of the night: Bob's 88-call turn re-read about 136k tokens per call, and two
 * `bee sessions operation get --id …` calls downloaded every operation in the
 * session to show one. Brian: "we need to capture this so it is observable."
 *
 * Four rules this module exists to keep:
 *
 * - **Absent is not zero.** Every number is `number | null`; a driver that
 *   reported nothing gets `null`, and the table renders `—`. A `0` here is a
 *   measured zero.
 * - **No new wire.** Everything comes from items this client already holds:
 *   `result` items for duration, cost and per-turn usage; `tool` items for
 *   names, arguments and result sizes.
 *   {@link CODING_SESSION_AUDIT_USAGE_LIMIT} says what an empty column means.
 * - **Bounded.** Turns and every list are capped, and each cap that bites
 *   emits a visible notice rather than silently shortening the truth.
 * - **Names, not judgements.** "Handed twice" counts repeats; it does not
 *   decide whether a repeat was wasteful. The reader does that.
 */

import type { TranscriptItem } from "@/features/agents/ui/agentSessionTypes";
import { tokenizeShellCommand } from "@/features/agents/ui/agentSessionToolClassifier";

/**
 * Why the token columns can read `—` for every turn on a live session.
 *
 * Per-turn `usage` is on the wire (`crates/beekeeper-core/src/
 * coding_session_payload.rs` `TurnUsageReport`, typed at
 * `codingSessionTranscriptItemContract.ts:120-140`) and, since batch 2, the
 * desktop transcript projection carries it through
 * (`codingSessionResultUsage.ts` `buildResultUsage`). So an empty column
 * is now a statement about the *driver*, not about this client: the turns
 * closed without any driver publishing a count.
 *
 * This module reads `usage` defensively and never substitutes an estimate,
 * so `—` keeps meaning "nobody reported this", never "we lost it".
 */
export const CODING_SESSION_AUDIT_USAGE_LIMIT =
  "No turn in this session published a token count; these drivers report usage only on some turns, and this client never estimates one.";

/** Bounds. Raising one is a product decision, never a way past a full list. */
export const CODING_SESSION_AUDIT_LIMITS = {
  turns: 200,
  rows: 50,
} as const;

/** One seat's projected transcript, named as the roster names it. */
export type CodingSessionMissionAuditSeatInput = {
  executionKey: string;
  /** `Keystone · Lead` — the surface's own resolved label, never a pubkey. */
  seat: string;
  transcript: readonly TranscriptItem[];
  /**
   * True when this transcript is known to be a prefix of the seat's real one —
   * the Desktop analogue of the CLI's `MAX_AUDIT_ITEMS_PER_EXECUTION` clip. It
   * makes a counted `toolCalls` a floor rather than the turn's number. No
   * caller sets it today: the tab folds exactly the projection it was handed,
   * and claiming truncation nobody observed would be its own lie.
   */
  transcriptTruncated?: boolean;
};

/** One turn's accounting. Field names are frozen with `bee sessions audit`. */
export type CodingSessionMissionAuditTurn = {
  seat: string;
  executionKey: string;
  /** The provider's turn id, or null for items it published without one. */
  turnId: string | null;
  /** Epoch ms of this turn's earliest item, or null when none parsed. */
  startedAt: number | null;
  durationMs: number | null;
  /**
   * The driver's own `usage.toolCalls` when the turn's `result` carried one,
   * otherwise the count of `tool_call` items this turn published. Frozen with
   * `bee sessions audit` (`crates/beekeeper-cli/src/commands/sessions/audit.rs`
   * `turn_row`): both are measurements of the same turn, neither is a guess,
   * so this one is never absent.
   */
  toolCalls: number;
  /** False when `toolCalls` is the published-item count rather than the driver's. */
  toolCallsReported: boolean;
  /**
   * True when this row's count was taken from a stream that was cut short: the
   * number is a floor, not the turn's. Frozen with the CLI, where it is
   * `clipped && the driver reported no count of its own` — truncation removes
   * exactly the terminal `result` item that would have carried the driver's.
   */
  toolCallsTruncated: boolean;
  inputTokens: number | null;
  outputTokens: number | null;
  cacheReadTokens: number | null;
  cacheWriteTokens: number | null;
  contextWindow: number | null;
  costUsd: number | null;
};

/** A path or command one seat handed itself more than once. */
export type CodingSessionMissionAuditHandedTwice = {
  seat: string;
  what: "path" | "command";
  key: string;
  count: number;
  /**
   * How many of those `count` calls actually published a result. `bytes`
   * covers exactly these, so a reader can tell a small total from a partly
   * unanswered one.
   */
  resultsSeen: number;
  /**
   * UTF-8 bytes of the results those calls **published**; `null` when none of
   * them came back. Null rather than `0`: an unanswered call has no byte
   * count, and `0` would claim it returned nothing.
   */
  bytes: number | null;
  /**
   * True when at least one of those results carried the provider's elision
   * marker, so `bytes` is a floor rather than the size of what the tool
   * produced. The provider clips a tool result at 8 KiB
   * (`beekeeper_session_provider::transcript::bound_text`); saying so out loud is
   * cheaper than letting a reader total up clipped numbers.
   */
  bytesClipped: boolean;
};

/** An unbounded relay read: `bee sessions status|inbox|send|operation …`. */
export type CodingSessionMissionAuditRoomDownload = {
  seat: string;
  command: string;
  count: number;
};

/**
 * The **longest** run of consecutive identical commands that returned
 * identical results, per seat and command — not the total across runs. A
 * command a seat came back to twice, hours apart, is work; a command it ran
 * five times in a row for the same answer is a loop.
 */
export type CodingSessionMissionAuditRetryLoop = {
  seat: string;
  command: string;
  count: number;
  /**
   * `true` only when the run published a result to compare. A run whose
   * results never reached this client is consecutive repeats of one command
   * and nothing more — and "unknown" is not "they agreed", so it is `null`.
   */
  identicalResults: boolean | null;
};

/** Σ over a seat or the session. Absent stays absent; nothing sums to zero. */
export type CodingSessionMissionAuditTotals = {
  turns: number;
  /**
   * How many of those turns reported any usage of their own. A Σ over three
   * turns where one reported is a real number about one third of the work, and
   * the row has to say so — a seat-granular disclosure hid exactly that
   * (REVIEW-A3 F2).
   */
  reportedTurns: number;
  /**
   * How many of those turns carry a cost of their own. Counted apart from
   * `reportedTurns`: a turn can report tokens and no price, and a cost total
   * over fewer priced turns than it covers must say so (ledger 275 A5).
   */
  pricedTurns: number;
  durationMs: number | null;
  toolCalls: number;
  /** False when any turn in the total fell back to the published-item count. */
  toolCallsReported: boolean;
  /**
   * True when any turn in the total contributed a floor, so the total is one
   * too. The flag travels with the number rather than sitting in a sibling
   * table the reader may not have open.
   */
  toolCallsTruncated: boolean;
  inputTokens: number | null;
  outputTokens: number | null;
  cacheReadTokens: number | null;
  cacheWriteTokens: number | null;
  costUsd: number | null;
};

/** One bound that bit, in the reader's words. */
export type CodingSessionMissionAuditTruncation = {
  id: string;
  section: "turns" | "handed-twice" | "room-downloads" | "retry-loops";
  notice: string;
};

export type CodingSessionMissionAudit = {
  turns: readonly CodingSessionMissionAuditTurn[];
  totalsBySeat: readonly {
    seat: string;
    executionKey: string;
    totals: CodingSessionMissionAuditTotals;
  }[];
  sessionTotals: CodingSessionMissionAuditTotals;
  /** Seats in the session, and how many of them reported any token usage. */
  seatCount: number;
  reportedSeatCount: number;
  handedTwice: readonly CodingSessionMissionAuditHandedTwice[];
  roomDownloads: readonly CodingSessionMissionAuditRoomDownload[];
  retryLoops: readonly CodingSessionMissionAuditRetryLoop[];
  truncations: readonly CodingSessionMissionAuditTruncation[];
};

/** The title `buildResultLifecycleItem` gives a projected turn terminator. */
const TURN_RESULT_TITLE = "Turn result";
/** The title `buildContextWindowStatusItem` gives an occupancy item. */
const CONTEXT_WINDOW_TITLE = "Context Window Updated";

/**
 * `bee sessions <verb>` calls that read the whole room to show one row.
 *
 * Frozen with the CLI's own `ROOM_DOWNLOAD_VERBS` so the ledger, `bee sessions
 * audit` and this tab name the same four reads.
 */
const ROOM_DOWNLOAD_VERBS = new Set(["status", "inbox", "send", "operation"]);

/** A key one seat asked for this many times or more is "handed twice". */
const HANDED_TWICE_MIN = 2;

/** A run shorter than this is repetition, not a loop. Frozen with the CLI. */
const RETRY_LOOP_MIN = 3;

/**
 * The marker `beekeeper_session_provider::transcript::bound_text` leaves behind
 * when it clips a tool result at its 8 KiB ceiling.
 */
const ELISION_MARKER = "…[elided ";

/** Fold every seat's signed transcript into the Audit tab's rows. */
export function deriveCodingSessionMissionAudit(
  seats: readonly CodingSessionMissionAuditSeatInput[],
): CodingSessionMissionAudit {
  const truncations: CodingSessionMissionAuditTruncation[] = [];
  const allTurns: CodingSessionMissionAuditTurn[] = [];
  const totalsBySeat: {
    seat: string;
    executionKey: string;
    totals: CodingSessionMissionAuditTotals;
  }[] = [];
  const handedTwice: CodingSessionMissionAuditHandedTwice[] = [];
  const roomDownloads: CodingSessionMissionAuditRoomDownload[] = [];
  const retryLoops: CodingSessionMissionAuditRetryLoop[] = [];
  let reportedSeatCount = 0;

  for (const seat of seats) {
    const turns = seatTurns(seat);
    allTurns.push(...turns);
    totalsBySeat.push({
      seat: seat.seat,
      executionKey: seat.executionKey,
      totals: sumTotals(turns),
    });
    if (turns.some(hasReportedUsage)) reportedSeatCount += 1;
    const calls = seatToolCalls(seat);
    handedTwice.push(...seatHandedTwice(seat.seat, calls));
    roomDownloads.push(...seatRoomDownloads(seat.seat, calls));
    retryLoops.push(...seatRetryLoops(seat.seat, calls));
  }

  allTurns.sort(
    (left, right) =>
      (left.startedAt ?? 0) - (right.startedAt ?? 0) ||
      left.seat.localeCompare(right.seat),
  );

  return {
    turns: bound(allTurns, "turns", "turns", truncations),
    totalsBySeat,
    sessionTotals: sumTotals(allTurns),
    seatCount: seats.length,
    reportedSeatCount,
    handedTwice: bound(
      handedTwice.sort((left, right) => right.count - left.count),
      "handed-twice",
      "repeated reads",
      truncations,
    ),
    roomDownloads: bound(
      roomDownloads.sort((left, right) => right.count - left.count),
      "room-downloads",
      "unbounded relay reads",
      truncations,
    ),
    retryLoops: bound(
      retryLoops.sort((left, right) => right.count - left.count),
      "retry-loops",
      "retry loops",
      truncations,
    ),
    truncations,
  };
}

/** `12 of 118 repeated reads shown; 106 are not listed.` */
function bound<T>(
  rows: readonly T[],
  section: CodingSessionMissionAuditTruncation["section"],
  noun: string,
  truncations: CodingSessionMissionAuditTruncation[],
): readonly T[] {
  const limit =
    section === "turns"
      ? CODING_SESSION_AUDIT_LIMITS.turns
      : CODING_SESSION_AUDIT_LIMITS.rows;
  if (rows.length <= limit) return rows;
  truncations.push({
    id: `audit-truncation-${section}`,
    section,
    notice: `Showing ${limit} of ${rows.length} ${noun}; ${
      rows.length - limit
    } are not listed.`,
  });
  return rows.slice(0, limit);
}

/**
 * Group one seat's items by the provider's turn id, newest turn last.
 *
 * Items outside any turn — session init, lifecycle status rows — are real, but
 * they are not a turn, and inventing one for them would put a row in the table
 * that nothing spent. Same rule as `execution_turns` in the CLI.
 */
function seatTurns(
  seat: CodingSessionMissionAuditSeatInput,
): CodingSessionMissionAuditTurn[] {
  const buckets = new Map<string, TranscriptItem[]>();
  const order: string[] = [];
  for (const item of seat.transcript) {
    const key = item.turnId;
    if (key === null || key === undefined || key.length === 0) continue;
    let bucket = buckets.get(key);
    if (bucket === undefined) {
      bucket = [];
      buckets.set(key, bucket);
      order.push(key);
    }
    bucket.push(item);
  }
  return order.map((key) => buildTurn(seat, key, buckets.get(key) ?? []));
}

function buildTurn(
  seat: CodingSessionMissionAuditSeatInput,
  turnId: string,
  items: readonly TranscriptItem[],
): CodingSessionMissionAuditTurn {
  const truncated = seat.transcriptTruncated === true;
  const result = items.find(
    (item) => item.type === "lifecycle" && item.title === TURN_RESULT_TITLE,
  );
  const usage = readTurnUsage(result);
  const published = items.filter((item) => item.type === "tool").length;
  return {
    seat: seat.seat,
    executionKey: seat.executionKey,
    turnId,
    startedAt: earliestTimestamp(items),
    durationMs:
      result !== undefined && result.type === "lifecycle"
        ? finiteOrNull(result.durationMs)
        : null,
    toolCalls: usage.toolCalls ?? published,
    toolCallsReported: usage.toolCalls !== null,
    toolCallsTruncated: truncated && usage.toolCalls === null,
    inputTokens: usage.inputTokens,
    outputTokens: usage.outputTokens,
    cacheReadTokens: usage.cacheReadTokens,
    cacheWriteTokens: usage.cacheWriteTokens,
    contextWindow: usage.contextWindow ?? reportedContextWindow(items),
    // Reported when the turn carries one, and `null` otherwise. This used to
    // be gated on a `usage.pricingIdentity` — but no such field exists on the
    // wire: `TurnUsageReport`
    // (`crates/beekeeper-core/src/coding_session_payload.rs`) is
    // `deny_unknown_fields` over six token fields, while the provider *does*
    // publish `cost_usd` (`crates/beekeeper-session-provider/src/lib.rs`). The gate
    // made the column dead by construction and printed `not reported` over a
    // cost the driver had reported — the one thing that was not true about it.
    // A1 dropped the same gate in the CLI. REVIEW-A3 F4.
    costUsd:
      result !== undefined && result.type === "lifecycle"
        ? finiteOrNull(result.costUsd)
        : null,
  };
}

/** Every number a turn's `usage` block reported, each independently absent. */
type TurnUsage = {
  toolCalls: number | null;
  inputTokens: number | null;
  outputTokens: number | null;
  cacheReadTokens: number | null;
  cacheWriteTokens: number | null;
  contextWindow: number | null;
};

const NO_USAGE: TurnUsage = {
  toolCalls: null,
  inputTokens: null,
  outputTokens: null,
  cacheReadTokens: null,
  cacheWriteTokens: null,
  contextWindow: null,
};

/**
 * Read the `result` item's `usage` block, defensively.
 *
 * `TranscriptItem`'s lifecycle member now carries the block
 * (`agentSessionTypes.ts`), and the projection fills it, but the read stays
 * field-by-field and total: the block is additive, every field is
 * independently optional, and an unreadable one must become `null` rather
 * than `0`. See {@link CODING_SESSION_AUDIT_USAGE_LIMIT}.
 */
function readTurnUsage(item: TranscriptItem | undefined): TurnUsage {
  if (item === undefined) return NO_USAGE;
  const usage = (item as { usage?: unknown }).usage;
  if (typeof usage !== "object" || usage === null) return NO_USAGE;
  const record = usage as Record<string, unknown>;
  return {
    toolCalls: finiteOrNull(record.toolCalls),
    inputTokens: finiteOrNull(record.inputTokens),
    outputTokens: finiteOrNull(record.outputTokens),
    cacheReadTokens: finiteOrNull(record.cacheReadTokens),
    cacheWriteTokens: finiteOrNull(record.cacheWriteTokens),
    contextWindow: finiteOrNull(record.contextWindow),
  };
}

/**
 * The window this turn's own occupancy item named, if it published one.
 *
 * `context_window_updated` survives projection as a status row whose text is
 * `key: value` lines, which is the one usage-shaped number this client can
 * read today — so the window column is real even while the token columns wait
 * on the projection.
 */
function reportedContextWindow(
  items: readonly TranscriptItem[],
): number | null {
  for (let index = items.length - 1; index >= 0; index -= 1) {
    const item = items[index];
    if (item.type !== "lifecycle" || item.title !== CONTEXT_WINDOW_TITLE) {
      continue;
    }
    for (const line of item.text.split("\n")) {
      const [rawKey, rawValue] = line.split(":");
      if (rawValue === undefined) continue;
      const key = rawKey.trim();
      if (
        key !== "size" &&
        key !== "contextWindow" &&
        key !== "contextLimit" &&
        key !== "maxTokens"
      ) {
        continue;
      }
      const value = Number(rawValue.trim());
      if (Number.isFinite(value) && value > 0) return value;
    }
  }
  return null;
}

function earliestTimestamp(items: readonly TranscriptItem[]): number | null {
  let earliest: number | null = null;
  for (const item of items) {
    const parsed = Date.parse(item.timestamp);
    if (!Number.isFinite(parsed)) continue;
    if (earliest === null || parsed < earliest) earliest = parsed;
  }
  return earliest;
}

function hasReportedUsage(turn: CodingSessionMissionAuditTurn): boolean {
  return (
    turn.inputTokens !== null ||
    turn.outputTokens !== null ||
    turn.cacheReadTokens !== null ||
    turn.cacheWriteTokens !== null ||
    turn.toolCallsReported
  );
}

/** Σ that stays `null` when nothing reported, and never invents a zero. */
function sumTotals(
  turns: readonly CodingSessionMissionAuditTurn[],
): CodingSessionMissionAuditTotals {
  return {
    turns: turns.length,
    reportedTurns: turns.filter(hasReportedUsage).length,
    pricedTurns: turns.filter((turn) => turn.costUsd !== null).length,
    durationMs: sum(turns, (turn) => turn.durationMs),
    toolCalls: turns.reduce((total, turn) => total + turn.toolCalls, 0),
    toolCallsReported: turns.every((turn) => turn.toolCallsReported),
    toolCallsTruncated: turns.some((turn) => turn.toolCallsTruncated),
    inputTokens: sum(turns, (turn) => turn.inputTokens),
    outputTokens: sum(turns, (turn) => turn.outputTokens),
    cacheReadTokens: sum(turns, (turn) => turn.cacheReadTokens),
    cacheWriteTokens: sum(turns, (turn) => turn.cacheWriteTokens),
    costUsd: sum(turns, (turn) => turn.costUsd),
  };
}

function sum(
  turns: readonly CodingSessionMissionAuditTurn[],
  read: (turn: CodingSessionMissionAuditTurn) => number | null,
): number | null {
  let total: number | null = null;
  for (const turn of turns) {
    const value = read(turn);
    if (value === null) continue;
    total = (total ?? 0) + value;
  }
  return total;
}

/**
 * One signed tool call, reduced to what the three lists need from it.
 *
 * A single call can carry several keys — an edit names its paths *and* may
 * have been driven by a command — so the keys are a list, exactly as
 * `handled_keys` builds them in the CLI. `result` is what the provider
 * published, which it clips at 8 KiB; `clipped` says when that happened.
 */
type AuditToolCall = {
  keys: readonly { what: "path" | "command"; key: string }[];
  /** The first `command` key, when this call had one. Drives the two lists. */
  command: string | null;
  bytes: number;
  clipped: boolean;
  /**
   * Whether a result reached this client at all. A projected call still open
   * (`status: "executing"`) has never been answered, which is not the same as
   * having been answered with nothing.
   */
  resultSeen: boolean;
  result: string;
};

/** Path-carrying argument names, in the CLI's order; `edit` paths come first. */
const PATH_ARGS = ["path", "file_path", "filePath", "abs_path", "absPath"];

function seatToolCalls(
  seat: CodingSessionMissionAuditSeatInput,
): AuditToolCall[] {
  const calls: AuditToolCall[] = [];
  for (const item of seat.transcript) {
    if (item.type !== "tool") continue;
    const keys: { what: "path" | "command"; key: string }[] = [];
    const pushPath = (value: string | null) => {
      if (value === null) return;
      if (keys.some((entry) => entry.key === value)) return;
      keys.push({ what: "path", key: value });
    };
    for (const path of item.editPaths ?? []) pushPath(nonEmpty(path));
    for (const field of PATH_ARGS) pushPath(readArgString(item.args, field));
    const command = readArgString(item.args, "command");
    if (command !== null) keys.push({ what: "command", key: command });
    if (keys.length === 0) continue;
    const resultSeen = item.status !== "executing" && item.status !== "pending";
    calls.push({
      keys,
      command,
      bytes: resultSeen ? utf8Bytes(item.result) : 0,
      clipped: item.result.includes(ELISION_MARKER),
      resultSeen,
      result: item.result,
    });
  }
  return calls;
}

function readArgString(
  args: Record<string, unknown>,
  field: string,
): string | null {
  return nonEmpty(args[field]);
}

function nonEmpty(value: unknown): string | null {
  return typeof value === "string" && value.trim().length > 0
    ? value.trim()
    : null;
}

/** Anything this seat asked for more than once, biggest repeat first. */
function seatHandedTwice(
  seat: string,
  calls: readonly AuditToolCall[],
): CodingSessionMissionAuditHandedTwice[] {
  const groups = new Map<
    string,
    {
      what: "path" | "command";
      key: string;
      count: number;
      resultsSeen: number;
      bytes: number;
      clipped: boolean;
    }
  >();
  for (const call of calls) {
    for (const { what, key } of call.keys) {
      const id = `${what}:${key}`;
      const group = groups.get(id) ?? {
        what,
        key,
        count: 0,
        resultsSeen: 0,
        bytes: 0,
        clipped: false,
      };
      group.count += 1;
      if (call.resultSeen) {
        group.resultsSeen += 1;
        group.bytes += call.bytes;
      }
      group.clipped ||= call.clipped;
      groups.set(id, group);
    }
  }
  return [...groups.values()]
    .filter((group) => group.count >= HANDED_TWICE_MIN)
    .map((group) => ({
      seat,
      what: group.what,
      key: group.key,
      count: group.count,
      resultsSeen: group.resultsSeen,
      // Null, not 0, when nothing came back — the reader would read `0 B` as
      // "it cost nothing", and what it means is "this client never saw the
      // bodies". An answered call that returned an empty body still counts,
      // and reports `0 B`, which is the true size of what it returned.
      bytes: group.resultsSeen > 0 ? group.bytes : null,
      bytesClipped: group.clipped,
    }));
}

/**
 * `sessions status|inbox|send|operation` — the reads with no bound.
 *
 * Counted per seat and per subcommand, never per invocation, so the row says
 * "this seat downloaded the room nine times" instead of listing nine
 * near-identical command lines. The name is the CLI's own
 * (`format!("sessions {verb}")`), so the ledger, `bee sessions audit` and this
 * tab print one string.
 */
function seatRoomDownloads(
  seat: string,
  calls: readonly AuditToolCall[],
): CodingSessionMissionAuditRoomDownload[] {
  const counts = new Map<string, number>();
  for (const call of calls) {
    if (call.command === null) continue;
    const command = roomDownloadCommand(call.command);
    if (command === null) continue;
    counts.set(command, (counts.get(command) ?? 0) + 1);
  }
  return [...counts.entries()].map(([command, count]) => ({
    seat,
    command,
    count,
  }));
}

/** `… | bee sessions operation get --id x` → `sessions operation`. */
export function roomDownloadCommand(command: string): string | null {
  const tokens = tokenizeShellCommand(command);
  for (let index = 0; index < tokens.length; index += 1) {
    const executable = tokens[index].split(/[\\/]/).pop();
    if (executable !== "bee" && executable !== "buzz") continue;
    let cursor = index + 1;
    while (cursor < tokens.length) {
      const token = tokens[cursor];
      if (token === "|" || token === ";" || token === "&") break;
      if (token.startsWith("-")) {
        // A long flag with a separate value consumes the next token, so
        // `--format compact sessions status` is still `sessions`.
        if (
          !token.includes("=") &&
          tokens[cursor + 1] !== undefined &&
          !tokens[cursor + 1].startsWith("-")
        ) {
          cursor += 1;
        }
        cursor += 1;
        continue;
      }
      if (token === "sessions") {
        const verb = tokens[cursor + 1];
        return verb !== undefined && ROOM_DOWNLOAD_VERBS.has(verb)
          ? `sessions ${verb}`
          : null;
      }
      break;
    }
  }
  return null;
}

/**
 * The longest run of the same command, back to back, answered the same way.
 *
 * Both halves matter. Repeating a command is normal — polling a build, or
 * re-running a test after an edit. Repeating it and getting a byte-identical
 * answer is the shape of the `seat-repair` loop the lead spent a turn in on
 * 2026-09-01, and it is the only thing this list claims. The count is the
 * **longest** such run, not the total across runs: a command a seat came back
 * to twice, hours apart, is work.
 */
function seatRetryLoops(
  seat: string,
  calls: readonly AuditToolCall[],
): CodingSessionMissionAuditRetryLoop[] {
  const ran = calls.filter(
    (call): call is AuditToolCall & { command: string } =>
      call.command !== null,
  );
  const longest = new Map<string, { length: number; resultsSeen: boolean }>();
  let index = 0;
  while (index < ran.length) {
    const start = ran[index];
    let end = index + 1;
    while (
      end < ran.length &&
      ran[end].command === start.command &&
      ran[end].resultSeen === start.resultSeen &&
      ran[end].result === start.result
    ) {
      end += 1;
    }
    // A run's members share one result by construction, so "were the results
    // seen" is a property of the run, not of its members.
    const run = { length: end - index, resultsSeen: start.resultSeen };
    const best = longest.get(start.command);
    if (best === undefined || run.length > best.length) {
      longest.set(start.command, run);
    }
    index = end;
  }
  return [...longest.entries()]
    .filter(([, run]) => run.length >= RETRY_LOOP_MIN)
    .map(([command, run]) => ({
      seat,
      command,
      count: run.length,
      identicalResults: run.resultsSeen ? true : null,
    }));
}

function utf8Bytes(value: string): number {
  return new TextEncoder().encode(value).length;
}

function finiteOrNull(value: unknown): number | null {
  return typeof value === "number" && Number.isFinite(value) ? value : null;
}

/**
 * `(1 of 2 turns priced)` beside an estimated cost that does not cover every
 * turn in its row (ledger 275 A5). Token reporting says nothing about price.
 */
export function unpricedDisclosure(
  totals: CodingSessionMissionAuditTotals,
): string | null {
  if (totals.turns === 0 || totals.pricedTurns === totals.turns) return null;
  return `(${totals.pricedTurns} of ${totals.turns} turn${
    totals.turns === 1 ? "" : "s"
  } priced)`;
}
