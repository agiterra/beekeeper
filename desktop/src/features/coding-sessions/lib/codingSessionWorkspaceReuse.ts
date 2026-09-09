import type { SeatWorktreeRow } from "@/shared/api/tauriCodingSessionWorktrees";

/**
 * Can this computer honour "New session in this workspace" for one session?
 *
 * Pure, and deliberately so: every input is a fact somebody already read —
 * the host's record of the worktrees it cut, a directory validation, the
 * provider-pubkey comparison the app already does — and the answer is one
 * value a menu can render without asking anything else. The menu item exists
 * whatever this says (contract §3); what this decides is what the item opens
 * and what its second line reads.
 *
 * Every rule fails closed. An unread validation is "missing", not "probably
 * fine"; an unknown execution location is *unknown*, never "somewhere else";
 * and a session with no recorded directory is "unrecorded" rather than
 * borrowing the channel's remembered folder — a session launched with the
 * worktree toggle off leaves no durable session -> path record at all, so
 * there is nothing here to reuse and saying otherwise would seed the launcher
 * with a directory this session never ran in.
 */

export type WorkspaceReuseAvailability =
  /** A recorded directory on this computer, present now. */
  | "available"
  /** Recorded here, but the directory is gone. */
  | "missing"
  /** This computer recorded no directory for this session. */
  | "unrecorded"
  /** The session's execution runs on a provider this computer does not hold. */
  | "elsewhere";

export type WorkspaceReuseResolution = {
  availability: WorkspaceReuseAvailability;
  /** Only for "available"/"missing". Absolute, local, never published. */
  path: string | null;
  /** Recorded branch (creation-time fact) or the live head when read. */
  branch: string | null;
  branchSource: "recorded" | "live" | null;
  /** Session refs that also recorded a tree at this exact path — never a registry. */
  alsoHere: readonly string[];
  /** One sentence naming what is known, in this app's idiom. */
  sentence: string;
};

/**
 * The sentences, in one place, so Lane E's copy module can reuse or re-word
 * them rather than growing a second set that drifts. Each is an absence on
 * *this* computer — none of them claims anything about another machine.
 */
export const WORKSPACE_REUSE_SENTENCES = {
  /** Reuses the wording `describeWorkdirProblem` already shows for a gone path. */
  missing: "No such directory on this computer.",
  unrecorded: "This computer recorded no directory for this session.",
  elsewhere:
    "This session's execution runs on a provider this computer does not hold.",
  /**
   * Appended whenever the execution's provider could not be compared. Not a
   * claim that it is elsewhere — only that this computer cannot tell.
   */
  executionUnknown: "Where its execution runs is not known here.",
} as const;

/** "…is on this computer", with the branch when one is known. */
export function describeAvailableWorkspace(branch: string | null): string {
  return branch === null || branch.length === 0
    ? "This session's directory is on this computer."
    : `This session's directory is on this computer, on ${branch}.`;
}

function joinSentences(...parts: readonly (string | null)[]): string {
  return parts.filter((part): part is string => part !== null).join(" ");
}

/**
 * The row this session's workspace is read from.
 *
 * A session can have several seats and so several recorded trees. Preferring
 * a tree that still exists — and otherwise the first the host listed — keeps
 * the answer deterministic for the same read, which matters because the menu
 * re-resolves on every open.
 */
function selectRow(
  rows: readonly SeatWorktreeRow[],
  sessionRef: string,
): SeatWorktreeRow | null {
  const mine = rows.filter(
    (row) => row.sessionRef === sessionRef && row.path.length > 0,
  );
  return mine.find((row) => row.exists) ?? mine[0] ?? null;
}

/**
 * Other sessions whose rows name this exact directory.
 *
 * Drawn only from the rows already in hand — this is not a work registry and
 * never becomes one, so a read that returned nothing renders nothing rather
 * than implying the directory is unshared.
 */
function otherSessionsAt(
  rows: readonly SeatWorktreeRow[],
  sessionRef: string,
  path: string,
): readonly string[] {
  const seen = new Set<string>();
  for (const row of rows) {
    if (row.path !== path) continue;
    if (row.sessionRef === sessionRef) continue;
    seen.add(row.sessionRef);
  }
  return [...seen];
}

