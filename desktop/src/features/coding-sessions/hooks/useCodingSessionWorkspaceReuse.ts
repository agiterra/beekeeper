import * as React from "react";

import {
  resolveWorkspaceDraftBranch,
  resolveWorkspaceReuse,
  type WorkspaceDirectoryRead,
  type WorkspaceDraftBranch,
  type WorkspaceReuseResolution,
} from "@/features/coding-sessions/lib/codingSessionWorkspaceReuse";
import {
  listCodingSessionSeatWorktrees,
  listCodingSessionWorktreeBranches,
  type SeatWorktreeRow,
  type SeatWorktreeSessionFacts,
} from "@/shared/api/tauriCodingSessionWorktrees";
import { validateCodingSessionWorkdir } from "@/shared/api/tauriCodingSessionWorkdirs";
import { getCodingSessionProviderStatus } from "@/shared/api/tauriSessionProvider";

/**
 * What "New session in this workspace" would reuse, read on demand.
 *
 * On demand means on menu open — never on render, hover, or scroll. A session
 * row is drawn hundreds of times in a sidebar and a disk stat per draw would
 * be a filesystem walk per scroll tick, so nothing here runs until a caller
 * asks. Both entry points (the sidebar row's context menu and the open
 * session's overflow) call this one hook, so the answer they show cannot
 * drift apart.
 *
 * The read budget is four host calls, at most, per resolve:
 *
 * 1. `list_coding_session_seat_worktrees` — the host's record of the trees it
 *    cut for this session.
 * 2. `coding_session_provider_status` — this computer's provider pubkey, to
 *    compare against the execution's authority. Runs beside (1).
 * 3. `validate_coding_session_workdir` — only when a row was found.
 * 4. `list_coding_session_worktree_branches` — only when that directory
 *    validated; its failure keeps the recorded branch rather than blanking it.
 *
 * Every call is wrapped: a host that answers nothing degrades to a truthful
 * "unrecorded"/"unknown" resolution plus `error`, never to a confident offer.
 */

/** The session a resolve is about, and what the caller already knows of it. */
export type CodingSessionWorkspaceReuseTarget = {
  sessionRef: string;
  /**
   * The viewed execution's `providerAuthorityPubkey`, when the caller has it.
   *
   * `null` is "this caller does not know", which resolves to an unknown
   * execution location — never to "somewhere else".
   */
  executionProviderPubkey: string | null;
  /**
   * Relay facts the seat read takes. Only the host's disposition wording
   * depends on them and this hook reads none of it, so conservative defaults
   * are safe; pass the real ones when the caller holds them.
   */
  facts?: Partial<Omit<SeatWorktreeSessionFacts, "sessionRef">>;
};

/** The outside world, injected so tests need no Tauri host. */
export type CodingSessionWorkspaceReuseDeps = {
  listSeatWorktrees: (
    sessions: readonly SeatWorktreeSessionFacts[],
  ) => Promise<SeatWorktreeRow[]>;
  validateWorkdir: (
    path: string,
  ) => Promise<{ exists: boolean; isDir: boolean }>;
  listBranches: (input: {
    workdir: string;
  }) => Promise<{ headBranch: string | null }>;
  providerStatus: () => Promise<{ providerPubkey?: string }>;
};

const HOST_DEPS: CodingSessionWorkspaceReuseDeps = {
  listSeatWorktrees: listCodingSessionSeatWorktrees,
  validateWorkdir: validateCodingSessionWorkdir,
  listBranches: listCodingSessionWorktreeBranches,
  providerStatus: getCodingSessionProviderStatus,
};

export type CodingSessionWorkspaceReuseRead = {
  resolution: WorkspaceReuseResolution;
  /**
   * The one sentence for a read that did not complete, or null.
   *
   * A seat read that threw leaves no rows, and no rows resolves to
   * "unrecorded". That is the right *offer* — there is nothing to reuse — but
   * "this computer recorded no directory" would be a claim the read never
   * earned, so a surface showing this read must show this instead when it is
   * set.
   */
  error: string | null;
};

