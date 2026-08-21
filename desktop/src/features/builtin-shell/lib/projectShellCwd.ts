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

export function matchProjectCwd(
  repos: readonly ShellCwdRepo[],
  localRepos: readonly { name: string; path: string }[],
): string | undefined {
  const byName = new Map(localRepos.map((repo) => [repo.name, repo.path]));
  for (const repo of repos) {
    const path = byName.get(repo.dtag) ?? byName.get(repo.name);
    if (path) return path;
  }
  return undefined;
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
