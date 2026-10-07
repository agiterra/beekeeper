import { invokeTauri } from "@/shared/api/tauri";

/**
 * File paths an agent wrote in a coding-session answer, resolved against that
 * execution's folder **on this computer** (SV-32).
 *
 * The host resolves the folder with the same resolver as the Files surface
 * (`session_tree.rs`), then each candidate against it and its git toplevel.
 * Open and reveal send the candidate **as the agent wrote it** and the host
 * resolves it again at click time; the renderer never hands an absolute path
 * to the opener.
 *
 * Mirrors `desktop/src-tauri/src/coding_sessions/file_refs.rs`.
 */

/** Which execution wrote the answer. None of these is a path. */
export type CodingSessionFileRefScope = {
  channelId: string;
  /** The provider-minted session id of the execution (`cs-target`). */
  providerSessionId: string | null;
  projectRef: string | null;
  /** A hire works only in its own worktree: the defaults are refused. */
  isHiredSeat: boolean;
  /** This machine's provider signed the execution's items. */
  isLocalProvider: boolean;
};

/** The host's cap on candidates per lookup; more is refused, not truncated. */
export const MAX_FILE_REF_CANDIDATES = 256;

export type CodingSessionFileRefWhere =
  | "thisComputer"
  | "folderGone"
  | "notLocal"
  | "notRecorded"
  | "storeUnreadable";

export type CodingSessionFileRef = {
  exists: boolean;
  isDir: boolean;
  /** Relative to the folder it resolved against; `null` when outside it. */
  relativePath: string | null;
  /** Only on this computer, only when it exists. Never for an event. */
  fullPath: string | null;
  line: number | null;
  column: number | null;
};

export type CodingSessionFileRefsAnswer = {
  where: CodingSessionFileRefWhere;
  source: "session" | "project" | "channel" | null;
  /** Why the paths stay plain text; `null` on this computer. */
  reason: string | null;
  checkedAt: string;
  /** Keyed by candidate exactly as asked; empty unless `thisComputer`. */
  refs: Record<string, CodingSessionFileRef>;
};

/** Resolve one execution's candidates. Rejects only for more than 256. */
export async function resolveCodingSessionFileRefs(
  scope: CodingSessionFileRefScope,
  candidates: readonly string[],
): Promise<CodingSessionFileRefsAnswer> {
  return invokeTauri<CodingSessionFileRefsAnswer>("coding_session_file_refs", {
    request: { ...scope, candidates: [...candidates] },
  });
}

/** Open the candidate in this computer's default app; the host re-resolves. */
export async function openCodingSessionFileRef(
  scope: CodingSessionFileRefScope,
  candidate: string,
): Promise<void> {
  await invokeTauri<void>("coding_session_open_file_ref", {
    request: { ...scope, candidate },
  });
}

/** Reveal the candidate in the file manager; the host re-resolves. */
export async function revealCodingSessionFileRef(
  scope: CodingSessionFileRefScope,
  candidate: string,
): Promise<void> {
  await invokeTauri<void>("coding_session_reveal_file_ref", {
    request: { ...scope, candidate },
  });
}
