// Resolve the working directory a new project terminal should open in: the
// local checkout of one of the project's repositories, when one exists.
//
// "The project's folder" is its code: `list_project_local_repositories`
// reports every registered checkout (imported/linked from arbitrary paths,
// keyed by announcement dtag) plus the repos-root scan (keyed by folder
// name). The first project repo with a hit wins, in the project's own repo
// order. No hit (or not running under Tauri) falls back to the shell
// default ($HOME).

import { listProjectLocalRepositories } from "@/shared/api/projectGit";

/** The minimal repo identity needed for checkout matching. */
export type ShellCwdRepo = Pick<
  { dtag: string; name: string },
  "dtag" | "name"
>;

/**
 * The project repo (not just its path) whose registered/scanned checkout
 * matches, in the project's own repo order — first hit wins.
 *
 * Split out of `matchProjectCwd` so a caller that needs to know *which*
 * repository a checkout belongs to (LANE-L20: the launch path naming a
 * `repoRef`) is not left re-deriving the same match from the path alone.
 */
export function matchProjectCwdRepo<T extends ShellCwdRepo>(
  repos: readonly T[],
  localRepos: readonly { name: string; path: string }[],
): { repo: T; path: string } | undefined {
  const byName = new Map(localRepos.map((repo) => [repo.name, repo.path]));
  for (const repo of repos) {
    const path = byName.get(repo.dtag) ?? byName.get(repo.name);
    if (path) return { repo, path };
  }
  return undefined;
}

export function matchProjectCwd(
  repos: readonly ShellCwdRepo[],
  localRepos: readonly { name: string; path: string }[],
): string | undefined {
  return matchProjectCwdRepo(repos, localRepos)?.path;
}

/** Best-effort local-checkout lookup; `undefined` means "use the default". */
export async function projectDefaultCwd(
  repos: readonly ShellCwdRepo[],
): Promise<string | undefined> {
  if (repos.length === 0) return undefined;
  try {
    const local = await listProjectLocalRepositories({});
    return matchProjectCwd(repos, local);
  } catch {
    // Non-Tauri preview or command unavailable — default cwd.
    return undefined;
  }
}
