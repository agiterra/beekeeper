/**
 * What a turn changed, said from the best evidence there is (SV-28), and the
 * git range the Diff surface asks for (SV-30).
 *
 * Three sources, never blended into one claim:
 *
 * - **git** — the turn's signed checkpoint (kind 44231) measured `baseTree` →
 *   `tree`. It sees a `sed -i` through a shell, which no transcript fold can.
 * - **observed** — the transcript's edit-tool calls. Always labelled
 *   "may be incomplete", because that is what it is.
 * - **unavailable** — the checkpoint says why there is no git answer
 *   ("not a git repository"), and any observed files ride beside that reason.
 *
 * A checkpoint whose baseline was not captured lists no files (the producer
 * cannot diff what it did not capture), so it reads "Baseline not captured" —
 * never "0 files".
 */
import type {
  CodingSessionChangedFile,
  CodingSessionChangedFileDiff,
} from "./codingSessionTranscriptModelTypes";
import {
  type CodingSessionCheckpointEntry,
  type CodingSessionCheckpointFileStatus,
  type CodingSessionCheckpointUnavailableCode,
  type CodingSessionGenerationCheckpoints,
  previousCodingSessionCheckpoint,
} from "./codingSessionCheckpoints";

export type CodingSessionTurnChangesSource = "git" | "observed" | "unavailable";

/** A listed file: the transcript's shape, plus git's status when from git. */
export type CodingSessionTurnChangeFile = CodingSessionChangedFile & {
  status: CodingSessionCheckpointFileStatus | null;
  from: string | null;
};

export type CodingSessionTurnChanges = Readonly<{
  source: CodingSessionTurnChangesSource;
  /** The card's first words: a count, or why there is none. */
  headline: string;
  /** Where the list came from, or null when the headline already says. */
  sourceLabel: string | null;
  files: readonly CodingSessionTurnChangeFile[];
  /** Changed files git counted but did not list. */
  filesNotListed: number;
  /** `true` only when git said files changed between turns. */
  outsideTurn: boolean;
  /** Paths left out of the captured tree: `omitted.length + omittedNotListed`. */
  notCaptured: number;
  baselineMissing: boolean;
  unavailable: Readonly<{
    code: CodingSessionCheckpointUnavailableCode;
    sentence: string;
  }> | null;
  /** The listed files are the transcript's, standing in for git's. */
  observedFallback: boolean;
  /** Secondary lines, each a disclosure the card must show unopened. */
  notes: readonly string[];
  /** Line counts are shown only when every listed file has them. */
  showTotals: boolean;
}>;

export const OBSERVED_SOURCE_LABEL =
  "Observed in transcript · may be incomplete";
export const GIT_SOURCE_LABEL = "From git";
export const OUTSIDE_TURN_NOTE = "Files also changed outside a turn";
/**
 * `git.complete: false` with no path omitted: the provider would not vouch
 * for the capture (for instance the next turn began before it finished, so
 * it may hold that turn's edits). Never read as a complete measurement.
 */
export const INCOMPLETE_CAPTURE_NOTE =
  "Capture incomplete · files may not be exactly this turn's";

const UNAVAILABLE_HEADLINES: Record<
  CodingSessionCheckpointUnavailableCode,
  string
> = {
  NOT_A_REPOSITORY: "No checkpoint · not a git repository",
  BOUNDARY_UNPREPARED: "No checkpoint · git boundary not prepared",
  TIMED_OUT: "No checkpoint · capture timed out",
  GIT_FAILED: "No checkpoint · git failed",
};

export function codingSessionCheckpointUnavailableHeadline(
  code: CodingSessionCheckpointUnavailableCode,
): string {
  return UNAVAILABLE_HEADLINES[code];
}

function plural(count: number, one: string, many: string): string {
  return `${count} ${count === 1 ? one : many}`;
}

function filenameOf(path: string): string {
  const index = path.lastIndexOf("/");
  return index < 0 ? path : path.slice(index + 1);
}

const NO_DIFFS: readonly CodingSessionChangedFileDiff[] = Object.freeze([]);

