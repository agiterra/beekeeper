/**
 * The native `coding_session_checkpoint_diff` command (SV-30), typed.
 *
 * Mirrors `CodingSessionCheckpointDiff` in
 * `desktop/src-tauri/src/commands/coding_session_checkpoint_diff.rs`: either
 * the patch between two checkpoint trees, or *why there is none* — the trees
 * live on another computer, nothing is checked out here, or the checkpoint
 * has no baseline. An unknown answer shape is an error, never an empty diff.
 */
import { invokeTauri } from "@/shared/api/tauri";

export type CodingSessionCheckpointDiffFile = {
  path: string;
  additions: number;
  deletions: number;
  patch: string;
  /** Cut at the backend's per-file line cap. */
  truncated: boolean;
};

export type CodingSessionCheckpointDiffAnswer =
  | {
      state: "local";
      checkout: "seat_worktree" | "project_checkout";
      files: CodingSessionCheckpointDiffFile[];
      additions: number;
      deletions: number;
      filesNotListed: number;
    }
  | { state: "objects_missing"; missing: string[] }
  | { state: "no_checkout" }
  | { state: "baseline_missing" };

export type CodingSessionCheckpointDiffRequest = {
  /** The umbrella `sessionRef`; empty when the session has none. */
  sessionRef: string;
  /** The execution's provider session id (the 44223 `target`). */
  target: string;
  fromTree: string | null;
  toTree: string;
  projectRef: string | null;
};

type Invoke = (
  command: string,
  args: Record<string, unknown>,
) => Promise<unknown>;

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

function isCount(value: unknown): value is number {
  return typeof value === "number" && Number.isSafeInteger(value) && value >= 0;
}

/** Decode the command's answer strictly; anything else throws. */
export function decodeCodingSessionCheckpointDiff(
  raw: unknown,
): CodingSessionCheckpointDiffAnswer {
  if (!isRecord(raw))
    throw new Error("The checkpoint diff answer was unreadable.");
  switch (raw.state) {
    case "no_checkout":
      return { state: "no_checkout" };
    case "baseline_missing":
      return { state: "baseline_missing" };
    case "objects_missing": {
      const missing = raw.missing;
      if (
        !Array.isArray(missing) ||
        !missing.every((value) => typeof value === "string")
      ) {
        break;
      }
      return { state: "objects_missing", missing: [...missing] };
    }
    case "local": {
      const diff = raw.diff;
      if (
        (raw.checkout !== "seat_worktree" &&
          raw.checkout !== "project_checkout") ||
        !isRecord(diff) ||
        !Array.isArray(diff.files) ||
        !isCount(diff.additions) ||
        !isCount(diff.deletions) ||
        !isCount(raw.filesNotListed)
      ) {
        break;
      }
      const files: CodingSessionCheckpointDiffFile[] = [];
      for (const file of diff.files) {
        if (
          !isRecord(file) ||
          typeof file.path !== "string" ||
          !isCount(file.additions) ||
          !isCount(file.deletions) ||
          typeof file.patch !== "string" ||
          typeof file.truncated !== "boolean"
        ) {
          throw new Error("The checkpoint diff answer was unreadable.");
        }
        files.push({
          path: file.path,
          additions: file.additions,
          deletions: file.deletions,
          patch: file.patch,
          truncated: file.truncated,
        });
      }
      return {
        state: "local",
        checkout: raw.checkout,
        files,
        additions: diff.additions,
        deletions: diff.deletions,
        filesNotListed: raw.filesNotListed,
      };
    }
    default:
      break;
  }
  throw new Error("The checkpoint diff answer was unreadable.");
}

/** Ask this computer for the git diff between two checkpoint trees. */
export async function fetchCodingSessionCheckpointDiff(
  request: CodingSessionCheckpointDiffRequest,
  invoke: Invoke = (command, args) => invokeTauri<unknown>(command, args),
): Promise<CodingSessionCheckpointDiffAnswer> {
  const raw = await invoke("coding_session_checkpoint_diff", {
    sessionRef: request.sessionRef,
    target: request.target,
    fromTree: request.fromTree,
    toTree: request.toTree,
    projectRef: request.projectRef,
  });
  return decodeCodingSessionCheckpointDiff(raw);
}
