import { invokeTauri } from "@/shared/api/tauri";

/**
 * Host-local working directories for coding sessions.
 *
 * These paths never enter a relay event. A working directory names one
 * person's disk; publishing it into a channel would hand every member a
 * durable map of that machine for a value only that machine can use. The
 * desktop keeps them here and materializes the narrower view the provider
 * reads (`BUZZ_CSP_PROJECTS_FILE`) on every mutation.
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
 */
export async function stageCodingSessionCreateHint(input: {
  commandId: string;
  path: string;
}): Promise<CodingSessionWorkdirState> {
  return invokeTauri<CodingSessionWorkdirState>(
    "stage_coding_session_create_hint",
    { commandId: input.commandId, path: input.path },
  );
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