function fromObserved(
  files: readonly CodingSessionChangedFile[],
): CodingSessionTurnChangeFile[] {
  return files.map((file) => ({ ...file, status: null, from: null }));
}

/** The "not captured" note, naming only the reasons the checkpoint gave. */
export function codingSessionNotCapturedNote(
  entry: CodingSessionCheckpointEntry,
): string | null {
  const git = entry.payload.git;
  if (!git) return null;
  const total = git.omitted.length + git.omittedNotListed;
  if (total === 0) return null;
  const reasons = new Set(git.omitted.map((omission) => omission.reason));
  // Unnamed omissions carry no reason, so only a fully named list may say one.
  const why =
    git.omittedNotListed === 0 && reasons.size === 1
      ? reasons.has("too_large")
        ? "too large"
        : "unreadable"
      : "too large or unreadable";
  return `${plural(total, "file", "files")} not captured (${why})`;
}

/**
 * The card a turn shows, or null when there is nothing to say: no checkpoint
 * and no observed file, or a complete git measurement of no change that the
 * transcript does not contradict.
 */
export function deriveCodingSessionTurnChanges(input: {
  checkpoint: CodingSessionCheckpointEntry | null;
  observedFiles: readonly CodingSessionChangedFile[];
}): CodingSessionTurnChanges | null {
  const { checkpoint, observedFiles } = input;
  const observed = fromObserved(observedFiles);
  const observedCount = plural(
    observed.length,
    "changed file",
    "changed files",
  );
  if (!checkpoint) {
    if (observed.length === 0) return null;
    return Object.freeze({
      source: "observed",
      headline: observedCount,
      sourceLabel: OBSERVED_SOURCE_LABEL,
      files: observed,
      filesNotListed: 0,
      outsideTurn: false,
      notCaptured: 0,
      baselineMissing: false,
      unavailable: null,
      observedFallback: true,
      notes: [],
      showTotals: hasAllCounts(observed),
    });
  }
  const { payload } = checkpoint;
  if (payload.unavailable || !payload.git) {
    const unavailable = payload.unavailable ?? {
      code: "GIT_FAILED" as const,
      sentence: "The checkpoint carried no git facts.",
    };
    return Object.freeze({
      source: "unavailable",
      headline: codingSessionCheckpointUnavailableHeadline(unavailable.code),
      sourceLabel: null,
      files: observed,
      filesNotListed: 0,
      outsideTurn: false,
      notCaptured: 0,
      baselineMissing: false,
      unavailable,
      observedFallback: observed.length > 0,
      notes:
        observed.length > 0
          ? [`${observedCount} · ${OBSERVED_SOURCE_LABEL.toLowerCase()}`]
          : [],
      showTotals: observed.length > 0 && hasAllCounts(observed),
    });
  }
  const git = payload.git;
  const notes: string[] = [];
  const outsideTurn = git.outsideTurn === true;
  if (outsideTurn) notes.push(OUTSIDE_TURN_NOTE);
  const notCapturedNote = codingSessionNotCapturedNote(checkpoint);
  if (notCapturedNote) notes.push(notCapturedNote);
  const notCaptured = git.omitted.length + git.omittedNotListed;
  // Omissions already say why it is incomplete; without any, say it plainly.
  if (!git.complete && notCaptured === 0) notes.push(INCOMPLETE_CAPTURE_NOTE);
  if (git.baseTree === null) {
    if (observed.length > 0) {
      notes.push(`${observedCount} · ${OBSERVED_SOURCE_LABEL.toLowerCase()}`);
    }
    return Object.freeze({
      source: observed.length > 0 ? "observed" : "git",
      headline: "Baseline not captured",
      // The listed files are the transcript's, so the label must say so: git
      // contributed only the headline, never these files or their totals.
      sourceLabel:
        observed.length > 0 ? OBSERVED_SOURCE_LABEL : GIT_SOURCE_LABEL,
      files: observed,
      filesNotListed: 0,
      outsideTurn,
      notCaptured,
      baselineMissing: true,
      unavailable: null,
      observedFallback: observed.length > 0,
      notes,
      showTotals: observed.length > 0 && hasAllCounts(observed),
    });
  }
  const files: CodingSessionTurnChangeFile[] = payload.files.map((file) => ({
    path: file.path,
    filename: filenameOf(file.path),
    additions: file.additions,
    deletions: file.deletions,
    diffs: NO_DIFFS as CodingSessionChangedFileDiff[],
    editCount: 0,
    status: file.status,
    from: file.from,
  }));
  const total = files.length + payload.filesNotListed;
  if (payload.filesNotListed > 0) {
    notes.push(
      `${plural(payload.filesNotListed, "more file", "more files")} not listed`,
    );
  }
  if (total === 0 && notes.length === 0 && observed.length === 0) return null;
  return Object.freeze({
    source: "git",
    headline:
      total === 0
        ? "No files changed"
        : `${plural(total, "file", "files")} changed`,
    sourceLabel: GIT_SOURCE_LABEL,
    files,
    filesNotListed: payload.filesNotListed,
    outsideTurn,
    notCaptured,
    baselineMissing: false,
    unavailable: null,
    observedFallback: false,
    notes,
    showTotals:
      files.length > 0 && payload.filesNotListed === 0 && hasAllCounts(files),
  });
}

