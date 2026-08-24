import { invokeTauri } from "@/shared/api/tauri";

/**
 * Whether `git push` works from a terminal against the relay's git hosting.
 *
 * The app's own git runs with an ephemeral, env-only credential config, so a
 * repository it imported carries a Bee Keeper remote that a terminal cannot
 * authenticate to. These fields describe the *persistent* configuration, which
 * is the only thing a terminal can see.
 */
export type GitTerminalAccessStatus = {
  /** The credential scope, e.g. `https://hive.agiterra.org/git`. */
  scope: string;
  /** The configured helper: `nostr`, an absolute path, or null. */
  helper: string | null;
  /**
   * Whether that helper actually exists. Config naming a helper that is gone
   * reads as configured and fails at push time — the two states must not be
   * shown the same way.
   */
  helper_resolvable: boolean | null;
  use_http_path: string | null;
  keyfile: string | null;
  keyfile_present: boolean;
  keyfile_pubkey: string | null;
  keyfile_problem: string | null;
  /** All three in place. Anything less means pushes fail. */
  ready: boolean;
  pinned_helper: string;
  pinned_helper_present: boolean;
};

export function gitTerminalAccessStatus(): Promise<GitTerminalAccessStatus> {
  return invokeTauri<GitTerminalAccessStatus>("git_terminal_access_status");
}

/**
 * Provision terminal git access.
 *
 * This writes the identity key to a file on disk (`keyfile` in the result).
 * Only call it from an action where the user has been told that.
 */
export function enableGitTerminalAccess(): Promise<GitTerminalAccessStatus> {
  return invokeTauri<GitTerminalAccessStatus>("enable_git_terminal_access");
}
