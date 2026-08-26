import type { CodingSessionWorktreeBranches } from "@/shared/api/tauriCodingSessionWorktrees";

/**
 * The branch a session's worktree starts from.
 *
 * Sessions used to branch from whatever the checkout had checked out, and a
 * checkout parked on an old topic branch quietly became the ancestor of every
 * new session made from it. The rule now: the trunk (`main`, then `master`)
 * unless the person picks another existing branch.
 */

/**
 * The selection the source picker should sit on, given what the repository
 * actually has.
 *
 * A choice that still exists is kept; anything else — no choice yet, or a
 * branch that vanished, or a workdir pointing at a different repository —
 * resolves to the default: the trunk, else the checkout's own branch, else
 * the most recently committed one. Null only when there is nothing to offer,
 * in which case the picker has no business rendering.
 */
export function resolveWorktreeSourceSelection({
  branches,
  current,
}: {
  branches: CodingSessionWorktreeBranches | null;
  current: string | null;
}): string | null {
  if (!branches || branches.branches.length === 0) {
    return null;
  }
  if (current !== null && branches.branches.includes(current)) {
    return current;
  }
  return (
    branches.defaultBranch ??
    branches.headBranch ??
    branches.branches[0] ??
    null
  );
}
