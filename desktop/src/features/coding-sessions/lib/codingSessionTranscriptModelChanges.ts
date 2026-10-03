import { buildFileEditDiff } from "@/features/agents/ui/agentSessionFileEditDiff";
import type { TranscriptItem } from "@/features/agents/ui/agentSessionTypes";
import { getToolString } from "@/features/agents/ui/agentSessionUtils";
import type {
  CodingSessionChangedFile,
  CodingSessionChangedFileDiff,
  CodingSessionObservedChanges,
} from "@/features/coding-sessions/lib/codingSessionTranscriptModelTypes";
import { hasRedactionMarker } from "@/shared/lib/redactionMarker";

/*
 * Changed-file folding, split out of `codingSessionTranscriptModel.ts` for the
 * 1000-line ceiling. That module re-exports both public functions.
 */

/**
 * Fold a transcript's completed edits into per-file changes.
 *
 * The files half of {@link deriveCodingSessionObservedChanges}; kept as its own
 * export because the per-turn model only ever renders named files.
 */
export function deriveCodingSessionChangedFiles(
  items: TranscriptItem[],
): CodingSessionChangedFile[] {
  return deriveCodingSessionObservedChanges(items).files;
}

/**
 * Fold a transcript's completed edits into per-file changes *and* a count of
 * the edits that named no file.
 *
 * An item counts as an edit when the classifier recognized it as a file edit
 * **or** when the producer published ACP's own `edit` discriminant. The second
 * test is what makes real sessions work: claude-agent-acp names its editor
 * `Edit` — and, while the tool's arguments are still streaming, `Preparing
 * file…` — and no name rule in the classifier matches either, so a session's
 * every edit classified as "generic" and the fold never saw it.
 *
 * A path is taken from the diff, then the tool's arguments, then the
 * producer's published `edit.paths`, then the descriptor's object. An edit that
 * yields none is counted in `unreportedEditCount` rather than dropped.
 */
export function deriveCodingSessionObservedChanges(
  items: TranscriptItem[],
): CodingSessionObservedChanges {
  type MutableChangedFile = {
    path: string;
    filename: string;
    diffs: CodingSessionChangedFileDiff[];
    editCount: number;
    countedEditCount: number;
  };
  const files = new Map<string, MutableChangedFile>();
  let unreportedEditCount = 0;

  for (const item of items) {
    if (
      item.type !== "tool" ||
      item.isError ||
      item.status !== "completed" ||
      !isObservedFileEdit(item)
    ) {
      continue;
    }

    const diff = buildFileEditDiff(item, item.descriptor);
    const path =
      diff?.path ??
      getToolString(item.args, [
        "path",
        "file",
        "file_path",
        "filePath",
        "target_file",
      ]) ??
      item.editPaths?.[0] ??
      item.descriptor.object ??
      null;
    const normalizedPath = path ? normalizeChangedFilePath(path) : "";
    if (!normalizedPath) {
      unreportedEditCount += 1;
      continue;
    }
    const key = normalizedPath;
    const existing = files.get(key) ?? {
      path: normalizedPath,
      filename: changedFileBasename(normalizedPath),
      diffs: [],
      editCount: 0,
      countedEditCount: 0,
    };
    existing.editCount += 1;
    if (diff) {
      existing.diffs.push({ ...diff, id: item.id, path: normalizedPath });
      existing.countedEditCount += 1;
    }
    files.set(key, existing);
  }

  return {
    files: [...files.values()].map((file) => {
      const allEditsCounted =
        file.editCount > 0 && file.countedEditCount === file.editCount;
      return {
        path: file.path,
        filename: file.filename,
        additions: allEditsCounted
          ? file.diffs.reduce((total, diff) => total + diff.additions, 0)
          : null,
        deletions: allEditsCounted
          ? file.diffs.reduce((total, diff) => total + diff.deletions, 0)
          : null,
        diffs: file.diffs,
        editCount: file.editCount,
      };
    }),
    unreportedEditCount,
  };
}

/**
 * Whether a completed tool item is a file edit — by classification, or by the
 * producer's published ACP discriminant.
 */
function isObservedFileEdit(
  item: Extract<TranscriptItem, { type: "tool" }>,
): boolean {
  return (
    item.descriptor.renderClass === "file-edit" || item.toolKind === "edit"
  );
}

/**
 * Normalize a candidate path, or reject it as no path at all.
 *
 * Found live 2026-09-01 at 12:36: `FILES` printed
 * `[elided private context: 183 bytes, sha256:b35397…]` in the path slot and
 * `CHANGES` counted it as a second *named* edit. The provider redacts a host
 * path before signing, and the marker it leaves behind is the provider saying
 * "I had this and chose not to publish it" — which is precisely an edit with
 * no reported file name, not a file with a 90-character name.
 *
 * The empty string is the caller's existing signal for that, so the marker
 * takes the `unreportedEditCount` branch the field's own doc comment
 * describes. The marker's shape is owned by `shared/lib/redactionMarker` and
 * read from there; a second regex here would be a second definition of the
 * privacy contract, which is how the two readers drift apart.
 *
 * The predicate is a *contains* test on purpose: a marker anywhere in the
 * candidate makes the whole candidate unusable as a path. A real path that
 * merely contains the word `elided` matches nothing and is untouched.
 */
function normalizeChangedFilePath(path: string): string {
  if (hasRedactionMarker(path)) return "";
  return path
    .trim()
    .replace(/\\/g, "/")
    .replace(/^\.\/+/, "");
}

function changedFileBasename(path: string): string {
  return path.split("/").at(-1) || path;
}
