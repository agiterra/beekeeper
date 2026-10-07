import type { FileEditDiff } from "@/features/agents/ui/agentSessionFileEditDiff";
import type {
  CodingSessionCostBasis,
  TranscriptItem,
} from "@/features/agents/ui/agentSessionTypes";
import type { CodingSessionSubagentSpawn } from "@/features/coding-sessions/lib/codingSessionSubagents";

/**
 * The shapes `deriveCodingSessionTranscriptModel` produces.
 *
 * Split out of `codingSessionTranscriptModel.ts` for the 1000-line ceiling;
 * that module re-exports every name here, so importers keep one path.
 */

export type CodingSessionTurnCompletion = {
  durationMs: number | null;
  costUsd: number | null;
  /** Whose estimate `costUsd` is; `null` when the record named none. */
  costBasis: CodingSessionCostBasis | null;
  outcome: string | null;
  timestamp: string;
  state: "completed" | "failed" | "interrupted";
};

/**
 * Whether a turn's unended tool calls are still running, as far as anything
 * on screen can vouch for.
 *
 * - `live`: the turn is the one the session is working on now.
 * - `settled`: the turn is over — it reported a completion, a later turn
 *   exists, or the caller knows the session stopped. An unended call in it
 *   did not finish.
 * - `unknown`: nothing says either. The session is neither known to be
 *   working nor known to have stopped (waiting, disconnected, unread), so the
 *   honest reading of an unended call is that its status is not known.
 */
export type CodingSessionTurnSettlement = "live" | "settled" | "unknown";

/**
 * What the caller knows about the session when its latest turn has no
 * completion and is not the turn being worked on: `running` (the producer is
 * still on it), `stopped` (the session is idle, ended or otherwise definitely
 * not running), or `unknown`.
 */
export type CodingSessionTurnRestingStatus = "running" | "stopped" | "unknown";

/**
 * What the prose join (`codingSessionTranscriptModelText.ts`) adds to an
 * assistant message or a thought (CONTRACT.md, `conformance/transcript-prose-join`).
 *
 * A joined item keeps its first piece's id and `sourceEventId` (the
 * contract's `firstEventId`), so its row stays mounted as later paragraphs
 * arrive. These fields name where it currently ends. All are absent on a
 * single piece, which ends at itself.
 */
export type CodingSessionProseFields = {
  /** Item id of the message's last piece. */
  proseLastPieceId?: string;
  /** Signed event id of the message's last piece (the contract's `lastEventId`). */
  proseLastEventId?: string;
  /**
   * Rule 7: the producer is still writing this message — it is its turn's
   * last item, the turn has no result or interruption, and the caller holds
   * live evidence for the exact target (an unexpired lease on the current
   * generation, no session-ending status). Present only when true; never
   * derived from `isWorking`. See `codingSessionProseArriving.ts`.
   */
  arriving?: true;
};

export type CodingSessionTranscriptToolItem = Extract<
  TranscriptItem,
  { type: "tool" }
>;

export type CodingSessionTranscriptEntry =
  | { kind: "item"; item: TranscriptItem }
  /**
   * Two or more consecutive settled tool calls, read as one row whose `label`
   * is a sentence ("Read 3 files and ran 2 commands"). The calls themselves
   * are one click away, in place. A failed call joins only before the answer
   * of a turn that will fold (`groupAdjacentTools`); the label then ends
   * " · 1 failed" and `failedCount` says how many, so a renderer can tint
   * the row's icon as it tints a quiet failed step.
   */
  | {
      kind: "tool-group";
      id: string;
      label: string;
      items: CodingSessionTranscriptToolItem[];
      failedCount: number;
    }
  /** Consecutive Task/Agent spawns, each carrying its subagent's own items. */
  | {
      kind: "subagents";
      id: string;
      label: string;
      spawns: CodingSessionSubagentSpawn[];
    };

/**
 * How a settled turn folds its work behind one "Worked for …" row.
 *
 * Indexes point into the turn's own `entries`. The row sits at
 * `anchorIndex`; opening it shows every hidden entry again in its original
 * position. A fold never hides the final answer, a prompt, a permission
 * request, a subagent batch, or a failed or unfinished tool call after the
 * answer. Failed and unfinished calls *before* the answer do fold, and the
 * fold names them (SV-02) — see `deriveCodingSessionTurnFold`.
 */