export function resolveWorkspaceReuse(input: {
  sessionRef: string;
  /** From `listCodingSessionSeatWorktrees`. Empty when the read failed. */
  rows: readonly SeatWorktreeRow[];
  /** From `validateCodingSessionWorkdir`; null when it was not run or failed. */
  validation: { exists: boolean; isDir: boolean } | null;
  /** Provider-pubkey comparison. `null` means this computer cannot tell. */
  executionIsLocal: boolean | null;
  /** The head read off disk, when it was read. */
  liveBranch?: string | null;
}): WorkspaceReuseResolution {
  const executionUnknown =
    input.executionIsLocal === null
      ? WORKSPACE_REUSE_SENTENCES.executionUnknown
      : null;
  const row = selectRow(input.rows, input.sessionRef);

  // No row wins over everything else, including a foreign execution: this
  // computer's honest answer is that it recorded nothing, and there is no
  // path to withhold.
  if (row === null) {
    return {
      availability: "unrecorded",
      path: null,
      branch: null,
      branchSource: null,
      alsoHere: [],
      sentence: joinSentences(
        WORKSPACE_REUSE_SENTENCES.unrecorded,
        executionUnknown,
      ),
    };
  }

  // A directory on this machine is no use to an execution running on another
  // provider's body, so the path is withheld rather than offered as a launch
  // hint that would silently run somewhere else.
  if (input.executionIsLocal === false) {
    return {
      availability: "elsewhere",
      path: null,
      branch: null,
      branchSource: null,
      alsoHere: [],
      sentence: WORKSPACE_REUSE_SENTENCES.elsewhere,
    };
  }

  const live =
    typeof input.liveBranch === "string" && input.liveBranch.length > 0
      ? input.liveBranch
      : null;
  const recorded = row.branch.length > 0 ? row.branch : null;
  const branch = live ?? recorded;
  const branchSource =
    live !== null ? "live" : recorded !== null ? "recorded" : null;
  const alsoHere = otherSessionsAt(input.rows, input.sessionRef, row.path);

  // Fail closed: only a row the host still sees *and* a validation that says
  // this is a directory make it available. A validation that never ran, or
  // one that came back false, is "missing" — never an unverified offer.
  const present =
    row.exists && input.validation?.exists === true && input.validation.isDir;

  if (!present) {
    return {
      availability: "missing",
      path: row.path,
      branch,
      branchSource,
      alsoHere,
      sentence: joinSentences(
        WORKSPACE_REUSE_SENTENCES.missing,
        executionUnknown,
      ),
    };
  }

  return {
    availability: "available",
    path: row.path,
    branch,
    branchSource,
    alsoHere,
    sentence: joinSentences(
      describeAvailableWorkspace(branch),
      executionUnknown,
    ),
  };
}

/**
 * One directory, as this computer just answered for it.
 *
 * Raw facts, not a verdict: `validation === null` means the check itself
 * failed, which is not evidence the directory is gone, and `headBranch ===
 * null` means the head was not read, which is not evidence there is no
 * branch. Each caller decides what it may say from that.
 */
export type WorkspaceDirectoryRead = {
  validation: { exists: boolean; isDir: boolean } | null;
  headBranch: string | null;
};

/**
 * A branch fact that survives being written down.
 *
 * `"recorded"` is the branch the worktree was cut on — a creation-time fact
 * that cannot go stale, so it may be persisted with the request and shown
 * straight away. `"live"` is the head as it was a moment ago; it is read on
 * open and never stored, because restored from storage it would be a claim
 * about the present made from a record.
 */
export type WorkspaceBranchSource = "recorded" | "live";

/** What a reuse draft may say about its folder's branch, and when. */
export type WorkspaceDraftBranch = {
  /**
   * The directory answered that it is not there. The draft says so and shows
   * no branch at all — a branch line over a folder that is gone is a fact
   * about nothing.
   */
  missing: boolean;
  branch: string | null;
  /**
   * `"live"` only when the head was actually read from that exact directory
   * during this open; `"recorded"` when the request carried a creation-time
   * branch and nothing newer was read.
   */
  branchSource: WorkspaceBranchSource | null;
};

/**
 * What the draft shows for its branch, from the read it just made.
 *
 * `read === null` is "not read yet" — the first frame after the dialog opens,
 * and every frame in a preview with no host. It shows the branch the request
 * carried, attributed only as far as the request itself could be: a
 * creation-time branch says so, anything else says nothing.
 */
export function resolveWorkspaceDraftBranch(input: {
  /** The branch the request carried, from the worktree's own record. */
  recordedBranch: string | null;
  /**
   * `"recorded"` when the request carried that branch as a creation-time
   * fact. Absent or null means the branch's provenance is not known here,
   * which is said rather than guessed.
   */
  recordedBranchSource?: "recorded" | null;
  read: WorkspaceDirectoryRead | null;
}): WorkspaceDraftBranch {
  const recorded =
    input.recordedBranch !== null && input.recordedBranch.length > 0
      ? input.recordedBranch
      : null;
  const recordedSource =
    recorded !== null && input.recordedBranchSource === "recorded"
      ? "recorded"
      : null;
  if (input.read === null) {
    return { missing: false, branch: recorded, branchSource: recordedSource };
  }
  const validation = input.read.validation;
  // Only a validation that *answered* can say a directory is gone. One that
  // threw leaves the recorded branch standing, unattributed.
  if (validation !== null && (!validation.exists || !validation.isDir)) {
    return { missing: true, branch: null, branchSource: null };
  }
  const head = input.read.headBranch;
  if (head !== null && head.length > 0) {
    return { missing: false, branch: head, branchSource: "live" };
  }
  return { missing: false, branch: recorded, branchSource: recordedSource };
}

/**
 * Whether the launcher can actually honour a seeded workspace yet.
 *
 * `false` until **all three** of root's changes land in the reserved files
 * (`CONTEXTUAL_SESSIONS_IMPL.md` §1):
 *
 * 1. `NewCodingSessionForm` accepts a `workspaceReuse` prop;
 * 2. its two state initializers read it —
 *    `useState(workspaceReuse?.path ?? "")` for the directory and
 *    `useState(workspaceReuse === null)` for the worktree toggle;
 * 3. `rememberWorkspace` is threaded to the create, so a one-off directory
 *    neither enters the MRU nor becomes a project's first default.
 *
 * Until then the form still starts empty with "Use a worktree" ticked, so a
 * disclosure above it would state one folder over a field about to use a
 * different one — a draft that lies about what pressing Start will do, which
 * is worse than an unseeded draft. **Flipping this without those three is not
 * a cosmetic change; it publishes that lie.** The prop is passed to the form
 * regardless: harmless today, correct the moment the seam lands.
 */
export const WORKSPACE_REUSE_SEAM_LANDED = false;