function hasAllCounts(files: readonly CodingSessionChangedFile[]): boolean {
  return files.every(
    (file) => file.additions !== null && file.deletions !== null,
  );
}

// ── The Diff surface's range ────────────────────────────────────────────────

export type CodingSessionDiffScope = "turn" | "session";

/** One file the wire listed, for the range the surface shows. */
export type CodingSessionDiffListedFile = Readonly<{
  path: string;
  status: CodingSessionCheckpointFileStatus;
  from: string | null;
  /** `null` when unknown — a binary file, or summed across turns. */
  additions: number | null;
  deletions: number | null;
}>;

export type CodingSessionDiffRange =
  | Readonly<{
      kind: "range";
      fromTree: string;
      toTree: string;
      /** The files the signed checkpoints list for this range. */
      files: readonly CodingSessionDiffListedFile[];
      filesNotListed: number;
      /** Why the range is not exactly what the scope promises, or null. */
      note: string | null;
      /** The checkpoint whose `tree` ends the range. */
      last: CodingSessionCheckpointEntry;
    }>
  | Readonly<{
      kind: "none";
      /** One sentence: why there is no git diff to show. */
      reason: string;
      files: readonly CodingSessionDiffListedFile[];
    }>;

/**
 * The git range for one scope.
 *
 * Turn: `baseTree` → `tree`; with no baseline, the previous checkpoint's
 * `tree` stands in and the note says so (it may include changes made between
 * turns). Session: the first checkpointed turn's `baseTree` → the latest
 * `tree`, under the same stand-in rule.
 */
