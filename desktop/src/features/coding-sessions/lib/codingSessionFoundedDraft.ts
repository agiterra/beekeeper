/**
 * What the click knew, remembered on this computer between the founding and
 * Start.
 *
 * A "New coding session" click founds the topic and opens its page; nothing
 * is cut and nothing beyond the genesis is signed until the founder commits a
 * field or presses Start (a worktree for a topic nobody starts is a tree
 * nobody asked for). So the facts the click resolved — where it should run,
 * whether to cut a worktree, which repository it names, which workspace it
 * reuses — are host-local, keyed by the umbrella's `sessionRef`, and the
 * founded page prefills from here. Founding on one desktop and opening the
 * page on another finds nothing — that page then shows the project default
 * and says so, rather than guessing.
 *
 * The prompt text is **not** here: it uses the ordinary goal draft
 * (`newCodingSessionDraft.ts`, scope `founded:<sessionRef>`), which already
 * has a size cap and a persistence line.
 *
 * v2 (2026-09-10) adds the Name field's unpublished text, the workspace-reuse
 * facts and `repoRef`. The reader still accepts a v1 record (the four
 * where-it-runs fields) and fills the rest with the defaults an ordinary
 * founding would have written, so last week's founded sessions keep their
 * prefill. `clear` removes both.
 *
 * Same fallbacks as `newCodingSessionDraft.ts`: unavailable storage reads as
 * nothing and writes as a no-op; a malformed or foreign record reads as
 * nothing.
 */
const FOUNDED_DRAFT_SCHEMA = "buzz-coding-session-founded-draft/v2";
const FOUNDED_DRAFT_KEY_PREFIX = "buzz.coding-session-founded-draft.v2:";
const FOUNDED_DRAFT_LEGACY_SCHEMA = "buzz-coding-session-founded-draft/v1";
const FOUNDED_DRAFT_LEGACY_KEY_PREFIX = "buzz.coding-session-founded-draft.v1:";

export type CodingSessionFoundedDraft = {
  /** The Name field's unpublished text; null once published or never typed. */
  name: string | null;
  workdir: string | null;
  useWorktree: boolean;
  worktreeName: string | null;
  worktreeSource: string | null;
  /** False when founded "in this workspace": the MRU must not learn the reused path. */
  rememberWorkspace: boolean;
  /** LANE-L20: resolved at the click — the checkout's repo or the source session's — never guessed. */
  repoRef: string | null;
  /** The reused workspace's path: the Where summary, and the rule that nulls `repoRef` when the workdir leaves it. */
  workspaceSourcePath: string | null;
  /** The recorded branch, for the summary's fallback when no head is readable. */
  workspaceSourceBranch: string | null;
  /** Never `"live"`: a head read at the click would be shown as current long after. */
  workspaceSourceBranchSource: "recorded" | null;
};

type StoredCodingSessionFoundedDraft = {
  schema: typeof FOUNDED_DRAFT_SCHEMA;
  sessionRef: string;
  draft: CodingSessionFoundedDraft;
};

type DraftStorage = Pick<Storage, "getItem" | "setItem" | "removeItem">;

/** The v2 key: where a draft is written, and read first. */
export function codingSessionFoundedDraftStorageKey(sessionRef: string) {
  return `${FOUNDED_DRAFT_KEY_PREFIX}${sessionRef}`;
}

/** The v1 key: read when no v2 record exists, and cleared alongside it. */
export function codingSessionFoundedDraftLegacyStorageKey(sessionRef: string) {
  return `${FOUNDED_DRAFT_LEGACY_KEY_PREFIX}${sessionRef}`;
}

function isNullableString(value: unknown): value is string | null {
  return value === null || typeof value === "string";
}

const V2_KEYS =
  "name,rememberWorkspace,repoRef,useWorktree,workdir,workspaceSourceBranch,workspaceSourceBranchSource,workspaceSourcePath,worktreeName,worktreeSource";
const V1_KEYS = "useWorktree,workdir,worktreeName,worktreeSource";

function parseWhereFields(record: Record<string, unknown>) {
  if (
    !isNullableString(record.workdir) ||
    typeof record.useWorktree !== "boolean" ||
    !isNullableString(record.worktreeName) ||
    !isNullableString(record.worktreeSource)
  ) {
    return null;
  }
  return {
    workdir: record.workdir,
    useWorktree: record.useWorktree,
    worktreeName: record.worktreeName,
    worktreeSource: record.worktreeSource,
  };
}

