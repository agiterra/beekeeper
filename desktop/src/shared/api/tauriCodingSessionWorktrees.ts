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
