import { codingSessionProseItemsEqual } from "@/features/coding-sessions/lib/codingSessionTranscriptModelText";
import type {
  CodingSessionChangedFile,
  CodingSessionTranscriptBlock,
  CodingSessionTranscriptEntry,
  CodingSessionTranscriptModel,
  CodingSessionTurnBackgroundTask,
  CodingSessionTurnCompletion,
} from "@/features/coding-sessions/lib/codingSessionTranscriptModelTypes";

/**
 * Reuses unchanged block objects across append-only transcript updates.
 *
 * The signed projection store emits a new transcript array for every frame.
 * Re-derivation remains linear and deterministic, but stable block identities
 * let memoized turn rows skip all prior DOM/Markdown/tool work when only the
 * live tail changed.
 *
 * Joined prose (`codingSessionTranscriptModelText.ts`) is rebuilt on every
 * derivation, so it is compared by id and words rather than identity —
 * without that, every settled turn with a multi-paragraph answer would look
 * changed on every event. Split out of `codingSessionTranscriptModel.ts`.
 */
export function stabilizeCodingSessionTranscriptModel(
  previous: CodingSessionTranscriptModel | null,
  next: CodingSessionTranscriptModel,
): CodingSessionTranscriptModel {
  if (!previous) return next;

  const blocks = next.blocks.map((block, index) => {
    const prior = previous.blocks[index];
    return prior && transcriptBlocksEqual(prior, block) ? prior : block;
  });
  const diagnostics = arraysReferenceEqual(
    previous.diagnostics,
    next.diagnostics,
  )
    ? previous.diagnostics
    : next.diagnostics;

  const sessionFacts = arraysReferenceEqual(
    previous.sessionFacts,
    next.sessionFacts,
  )
    ? previous.sessionFacts
    : next.sessionFacts;

  if (
    arraysReferenceEqual(previous.blocks, blocks) &&
    diagnostics === previous.diagnostics &&
    sessionFacts === previous.sessionFacts
  ) {
    return previous;
  }
  return { blocks, diagnostics, sessionFacts };
}

function transcriptBlocksEqual(
  left: CodingSessionTranscriptBlock,
  right: CodingSessionTranscriptBlock,
): boolean {
  if (left.kind !== right.kind || left.id !== right.id) return false;
  if (left.kind === "standalone" && right.kind === "standalone") {
    return transcriptEntriesEqual(left.entry, right.entry);
  }
  if (left.kind !== "turn" || right.kind !== "turn") return false;
  // `fold` is a pure function of the fields compared here, so equal inputs
  // mean an equal fold and the prior block's fold is kept with it.
  return (
    left.isWorking === right.isWorking &&
    left.superseded === right.superseded &&
    left.startedAt === right.startedAt &&
    backgroundTasksEqual(left.backgroundTasks, right.backgroundTasks) &&
    completionsEqual(left.completion, right.completion) &&
    changedFilesEqual(left.changedFiles, right.changedFiles) &&
    arraysReferenceEqual(left.diagnostics, right.diagnostics) &&
    left.entries.length === right.entries.length &&
    left.entries.every((entry, index) =>
      transcriptEntriesEqual(entry, right.entries[index]),
    )
  );
}

function changedFilesEqual(
  left: CodingSessionChangedFile[],
  right: CodingSessionChangedFile[],
): boolean {
  return (
    left.length === right.length &&
    left.every((file, index) => {
      const other = right[index];
      return (
        other !== undefined &&
        file.path === other.path &&
        file.filename === other.filename &&
        file.additions === other.additions &&
        file.deletions === other.deletions &&
        file.editCount === other.editCount &&
        file.diffs.length === other.diffs.length &&
        file.diffs.every((diff, diffIndex) => {
          const otherDiff = other.diffs[diffIndex];
          return (
            otherDiff !== undefined &&
            diff.id === otherDiff.id &&
            diff.path === otherDiff.path &&
            diff.filename === otherDiff.filename &&
            diff.additions === otherDiff.additions &&
            diff.deletions === otherDiff.deletions &&
            diff.lines.length === otherDiff.lines.length &&
            diff.lines.every((line, lineIndex) => {
              const otherLine = otherDiff.lines[lineIndex];
              return (
                otherLine !== undefined &&
                line.kind === otherLine.kind &&
                line.text === otherLine.text
              );
            })
          );
        })
      );
    })
  );
}

function transcriptEntriesEqual(
  left: CodingSessionTranscriptEntry,
  right: CodingSessionTranscriptEntry | undefined,
): boolean {
  if (!right || left.kind !== right.kind) return false;
  if (left.kind === "item" && right.kind === "item") {
    return codingSessionProseItemsEqual(left.item, right.item);
  }
  if (left.kind === "subagents" && right.kind === "subagents") {
    return (
      left.id === right.id &&
      left.label === right.label &&
      left.spawns.length === right.spawns.length &&
      left.spawns.every((spawn, index) => {
        const other = right.spawns[index];
        return (
          other !== undefined &&
          spawn.call === other.call &&
          arraysReferenceEqual(spawn.children, other.children)
        );
      })
    );
  }
  if (left.kind !== "tool-group" || right.kind !== "tool-group") return false;
  return (
    left.id === right.id &&
    left.label === right.label &&
    arraysReferenceEqual(left.items, right.items)
  );
}

function backgroundTasksEqual(
  left: readonly CodingSessionTurnBackgroundTask[],
  right: readonly CodingSessionTurnBackgroundTask[],
): boolean {
  return (
    left.length === right.length &&
    left.every((task, index) => {
      const other = right[index];
      return (
        other !== undefined &&
        task.id === other.id &&
        task.state === other.state &&
        task.status === other.status
      );
    })
  );
}

function completionsEqual(
  left: CodingSessionTurnCompletion | null,
  right: CodingSessionTurnCompletion | null,
): boolean {
  if (left === right) return true;
  if (!left || !right) return false;
  return (
    left.durationMs === right.durationMs &&
    left.costUsd === right.costUsd &&
    left.costBasis === right.costBasis &&
    left.outcome === right.outcome &&
    left.timestamp === right.timestamp &&
    left.state === right.state
  );
}

function arraysReferenceEqual<T>(
  left: readonly T[],
  right: readonly T[],
): boolean {
  return (
    left.length === right.length &&
    left.every((value, index) => value === right[index])
  );
}
