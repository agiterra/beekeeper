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

export type CodingSessionTranscriptToolItem = Extract<
  TranscriptItem,
  { type: "tool" }
>;

export type CodingSessionTranscriptEntry =
  | { kind: "item"; item: TranscriptItem }
  /**
   * Two or more consecutive settled, successful tool calls, read as one row
   * whose `label` is a sentence ("Read 3 files and ran 2 commands"). The
   * calls themselves are one click away, in place.
   */
  | {
      kind: "tool-group";
      id: string;
      label: string;
      items: CodingSessionTranscriptToolItem[];
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
 * position. A fold never hides the final answer, a prompt, a failure, a
 * permission request, a subagent batch, or a tool that never finished — see
 * `deriveCodingSessionTurnFold`.
 */
export type CodingSessionTurnFold = {
  anchorIndex: number;
  hiddenIndexes: readonly number[];
  /** The turn's own measured duration, or start-to-terminal when unmeasured. */
  durationMs: number | null;
  /** Sentence summary of the tool calls the fold hides; empty when none. */
  summary: string;
};

export type CodingSessionTranscriptTurn = {
  kind: "turn";
  id: string;
  entries: CodingSessionTranscriptEntry[];
  changedFiles: CodingSessionChangedFile[];
  diagnostics: TranscriptItem[];
  completion: CodingSessionTurnCompletion | null;
  isWorking: boolean;
  startedAt: string | null;
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
