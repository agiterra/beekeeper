/**
 * Which repository a launch's create should name — finding 38.
 *
 * `session.create` has always accepted `repoRef`
 * (`coding_session_lifecycle_command.rs`'s `CREATE_ACTION_FORMS`, every
 * historical shape included), but nothing in the launch path ever wrote one:
 * every create the app ever signed carried `repoRef: null`, so Land
 * (`useCodingSessionMissionLand.ts`) could never resolve a repository for a
 * session the app created, no matter how clearly the checkout named one.
 *
 * Two paths, in the project's own repo order, matching LANE-L20's spec:
 *
 * 1. The repository whose local checkout the launch actually resolved — the
 *    same registered-checkout/repos-root-scan signal
 *    `projectShellCwd.ts#matchProjectCwdRepo` already uses for the workdir
 *    prefill (backed, server-side, by `project_repo_registry.rs`'s own
 *    clone-URL match against the checkout's remote).
 * 2. Failing that, the project's *only* repository — a checkout not yet
 *    linked or scanned still names the one repository the project could mean.
 *
 * Two or more repositories with no checkout match name none: this never
 * guesses, because a wrong `repoRef` is worse than the silence finding 38
 * documented — it would gate a push against the wrong repository's rules.
 */
import {
  matchProjectCwdRepo,
  type ShellCwdRepo,
} from "@/features/builtin-shell/lib/projectShellCwd";

/** The minimal repo identity `selectLaunchRepoRef` needs. */
export type LaunchRepoCandidate = ShellCwdRepo & {
  /** `30617:<owner>:<dtag>` — signed verbatim as the create's `repoRef`. */
  repoAddress: string;
};

export function selectLaunchRepoRef(input: {
  repos: readonly LaunchRepoCandidate[];
  localRepos: readonly { name: string; path: string }[];
}): string | null {
  const matched = matchProjectCwdRepo(input.repos, input.localRepos);
  if (matched) return matched.repo.repoAddress;
  return input.repos.length === 1 ? input.repos[0].repoAddress : null;
}