export type CodingSessionTurnFold = {
  anchorIndex: number;
  hiddenIndexes: readonly number[];
  /** The turn's own measured duration, or start-to-terminal when unmeasured. */
  durationMs: number | null;
  /**
   * The whole fold sentence: `workSummary`, then `failureSummary` after
   * " · " when there is one ("Ran 5 commands · 1 step failed"). Empty when
   * the fold hides no tool call.
   */
  summary: string;
  /** Sentence summary of the tool calls the fold hides; empty when none. */
  workSummary: string;
  /**
   * The folded failures, named: "1 step failed", "2 steps failed and 1 did
   * not finish". Empty when every folded call succeeded. A renderer that
   * truncates `workSummary` must keep this on screen whole.
   */
  failureSummary: string;
  /** Failed tool calls among the hidden entries. */
  failedCount: number;
  /** Tool calls among the hidden entries that never settled. */
  unfinishedCount: number;
};

/**
 * A background task a turn started (SV-78): its id from the tool result that
 * announced it, and what the rest of the transcript says about it since.
 * `reported` carries the notification's own `status` word (`completed`,
 * `failed`, `killed`, …) or `null` when it named none. See
 * `deriveCodingSessionBackgroundTasks`.
 */
export type CodingSessionTurnBackgroundTask = {
  id: string;
  state: "running" | "reported" | "unreported" | "woke";
  status: string | null;
};

/**
 * A turn the agent began on its own, as the provider's status rows say
 * (SV-93). `cause` is `"background-task"` only when a row names a task
 * notification as what woke it (`autonomous_turn: the agent woke on
 * task-notification`); otherwise `null` — the provider said the turn was
 * unprompted and nothing more. `timestamp` is the first such row's.
 */
export type CodingSessionTurnAutonomousWake = {
  cause: "background-task" | null;
  timestamp: string;
};

export type CodingSessionTranscriptTurn = {
  kind: "turn";
  id: string;
  entries: CodingSessionTranscriptEntry[];
  changedFiles: CodingSessionChangedFile[];
  diagnostics: TranscriptItem[];
  completion: CodingSessionTurnCompletion | null;
  isWorking: boolean;
  /**
   * The turn reported no completion and a later turn exists in the same
   * transcript. The producer moved on, so this turn is over although its own
   * end was never reported — a fact of the items, never of the session's
   * current status. Always `false` on a turn with a completion, which is
   * settled by that alone.
   */
  superseded: boolean;
  startedAt: string | null;
  /**
   * Background tasks this turn started, in order (SV-78). A settled turn
   * with one still `running` has not plainly finished: its row says so.
   */
  backgroundTasks: readonly CodingSessionTurnBackgroundTask[];
  /**
   * The provider said nobody prompted this turn (SV-93): its
   * `autonomous_turn…` status rows, read as one fact. `null` on a prompted
   * turn, and on one a `<task-notification>` message opens — that message's
   * own row already says what woke it.
   */
  autonomousWake: CodingSessionTurnAutonomousWake | null;
  /** Always `null` while the turn is live: nothing folds while it is watched. */
  fold: CodingSessionTurnFold | null;
};

export type CodingSessionChangedFile = {
  path: string;
  filename: string;
  additions: number | null;
  deletions: number | null;
  diffs: CodingSessionChangedFileDiff[];
  editCount: number;
};

export type CodingSessionChangedFileDiff = FileEditDiff & {
  id: string;
};

export type CodingSessionTranscriptStandalone = {
  kind: "standalone";
  id: string;
  entry: CodingSessionTranscriptEntry;
};

export type CodingSessionTranscriptBlock =
  | CodingSessionTranscriptTurn
  | CodingSessionTranscriptStandalone;

export type CodingSessionTranscriptModel = {
  blocks: CodingSessionTranscriptBlock[];
  diagnostics: TranscriptItem[];
  /**
   * Every continuity and boundary row, in transcript order (SV-16). The
   * routine ones are not in `blocks`; a continuity loss is in both. Details
   * and the composer's sandbox chip read these — never an empty list as
   * "no boundary": absence here means nothing was published.
   */
  sessionFacts: TranscriptItem[];
};

/**
 * What the Observed-changes surface learned from a transcript.
 *
 * `files` is what can be named. `unreportedEditCount` is the honest remainder:
 * edits the transcript *does* record, for which no producer published a path.
 * The two are kept apart deliberately — folding the remainder into `files`
 * would invent a filename, and dropping it renders presence as absence, which
 * is what the 2026-08-29 walk found the surface doing over sixteen real edits.
 */
export type CodingSessionObservedChanges = {
  files: CodingSessionChangedFile[];
  unreportedEditCount: number;
};

/** The stable key of one entry: its item's id, or the group's own id. */
export function codingSessionTranscriptEntryKey(
  entry: CodingSessionTranscriptEntry,
): string {
  return entry.kind === "item" ? entry.item.id : entry.id;
}
