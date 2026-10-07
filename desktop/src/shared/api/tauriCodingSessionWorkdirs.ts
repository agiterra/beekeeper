import { invokeTauri } from "@/shared/api/tauri";

/**
 * Host-local working directories for coding sessions.
 *
 * These paths never enter a relay event. A working directory names one
 * person's disk; publishing it into a channel would hand every member a
 * durable map of that machine for a value only that machine can use. The
 * desktop keeps them here and materializes the narrower view the provider
 * reads (`BEEKEEPER_CSP_PROJECTS_FILE`) on every mutation.
 *
 * Mirrors the Rust types in
 * `desktop/src-tauri/src/coding_sessions/workdir_store.rs`.
 */

export type CodingSessionWorkdirEntry = {
  path: string;
  updatedAt: string;
};

export type CodingSessionWorkdirMruEntry = {
  path: string;
  lastUsedAt: string;
};

export type CodingSessionWorkdirState = {
  version: number;
  /** Keyed by NIP-MP project coordinate (`30621:<owner>:<dtag>`). */
  byProject: Record<string, CodingSessionWorkdirEntry>;
  /** Keyed by channel id — the fallback when no project is involved. */
  byChannel: Record<string, CodingSessionWorkdirEntry>;
  /** Newest first, capped at ten. */
  mru: CodingSessionWorkdirMruEntry[];
  /** One-shot create hints, keyed by the 44221 `commandId` they belong to. */
  pending: Record<string, string>;
};

export type CodingSessionWorkdirScope = "project" | "channel";

export type CodingSessionWorkdirValidation = {
  exists: boolean;
  isDir: boolean;
  /**
   * The provider treats anything non-absolute as unconfigured, so a relative
   * path is reported here rather than failing a create later.
   */
  isAbsolute: boolean;
};

/**
 * The React Query key every view that caches `getCodingSessionWorkdirState`
 * reads through. A write to the store that does not invalidate it leaves a
 * stale record on screen (item 167); the literal was copied into four files
 * before this export existed, and they still spell it identically.
 */
export const CODING_SESSION_WORKDIR_STATE_QUERY_KEY = [
  "coding-session-workdir-state",
] as const;

/** Read every remembered directory, MRU entry, and staged hint. */
export async function getCodingSessionWorkdirState(): Promise<CodingSessionWorkdirState> {
  return invokeTauri<CodingSessionWorkdirState>(
    "get_coding_session_workdir_state",
  );
}

/** Remember a directory for a project coordinate or a channel. */
export async function setCodingSessionWorkdir(input: {
  scope: CodingSessionWorkdirScope;
  key: string;
  path: string;
}): Promise<CodingSessionWorkdirState> {
  return invokeTauri<CodingSessionWorkdirState>("set_coding_session_workdir", {
    scope: input.scope,
    key: input.key,
    path: input.path,
  });
}

/** Forget the directory remembered for a project coordinate or a channel. */
export async function clearCodingSessionWorkdir(input: {
  scope: CodingSessionWorkdirScope;
  key: string;
}): Promise<CodingSessionWorkdirState> {
  return invokeTauri<CodingSessionWorkdirState>(
    "clear_coding_session_workdir",
    { scope: input.scope, key: input.key },
  );
}

/** Promote a directory to the head of the MRU list. */
export async function recordCodingSessionWorkdirUse(
  path: string,
): Promise<CodingSessionWorkdirState> {
  return invokeTauri<CodingSessionWorkdirState>(
    "record_coding_session_workdir_use",
    { path },
  );
}

/**
 * Stage the directory one exact create command should run in.
 *
 * The provider resolves `pending[commandId]` first, so this is how a
 * standalone session gets a working directory that no project or channel
 * default would have supplied.
 *
 * With `projectRef`, the host records `rememberPath` (or `path` when omitted)
 * as that project's default when it has none yet, so a create issued from
 * another device (the phone cannot pick a folder on this machine) resolves through
 * `projects[projectRef]` instead of being refused `PROJECT_CWD_UNRESOLVED`.
 * A directory already set in project settings is left alone.
 */
export async function stageCodingSessionCreateHint(input: {
  commandId: string;
  path: string;
  projectRef?: string | null;
  /** Canonical checkout to remember; `path` still pins this command's execution. */
  rememberPath?: string | null;
}): Promise<CodingSessionWorkdirState> {
  return invokeTauri<CodingSessionWorkdirState>(
    "stage_coding_session_create_hint",
    {
      commandId: input.commandId,
      path: input.path,
      projectRef: input.projectRef ?? null,
      rememberPath: input.rememberPath ?? null,
    },
  );
}

/**
 * Stage a hired seat's create hint at the seat's own recorded worktree.
 *
 * The host reads the path from `worktrees/<sessionRef>/<seatLabel>` — never
 * from the caller — and rejects with `"<CODE>: <reason>"`
 * (`SEAT_CWD_UNRECORDED`, `SEAT_CWD_PROJECT_ROOT`, `SEAT_CWD_SHARED`) rather
 * than let the provider fall back to the project's checkout. Resolves to the
 * staged path.
 */
export async function stageCodingSessionSeatCreateHint(input: {
  commandId: string;
  sessionRef: string;
  seatLabel: string;
  projectRef: string | null;
}): Promise<string> {
  return invokeTauri<string>("stage_coding_session_seat_create_hint", {
    commandId: input.commandId,
    sessionRef: input.sessionRef,
    seatLabel: input.seatLabel,
    projectRef: input.projectRef,
  });
}

/** Drop a staged hint once its 44224 receipt has settled the create. */
export async function clearCodingSessionCreateHint(
  commandId: string,
): Promise<CodingSessionWorkdirState> {
  return invokeTauri<CodingSessionWorkdirState>(
    "clear_coding_session_create_hint",
    { commandId },
  );
}

/** Check a candidate directory before committing it to anything. */
export async function validateCodingSessionWorkdir(
  path: string,
): Promise<CodingSessionWorkdirValidation> {
  return invokeTauri<CodingSessionWorkdirValidation>(
    "validate_coding_session_workdir",
    { path },
  );
}

/** Open the OS folder picker. Resolves to `null` when the user cancels. */
export async function pickCodingSessionWorkdir(): Promise<string | null> {
  return invokeTauri<string | null>("pick_coding_session_workdir");
}
