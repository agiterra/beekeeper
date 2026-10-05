import { invokeTauri } from "@/shared/api/tauri";

/**
 * Where a coding session's working tree is on **this machine**, as the host
 * resolves it — never its path.
 *
 * The host looks the tree up in its own workdir store: the worktree filed
 * under the session id, then (only when this machine's provider runs the
 * execution, and never for a hired seat) the project's checkout, then the
 * channel's folder. The renderer
 * gets which record answered and relative entries under it; the absolute path
 * stays in Rust, like every other workdir fact (`tauriCodingSessionWorkdirs.ts`).
 *
 * Mirrors `desktop/src-tauri/src/coding_sessions/session_tree.rs`.
 */

export type CodingSessionTreeQuery = {
  /** The focused execution's provider-minted session id, when known. */
  sessionId: string | null;
  channelId: string;
  projectRef: string | null;
  /** Gates the project and channel defaults (DB9). */
  isLocalProvider: boolean;
  /**
   * An agent is seated on the execution (a hire). A hired seat works only in
   * the worktree cut for it, so the project and channel defaults are refused
   * for it — the provider's own `hired_seat_cwd_refusal` rule.
   */
  isHiredSeat?: boolean;
};

export type CodingSessionTreeSource = "session" | "project" | "channel";

export type CodingSessionTreeRefusal =
  | "notLocal"
  | "notRecorded"
  | "storeUnreadable";

export type CodingSessionTreeResolution = {
  available: boolean;
  source: CodingSessionTreeSource | null;
  label: string;
  reason: string | null;
  refusal: CodingSessionTreeRefusal | null;
};

export type CodingSessionTreeEntry = {
  name: string;
  /** `/`-separated, relative to the tree's root. */
  relPath: string;
  kind: "directory" | "file" | "symlink";
};

export type CodingSessionTreeListing = {
  entries: CodingSessionTreeEntry[];
  /** True when the folder held more than the host's cap (2,000). */
  truncated: boolean;
  /**
   * Entries present but not listed because the host could not represent them
   * (a non-UTF-8 name, an unreadable entry). Non-zero means incomplete;
   * `.git`, hidden by contract, is never counted. Optional only so an older
   * host's answer still decodes; absent reads as unknown-zero.
   */
  omitted?: number;
};

/** Resolve the session's tree on this computer. Never rejects for "no tree". */
export async function resolveCodingSessionTree(
  session: CodingSessionTreeQuery,
): Promise<CodingSessionTreeResolution> {
  return invokeTauri<CodingSessionTreeResolution>(
    "resolve_coding_session_tree",
    { session },
  );
}

/**
 * List one folder of the session's tree. `relPath` is relative (`""` for the
 * root); the host refuses `..`, absolute paths, `.git` (also through a
 * symlink) and symlinks out.
 */
export async function listCodingSessionTreeEntries(
  session: CodingSessionTreeQuery,
  relPath: string,
): Promise<CodingSessionTreeListing> {
  return invokeTauri<CodingSessionTreeListing>(
    "list_coding_session_tree_entries",
    { session, relPath },
  );
}