/**
 * Ask this computer about one directory: is it there, and what is it on?
 *
 * The one place either surface touches a path. The menu's resolution and the
 * draft's branch line both come through here, so they cannot answer
 * differently about the same folder, and the two calls stay two calls.
 * Nothing throws out of it — a host that will not answer produces nulls, and
 * a null is "not known", never "not there".
 */
export async function readWorkspaceDirectory(
  path: string,
  deps: CodingSessionWorkspaceReuseDeps = HOST_DEPS,
): Promise<WorkspaceDirectoryRead> {
  const validation = await deps
    .validateWorkdir(path)
    .catch(() => null as { exists: boolean; isDir: boolean } | null);
  if (validation?.isDir !== true) return { validation, headBranch: null };
  const headBranch = await deps
    .listBranches({ workdir: path })
    .then((branches) => branches.headBranch)
    .catch(() => null);
  return { validation, headBranch };
}

/**
 * Perform the read. Exported without React so the budget itself is testable.
 */
export async function readCodingSessionWorkspaceReuse(
  target: CodingSessionWorkspaceReuseTarget,
  deps: CodingSessionWorkspaceReuseDeps = HOST_DEPS,
): Promise<CodingSessionWorkspaceReuseRead> {
  const facts: SeatWorktreeSessionFacts = {
    sessionRef: target.sessionRef,
    sessionSettled: target.facts?.sessionSettled ?? false,
    executionLive: target.facts?.executionLive ?? false,
    tipOnRelay: target.facts?.tipOnRelay ?? null,
    settledForSecs: target.facts?.settledForSecs ?? null,
  };

  const [rowsRead, statusRead] = await Promise.all([
    deps
      .listSeatWorktrees([facts])
      .then((rows) => ({ ok: true as const, rows }))
      .catch(() => ({ ok: false as const, rows: [] as SeatWorktreeRow[] })),
    deps
      .providerStatus()
      .then((status) => status.providerPubkey ?? null)
      .catch(() => null),
  ]);

  // No provider here, or no answer: the location is unknown. Never `false` —
  // "this computer holds no provider" is not evidence that the execution runs
  // on somebody else's.
  const executionIsLocal =
    statusRead === null || target.executionProviderPubkey === null
      ? null
      : statusRead === target.executionProviderPubkey;

  const row =
    rowsRead.rows.find(
      (candidate) =>
        candidate.sessionRef === target.sessionRef && candidate.path.length > 0,
    ) ?? null;

  // A foreign execution's answer withholds the path, so probing this
  // computer's disk for it would be two host calls spent on something the
  // resolution discards.
  const directory =
    row !== null && executionIsLocal !== false
      ? await readWorkspaceDirectory(row.path, deps)
      : { validation: null, headBranch: null };
  const validation = directory.validation;
  const liveBranch = directory.headBranch;

  return {
    resolution: resolveWorkspaceReuse({
      sessionRef: target.sessionRef,
      rows: rowsRead.rows,
      validation,
      executionIsLocal,
      liveBranch,
    }),
    error: rowsRead.ok
      ? null
      : "This computer could not read its record of this session's directories.",
  };
}

/**
 * One resolve, held for the surface that asked for it.
 *
 * The signature is the contract's: both entry points name the same question
 * the same way, so neither can grow its own read. `channelId` does not change
 * the answer — a directory belongs to a session, not to a channel — but it is
 * part of *which* question this is, so a held answer is dropped when either
 * moves rather than lingering under the next session's menu.
 *
 * `executionProviderPubkey` is the viewed execution's own
 * `providerAuthorityPubkey`. Omitting it is not "local": the resolution says
 * the execution's location is unknown and the offer degrades honestly, which
 * is why a caller that holds the value should pass it.
 *
 * No module-level state: the read and its answer live in this hook's own
 * state, so a community switch that remounts the tree takes them with it and
 * `resetCommunityState()` needs no new entry.
 */
