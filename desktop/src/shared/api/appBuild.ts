import { invokeTauri } from "./tauri";

/** What this build can say about the source it was compiled from. */
export type AppBuildIdentity = {
  /** Full 40-hex commit, or `null` when the build could not determine one. */
  commit: string | null;
  /** `git rev-list --count` of {@link commit}, or `null`. */
  commitCount: number | null;
  /**
   * `true` only when the build script *observed* a modified working tree.
   *
   * Never `false`: a clean claim is deliberately never embedded, because
   * Cargo cannot cheaply watch every untracked path and a stale false-clean
   * would survive an incremental rebuild. So `null` means "not observed
   * dirty", which must not be rendered as "clean".
   */
  sourceDirty: boolean | null;
};

/**
 * This app's own build identity — compile-time constants, no I/O behind the
 * IPC hop, so it cannot change while the app runs.
 */
export async function getAppBuildIdentity(): Promise<AppBuildIdentity> {
  return invokeTauri<AppBuildIdentity>("get_app_build_identity");
}
