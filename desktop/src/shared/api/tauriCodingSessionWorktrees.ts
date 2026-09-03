import { invokeTauri } from "@/shared/api/tauri";

/**
 * Git worktrees for coding sessions.
 *
 * A worktree gives a session its own directory and branch — off a chosen
 * source branch, the trunk by default — so an agent can commit and switch
 * without disturbing what the person has open. Like the working directories
 * in `@/shared/api/tauriCodingSessionWorkdirs`, none of this is ever
 * published: a worktree path names one machine's disk.
 *
 * Mirrors the Rust types in
 * `desktop/src-tauri/src/coding_sessions/worktree.rs`.
 */

export type CodingSessionWorktreePlan = {
  /** Repository root of the working directory, when it is inside one. */
  repoRoot: string | null;
  /** Directory the worktree would occupy, or null when there is a problem. */
  path: string | null;
  /** Branch that would be created, matching the directory's slug. */
  branch: string | null;
  /** The slug actually chosen — the requested one, or a disambiguated form. */
  slug: string | null;
  /** True when the requested slug was taken and this one differs. */
  disambiguated: boolean;
  /**
   * Branch the new branch starts from — the requested source, else the
   * repository's default. Null means the checkout's current HEAD.
   */
  source: string | null;
  /** The one sentence explaining why no worktree can be planned. */
  problem: string | null;
};

export type CodingSessionWorktreeBranches = {
  /** Local branches, most recently committed first. */
  branches: string[];
  /** `main` when it exists, else `master`, else null. */
  defaultBranch: string | null;
  /** The branch the checkout has checked out, when it is on one. */
  headBranch: string | null;
};

export type CodingSessionWorktreeCreated = {
  path: string;
  branch: string;
  repoRoot: string;
};

/**
 * What creating a worktree here would do — checked against disk and the
 * repository's branches, not guessed from the name.
 */
export async function planCodingSessionWorktree(input: {
  workdir: string;
  name: string;
  source: string | null;
}): Promise<CodingSessionWorktreePlan> {
  return invokeTauri<CodingSessionWorktreePlan>(
    "plan_coding_session_worktree",
    {
      workdir: input.workdir,
      name: input.name,
      source: input.source,
    },
  );
}

/**
 * The branches a worktree in this working directory could start from, and
 * which of them is the default. An empty list when the directory is not a
 * git checkout.
 */
export async function listCodingSessionWorktreeBranches(input: {
  workdir: string;
}): Promise<CodingSessionWorktreeBranches> {
  return invokeTauri<CodingSessionWorktreeBranches>(
    "list_coding_session_worktree_branches",
    { workdir: input.workdir },
  );
}

/**
 * Create the worktree and its branch.
 *
 * The plan is recomputed inside the command, so the answer names where the
 * worktree actually landed — which may carry a disambiguating suffix the
 * preview did not show.
 */
export async function createCodingSessionWorktree(input: {
  workdir: string;
  name: string;
  source: string | null;
}): Promise<CodingSessionWorktreeCreated> {
  return invokeTauri<CodingSessionWorktreeCreated>(
    "create_coding_session_worktree",
    { workdir: input.workdir, name: input.name, source: input.source },
  );
}

// ── L11: the host's record of the worktrees it cut, and what may go ─────────
//
// The create path above records what it made; everything below reads that
// record. Every disposition was decided in Rust
// (`buzz_core::worktree_lifecycle`) — these wrappers carry the answer across,
// and the UI renders the strings it is given rather than re-deciding anything.

/** The stable tokens `classify_seat_worktree` can answer with. */
export const SEAT_WORKTREE_DISPOSITIONS = [
  "prunable",
  "held",
  "not-settled",
  "tip-not-on-relay",
  "execution-live",
  "unrecorded",
  "protected",
  "within-grace",
] as const;

export type SeatWorktreeDisposition =
  (typeof SEAT_WORKTREE_DISPOSITIONS)[number];

export function isSeatWorktreeDisposition(
  value: unknown,
): value is SeatWorktreeDisposition {
  return (SEAT_WORKTREE_DISPOSITIONS as readonly unknown[]).includes(value);
}

/** One recorded seat worktree, already classified by the host. */
export type SeatWorktreeRow = {
  key: string;
  sessionRef: string;
  seatLabel: string;
  path: string;
  branch: string;
  repoRoot: string;
  disposition: SeatWorktreeDisposition;
  /** `git status --porcelain` lines, which exclude ignored paths. */
  dirtyFiles: number;
  /** Rebuildable bytes, or `null` when they could not be measured. */
  reclaimableBytes: number | null;
  /** `{N} GB`, or `unknown` — never `0` for an unmeasurable directory. */
  reclaimableLabel: string;
  reclaimableNow: boolean;
  graceRemainingSecs: number | null;
  exists: boolean;
  /** Whether the relay's current ref state was established at all. */
  tipOnRelayKnown: boolean;
  /** The one sentence a surface shows for this row. */
  detail: string;
};

/** What the caller already knows about a session, from the relay. */
export type SeatWorktreeSessionFacts = {
  sessionRef: string;
  sessionSettled: boolean;
  executionLive: boolean;
  /** `null` means unestablished, which never renders as "not pushed". */
  tipOnRelay: boolean | null;
  settledForSecs: number | null;
};

/**
 * Kind 30618 is parameterized-replaceable, so it says where a ref stands
 * **now** and is never a push history. Every surface that shows a relay-tip
 * answer shows this sentence with it.
 */
export const TIP_ON_RELAY_LIMIT =
  "The relay's ref state says where a branch stands now, not whether it was ever pushed.";

export async function listCodingSessionSeatWorktrees(
  sessions: readonly SeatWorktreeSessionFacts[],
): Promise<SeatWorktreeRow[]> {
  return invokeTauri<SeatWorktreeRow[]>("list_coding_session_seat_worktrees", {
    sessions,
  });
}

/** Remove one recorded worktree. Only ever called from a person's own click. */
export async function pruneCodingSessionSeatWorktree(input: {
  sessionRef: string;
  seatLabel: string;
}): Promise<string> {
  return invokeTauri<string>("prune_coding_session_seat_worktree", input);
}

/** Remove `target/` and `desktop/node_modules` from one recorded worktree. */
export async function reclaimCodingSessionSeatWorktree(input: {
  sessionRef: string;
  seatLabel: string;
}): Promise<{
  removed: string[];
  freedBytes: number | null;
  freedLabel: string;
}> {
  return invokeTauri("reclaim_coding_session_seat_worktree", input);
}