function parseDraftV2(value: unknown): CodingSessionFoundedDraft | null {
  if (typeof value !== "object" || value === null || Array.isArray(value)) {
    return null;
  }
  const record = value as Record<string, unknown>;
  if (Object.keys(record).sort().join(",") !== V2_KEYS) return null;
  const where = parseWhereFields(record);
  if (
    where === null ||
    !isNullableString(record.name) ||
    typeof record.rememberWorkspace !== "boolean" ||
    !isNullableString(record.repoRef) ||
    !isNullableString(record.workspaceSourcePath) ||
    !isNullableString(record.workspaceSourceBranch) ||
    (record.workspaceSourceBranchSource !== "recorded" &&
      record.workspaceSourceBranchSource !== null)
  ) {
    return null;
  }
  return {
    name: record.name,
    ...where,
    rememberWorkspace: record.rememberWorkspace,
    repoRef: record.repoRef,
    workspaceSourcePath: record.workspaceSourcePath,
    workspaceSourceBranch: record.workspaceSourceBranch,
    workspaceSourceBranchSource: record.workspaceSourceBranchSource,
  };
}

/** A v1 record, exact four keys, upgraded with what an ordinary founding writes. */
function parseDraftV1(value: unknown): CodingSessionFoundedDraft | null {
  if (typeof value !== "object" || value === null || Array.isArray(value)) {
    return null;
  }
  const record = value as Record<string, unknown>;
  if (Object.keys(record).sort().join(",") !== V1_KEYS) return null;
  const where = parseWhereFields(record);
  if (where === null) return null;
  return {
    name: null,
    ...where,
    rememberWorkspace: true,
    repoRef: null,
    workspaceSourcePath: null,
    workspaceSourceBranch: null,
    workspaceSourceBranchSource: null,
  };
}

function readRecord(
  storage: DraftStorage,
  key: string,
  schema: string,
  sessionRef: string,
): unknown {
  const stored = storage.getItem(key);
  if (stored === null) return undefined;
  const parsed: unknown = JSON.parse(stored);
  if (
    typeof parsed !== "object" ||
    parsed === null ||
    Array.isArray(parsed) ||
    !("schema" in parsed) ||
    parsed.schema !== schema ||
    !("sessionRef" in parsed) ||
    parsed.sessionRef !== sessionRef ||
    !("draft" in parsed)
  ) {
    return undefined;
  }
  return parsed.draft;
}

/**
 * The draft written for this umbrella, or null when none is readable.
 *
 * The v2 key is tried first; a v1 record is read only when there is no v2
 * record at all, and comes back upgraded.
 */
export function readCodingSessionFoundedDraft(
  sessionRef: string,
  storage?: DraftStorage,
): CodingSessionFoundedDraft | null {
  const targetStorage = storage ?? resolveDefaultStorage();
  if (!targetStorage) return null;
  try {
    const v2 = readRecord(
      targetStorage,
      codingSessionFoundedDraftStorageKey(sessionRef),
      FOUNDED_DRAFT_SCHEMA,
      sessionRef,
    );
    if (v2 !== undefined) return parseDraftV2(v2);
    const v1 = readRecord(
      targetStorage,
      codingSessionFoundedDraftLegacyStorageKey(sessionRef),
      FOUNDED_DRAFT_LEGACY_SCHEMA,
      sessionRef,
    );
    if (v1 !== undefined) return parseDraftV1(v1);
    return null;
  } catch {
    return null;
  }
}

/** Remember the draft, as v2. Returns false when storage refused it. */
export function writeCodingSessionFoundedDraft(
  sessionRef: string,
  draft: CodingSessionFoundedDraft,
  storage?: DraftStorage,
): boolean {
  const targetStorage = storage ?? resolveDefaultStorage();
  if (!targetStorage) return false;
  const record: StoredCodingSessionFoundedDraft = {
    schema: FOUNDED_DRAFT_SCHEMA,
    sessionRef,
    draft: {
      name: draft.name,
      workdir: draft.workdir,
      useWorktree: draft.useWorktree,
      worktreeName: draft.worktreeName,
      worktreeSource: draft.worktreeSource,
      rememberWorkspace: draft.rememberWorkspace,
      repoRef: draft.repoRef,
      workspaceSourcePath: draft.workspaceSourcePath,
      workspaceSourceBranch: draft.workspaceSourceBranch,
      workspaceSourceBranchSource:
        draft.workspaceSourceBranchSource === "recorded" ? "recorded" : null,
    },
  };
  try {
    targetStorage.setItem(
      codingSessionFoundedDraftStorageKey(sessionRef),
      JSON.stringify(record),
    );
    return true;
  } catch {
    return false;
  }
}

/** Forget the draft, both versions — on Start, when the answer has been used. */
export function clearCodingSessionFoundedDraft(
  sessionRef: string,
  storage?: DraftStorage,
): void {
  const targetStorage = storage ?? resolveDefaultStorage();
  if (!targetStorage) return;
  for (const key of [
    codingSessionFoundedDraftStorageKey(sessionRef),
    codingSessionFoundedDraftLegacyStorageKey(sessionRef),
  ]) {
    try {
      targetStorage.removeItem(key);
    } catch {
      // Unavailable storage: there is nothing to forget.
    }
  }
}

function resolveDefaultStorage(): DraftStorage | undefined {
  try {
    return globalThis.localStorage;
  } catch {
    return undefined;
  }
}