export function useCodingSessionWorkspaceReuse(
  sessionRef: string,
  channelId: string | null,
  options: {
    /** The viewed execution's provider authority, when the caller holds it. */
    executionProviderPubkey?: string | null;
    /** Relay facts for the seat read; conservative defaults otherwise. */
    facts?: Partial<Omit<SeatWorktreeSessionFacts, "sessionRef">>;
    /** Injected only by tests. */
    deps?: CodingSessionWorkspaceReuseDeps;
  } = {},
): {
  /** Read now — call this when the menu opens. */
  resolve: () => Promise<WorkspaceReuseResolution>;
  /** The most recent answer for this session, or null before the first. */
  resolution: WorkspaceReuseResolution | null;
  /**
   * Set when the read itself failed. The resolution is still safe to act on —
   * it offers nothing — but a surface must show this rather than the
   * resolution's "recorded no directory" sentence, which the read never
   * earned.
   */
  error: string | null;
  isResolving: boolean;
} {
  const [read, setRead] =
    React.useState<CodingSessionWorkspaceReuseRead | null>(null);
  const [isResolving, setIsResolving] = React.useState(false);
  // Which resolve is allowed to publish. A menu re-opened before the previous
  // read landed must not be answered by the stale one.
  const generation = React.useRef(0);
  const mounted = React.useRef(true);
  React.useEffect(() => {
    mounted.current = true;
    return () => {
      mounted.current = false;
    };
  }, []);

  const executionProviderPubkey = options.executionProviderPubkey ?? null;
  const facts = options.facts;
  const deps = options.deps ?? HOST_DEPS;

  // Answering for a different session — or the same session reached in
  // another channel — starts from nothing rather than showing the previous
  // answer for a frame.
  const asked = React.useRef<string | null>(null);
  // Both halves are uuids, so a plain separator cannot make two different
  // questions collide.
  const question = `${channelId ?? "-"}/${sessionRef}`;
  if (asked.current !== question) {
    asked.current = question;
    if (read !== null) setRead(null);
  }

  const resolve = React.useCallback(async () => {
    generation.current += 1;
    const mine = generation.current;
    setIsResolving(true);
    try {
      const next = await readCodingSessionWorkspaceReuse(
        { sessionRef, executionProviderPubkey, facts },
        deps,
      );
      if (mounted.current && generation.current === mine) setRead(next);
      return next.resolution;
    } finally {
      if (mounted.current && generation.current === mine) setIsResolving(false);
    }
  }, [deps, executionProviderPubkey, facts, sessionRef]);

  return {
    resolve,
    resolution: read?.resolution ?? null,
    error: read?.error ?? null,
    isResolving,
  };
}

/**
 * The branch a reuse draft may show, read once when the draft opens.
 *
 * The request that opened the draft carries a *recorded* branch, and nothing
 * about a record says it is still true. So the head is read from that exact
 * directory once, on open — through `readWorkspaceDirectory`, the same path
 * the menu's resolution uses — and only then may the draft say "on disk now".
 * Nothing new is persisted: the answer lives for as long as the dialog does.
 *
 * Keyed on the path, so re-renders (typing in the form, a query settling) do
 * not read again. A different folder is a different question and reads once
 * more.
 */
export function useCodingSessionWorkspaceDraftBranch(
  workspace: {
    path: string;
    branch: string | null;
    /** Only ever `"recorded"` — see `WorkspaceBranchSource`. */
    branchSource?: "recorded" | null;
  } | null,
  deps: CodingSessionWorkspaceReuseDeps = HOST_DEPS,
): WorkspaceDraftBranch {
  const [read, setRead] = React.useState<WorkspaceDirectoryRead | null>(null);
  const path = workspace?.path ?? null;

  React.useEffect(() => {
    if (path === null) {
      setRead(null);
      return;
    }
    let cancelled = false;
    // The answer belongs to the path it was read for; a folder changed
    // mid-flight must not be described by the previous folder's head.
    setRead(null);
    void readWorkspaceDirectory(path, deps).then((next) => {
      if (!cancelled) setRead(next);
    });
    return () => {
      cancelled = true;
    };
  }, [deps, path]);

  return resolveWorkspaceDraftBranch({
    recordedBranch: workspace?.branch ?? null,
    recordedBranchSource: workspace?.branchSource ?? null,
    read,
  });
}