export function resolveCodingSessionDiffRange(input: {
  generation: CodingSessionGenerationCheckpoints;
  scope: CodingSessionDiffScope;
  turn: CodingSessionCheckpointEntry | null;
}): CodingSessionDiffRange {
  const { generation, scope } = input;
  if (scope === "turn") {
    const entry = input.turn ?? generation.turns.at(-1) ?? null;
    if (!entry)
      return { kind: "none", reason: "No checkpoint yet.", files: [] };
    const git = entry.payload.git;
    if (!git) {
      const code = entry.payload.unavailable?.code ?? "GIT_FAILED";
      return {
        kind: "none",
        reason: `${codingSessionCheckpointUnavailableHeadline(code)}.`,
        files: [],
      };
    }
    const files = listedFiles(entry);
    if (git.baseTree !== null) {
      return {
        kind: "range",
        fromTree: git.baseTree,
        toTree: git.tree,
        files,
        filesNotListed: entry.payload.filesNotListed,
        note: null,
        last: entry,
      };
    }
    const previous = previousCodingSessionCheckpoint(generation, entry);
    const previousTree = previous?.payload.git?.tree ?? null;
    if (previousTree === null) {
      return {
        kind: "none",
        reason:
          "Baseline not captured, and no earlier checkpoint to diff from.",
        files,
      };
    }
    return {
      kind: "range",
      fromTree: previousTree,
      toTree: git.tree,
      files,
      filesNotListed: entry.payload.filesNotListed,
      note: "Baseline not captured · diffed from the previous checkpoint, so changes made between turns are included.",
      last: entry,
    };
  }
  const measured = generation.turns.filter((entry) => entry.payload.git);
  const first = measured[0];
  const last = measured.at(-1);
  if (!first || !last) {
    const code = generation.turns.at(-1)?.payload.unavailable?.code;
    return {
      kind: "none",
      reason: code
        ? `${codingSessionCheckpointUnavailableHeadline(code)}.`
        : "No checkpoint yet.",
      files: [],
    };
  }
  const files = unionFiles(measured);
  const notListed = measured.reduce(
    (total, entry) => total + entry.payload.filesNotListed,
    0,
  );
  const unmeasured = generation.turns.length - measured.length;
  const gap =
    unmeasured > 0
      ? ` ${plural(unmeasured, "turn has", "turns have")} no git checkpoint.`
      : "";
  const firstBase = first.payload.git?.baseTree ?? null;
  if (firstBase !== null) {
    return {
      kind: "range",
      fromTree: firstBase,
      toTree: last.payload.git?.tree ?? "",
      files,
      filesNotListed: notListed,
      note: gap ? gap.trim() : null,
      last,
    };
  }
  if (first === last) {
    return {
      kind: "none",
      reason: "Baseline not captured for the only checkpointed turn.",
      files,
    };
  }
  return {
    kind: "range",
    fromTree: first.payload.git?.tree ?? "",
    toTree: last.payload.git?.tree ?? "",
    files,
    filesNotListed: notListed,
    note: `The first turn's baseline was not captured · the diff starts after it.${gap}`,
    last,
  };
}

function listedFiles(
  entry: CodingSessionCheckpointEntry,
): CodingSessionDiffListedFile[] {
  return entry.payload.files.map((file) => ({
    path: file.path,
    status: file.status,
    from: file.from,
    additions: file.additions,
    deletions: file.deletions,
  }));
}

/**
 * Every path any turn listed, newest status winning. Line counts are dropped:
 * per-turn counts do not add up to a session's net change.
 */
function unionFiles(
  entries: readonly CodingSessionCheckpointEntry[],
): CodingSessionDiffListedFile[] {
  const byPath = new Map<string, CodingSessionDiffListedFile>();
  for (const entry of entries) {
    for (const file of entry.payload.files) {
      byPath.set(file.path, {
        path: file.path,
        status: file.status,
        from: file.from,
        additions: entries.length === 1 ? file.additions : null,
        deletions: entries.length === 1 ? file.deletions : null,
      });
    }
  }
  return [...byPath.values()].sort((left, right) =>
    left.path.localeCompare(right.path),
  );
}

// ── Patch text → the transcript's diff block ────────────────────────────────

export type CodingSessionPatchLine = {
  kind: "add" | "remove" | "context" | "meta";
  text: string;
};

/** A git patch as the lines `FileEditDiffBlock` renders. Headers dropped. */
export function codingSessionPatchLines(
  patch: string,
): CodingSessionPatchLine[] {
  const lines: CodingSessionPatchLine[] = [];
  for (const line of patch.split("\n")) {
    if (
      line.startsWith("diff --git ") ||
      line.startsWith("index ") ||
      line.startsWith("--- ") ||
      line.startsWith("+++ ")
    ) {
      continue;
    }
    if (line.startsWith("@@")) lines.push({ kind: "meta", text: line });
    else if (line.startsWith("+"))
      lines.push({ kind: "add", text: line.slice(1) });
    else if (line.startsWith("-")) {
      lines.push({ kind: "remove", text: line.slice(1) });
    } else if (line.startsWith(" ")) {
      lines.push({ kind: "context", text: line.slice(1) });
    } else if (line.length > 0) lines.push({ kind: "meta", text: line });
  }
  return lines;
}
