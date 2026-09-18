import { invokeTauri } from "@/shared/api/tauri";

/**
 * Git worktrees for coding sessions.
 *
 * A worktree gives a session its own directory and branch — off a chosen
 * source branch, the trunk by default — so an agent can commit and switch
 * without disturbing what the person has open. Like the working directories
 * in `@/shared/api/tauriCodingSessionWorkdirs`, none of this is ever
 * published: a worktree path names one machine's disk.
 *
 * Mirrors the Rust types in
 * `desktop/src-tauri/src/coding_sessions/worktree.rs`.
 */

export type CodingSessionWorktreePlan = {
  /** Repository root of the working directory, when it is inside one. */
  repoRoot: string | null;
  /** Directory the worktree would occupy, or null when there is a problem. */
  path: string | null;
  /** Branch that would be created, matching the directory's slug. */
  branch: string | null;
  /** The slug actually chosen — the requested one, or a disambiguated form. */
  slug: string | null;
  /** True when the requested slug was taken and this one differs. */
  disambiguated: boolean;
  /** The folder the worktree's directory would sit in. */
  parent: string | null;
  /** Which rule chose it: "in-repo-holder" | "sibling" | "chosen". */
  placement: string | null;
  /** True when the repository has no working tree of its own. */
  bare: boolean;
  /** Why a folder the caller named was refused, when one was. */
  parentProblem: string | null;
  /**
   * Branch the new branch starts from — the requested source, else the
   * repository's default. Null means the checkout's current HEAD.
   */
  source: string | null;
  /** The one sentence explaining why no worktree can be planned. */
  problem: string | null;
};

export type CodingSessionWorktreeBranches = {
  /** Local branches, most recently committed first. */
  branches: string[];
  /** `main` when it exists, else `master`, else null. */
  defaultBranch: string | null;
  /** The branch the checkout has checked out, when it is on one. */
  headBranch: string | null;
};

export type CodingSessionWorktreeCreated = {
  path: string;
  branch: string;
  repoRoot: string;
};

/**
 * What creating a worktree here would do — checked against disk and the
 * repository's branches, not guessed from the name.
 */
export async function planCodingSessionWorktree(input: {
  workdir: string;
  name: string;
  source: string | null;
  /** A folder chosen for this repository's worktrees, or null for the default. */
  parent?: string | null;
}): Promise<CodingSessionWorktreePlan> {
  return invokeTauri<CodingSessionWorktreePlan>(
    "plan_coding_session_worktree",
    {
      workdir: input.workdir,
      name: input.name,
      source: input.source,
      parent: input.parent ?? null,
    },
  );
}

/**
 * Record a worktree that was cut before its session had a name.
 *
 * Safe to call for every settled session: the host resolves the directory to
 * its repository and records only what it recognises as a worktree it cut, so
 * a session running in an ordinary checkout records nothing. Resolves to
 * whether anything was recorded.
 */
export async function recordCodingSessionWorktree(input: {
  sessionRef: string;
  seatLabel: string;
  path: string;
  /**
   * The execution's own session id, when the caller knows it.
   *
   * The host publishes it to the provider as `sessions[<sessionId>]` in the
   * projects file, and the provider re-reads that at every gate. That is what
   * makes a relocated worktree reach a live session instead of leaving it
   * measuring a directory that no longer exists (finding 82).
   */
  sessionId?: string | null;
}): Promise<boolean> {
  return invokeTauri<boolean>("record_coding_session_worktree", {
    sessionRef: input.sessionRef,
    seatLabel: input.seatLabel,
    path: input.path,
    sessionId: input.sessionId ?? null,
  });
}

/**
 * Remember, or forget, the folder this repository's worktrees go in.
 *
 * Keyed host-side by the canonical repository root, so the choice survives
 * being reached from a subdirectory, a linked worktree, or the bare folder.
 * `null` forgets it and restores the defaults.
 */
export async function setCodingSessionWorktreeParent(input: {
  workdir: string;
  parent: string | null;
}): Promise<void> {
  await invokeTauri<void>("set_coding_session_worktree_parent", {
    workdir: input.workdir,
    parent: input.parent,
  });
}

/**
 * The branches a worktree in this working directory could start from, and
 * which of them is the default. An empty list when the directory is not a
 * git checkout.
 */
export async function listCodingSessionWorktreeBranches(input: {
  workdir: string;
}): Promise<CodingSessionWorktreeBranches> {
  return invokeTauri<CodingSessionWorktreeBranches>(
    "list_coding_session_worktree_branches",
    { workdir: input.workdir },
  );
}

/**
 * Create the worktree and its branch.
 *
 * The plan is recomputed inside the command, so the answer names where the
 * worktree actually landed — which may carry a disambiguating suffix the
 * preview did not show.
 *
 * `sessionRef`/`seatLabel` are the L11 record's own key, and are optional
 * only because a caller cutting a worktree before its session exists (the
 * lead's own, at the moment a mission is founded — `codingSessionCrewLaunch
 * .ts`) genuinely cannot supply them yet. Any caller that already knows the
 * session this worktree belongs to — a hire into a mission that has already
 * been founded, which is the overwhelming majority of seat worktrees cut —
 * must pass both, or `create_coding_session_worktree` records nothing and
 * the tree is invisible to `bee sessions worktree status/prune/reclaim` and
 * to Pulse's disk row until this host restarts (live-run finding 60: 27
 * worktrees staged, zero recorded).
 */
export async function createCodingSessionWorktree(input: {
  workdir: string;
  name: string;
  source: string | null;
  /** A folder chosen for this repository's worktrees, or null for the default. */
  parent?: string | null;
  sessionRef?: string | null;
  seatLabel?: string | null;
  /** The execution's session id, when it is already known at cut time. */
  sessionId?: string | null;
}): Promise<CodingSessionWorktreeCreated> {
  return invokeTauri<CodingSessionWorktreeCreated>(
    "create_coding_session_worktree",
    {
      workdir: input.workdir,
      name: input.name,
      source: input.source,
      parent: input.parent ?? null,
      sessionRef: input.sessionRef ?? null,
      seatLabel: input.seatLabel ?? null,
      sessionId: input.sessionId ?? null,
    },
  );
}

// ── L11: the host's record of the worktrees it cut, and what may go ─────────
//
// The create path above records what it made; everything below reads that
// record. Every disposition was decided in Rust
// (`buzz_core::worktree_lifecycle`) — these wrappers carry the answer across,
// and the UI renders the strings it is given rather than re-deciding anything.

/** The stable tokens `classify_seat_worktree` can answer with. */
export const SEAT_WORKTREE_DISPOSITIONS = [
  "prunable",
  "held",
  "not-settled",
  "tip-not-on-relay",
  "execution-live",
  "unrecorded",
  "protected",
  "within-grace",
] as const;

export type SeatWorktreeDisposition =
  (typeof SEAT_WORKTREE_DISPOSITIONS)[number];

export function isSeatWorktreeDisposition(
  value: unknown,
): value is SeatWorktreeDisposition {
  return (SEAT_WORKTREE_DISPOSITIONS as readonly unknown[]).includes(value);
}

/** One recorded seat worktree, already classified by the host. */
export type SeatWorktreeRow = {
  key: string;
  sessionRef: string;
  seatLabel: string;
  path: string;
  branch: string;
  repoRoot: string;
  disposition: SeatWorktreeDisposition;
  /** `git status --porcelain` lines, which exclude ignored paths. */
  dirtyFiles: number;
  /** Rebuildable bytes, or `null` when they could not be measured. */
  reclaimableBytes: number | null;
  /** `{N} GB`, or `unknown` — never `0` for an unmeasurable directory. */
  reclaimableLabel: string;
  reclaimableNow: boolean;
  graceRemainingSecs: number | null;
  exists: boolean;
  /** Whether the caller named this session as deleted. */
  sessionDeleted: boolean;
  /**
   * The provider session id recorded at cut time, or `null` for a tree cut
   * before its execution had one. Pairs a running execution's target with
   * its tree (spec § 4.9).
   */
  sessionId: string | null;
  /** Whether the relay's current ref state was established at all. */
  tipOnRelayKnown: boolean;
  /** The one sentence a surface shows for this row. */
  detail: string;
};

/** What the caller already knows about a session, from the relay. */
export type SeatWorktreeSessionFacts = {
  sessionRef: string;
  sessionSettled: boolean;
  /**
   * An accepted whole-session deletion ended this session.
   *
   * A deletion is not a closure — it takes the 44230 closures with it, so
   * nothing is left to fold — but it ends the work just as finally, and the
   * host disposes of the trees under exactly the same rules (a dirty tree is
   * still held). Omitted means "not said", which the host reads as false.
   */
  sessionDeleted?: boolean;
  executionLive: boolean;
  /** `null` means unestablished, which never renders as "not pushed". */
  tipOnRelay: boolean | null;
  settledForSecs: number | null;
};

/**
 * Kind 30618 is parameterized-replaceable, so it says where a ref stands
 * **now** and is never a push history. Every surface that shows a relay-tip
 * answer shows this sentence with it.
 */
export const TIP_ON_RELAY_LIMIT =
  "The relay's ref state says where a branch stands now, not whether it was ever pushed.";

export async function listCodingSessionSeatWorktrees(
  sessions: readonly SeatWorktreeSessionFacts[],
): Promise<SeatWorktreeRow[]> {
  return invokeTauri<SeatWorktreeRow[]>("list_coding_session_seat_worktrees", {
    sessions,
  });
}

/** Remove one recorded worktree. Only ever called from a person's own click. */
export async function pruneCodingSessionSeatWorktree(input: {
  sessionRef: string;
  seatLabel: string;
}): Promise<string> {
  return invokeTauri<string>("prune_coding_session_seat_worktree", input);
}

/** Remove `target/` and `desktop/node_modules` from one recorded worktree. */
export async function reclaimCodingSessionSeatWorktree(input: {
  sessionRef: string;
  seatLabel: string;
}): Promise<{
  removed: string[];
  freedBytes: number | null;
  freedLabel: string;
}> {
  return invokeTauri("reclaim_coding_session_seat_worktree", input);
}

/** What the host did with one seat's tree when its session closed. */
export type SeatWorktreeCloseOutcome = {
  key: string;
  path: string;
  disposition: SeatWorktreeDisposition;
  /** Whether the directory itself was removed. */
  pruned: boolean;
  /** Build-output bytes removed, or `null` when they could not be measured. */
  reclaimedBytes: number | null;
  tipOnRelayKnown: boolean;
  /** The one sentence for what happened — a prune as much as a refusal. */
  detail: string;
  /**
   * What happened to the seat's skill bundle, which lives outside every
   * checkout at `<app data dir>/agents/seats/<session id>`.
   *
   * Never inferred from `pruned`: a tree the host removed whose record carried
   * no session id names no bundle, and that is its own answer rather than a
   * silent success. `removed` is true only for the `removed` token.
   */
  bundle: SeatBundleRemoval;
};

/** What the host did with one seat's skill bundle, and the sentence for it. */
export type SeatBundleRemoval = {
  removed: boolean;
  /** The bundle path, when the host could name one. */
  path: string | null;
  /**
   * `removed`, `absent`, `unnamed_session`, `session_live`, `tree_held`,
   * `outside_root`, `unresolved_app_data`, `session_recorded` or `failed`.
   */
  token: string;
  detail: string;
};

/**
 * Let the host dispose of one seat's worktree, now that its session is closed.
 *
 * Safe to call for every seat of a session that just settled: the host decides
 * with the same predicate `bee sessions worktree prune` uses and refuses with a
 * sentence for anything it may not remove. Uncommitted work is never removed
 * here — that stays a person's own click.
 *
 * Three things can happen and they are independent: the tree is removed when
 * the predicate says `prunable`; the rebuildable build output is reclaimed as
 * soon as the session is settled and no execution is live, which needs no
 * grace window at all; and the seat's skill bundle goes with the tree, so a
 * held tree keeps the skills it was running on.
 */
export async function closeCodingSessionSeatWorktree(input: {
  sessionRef: string;
  seatLabel: string;
  executionLive: boolean;
  /** Seconds since the closure — or the deletion — ended the session. */
  settledForSecs: number | null;
  /**
   * Whether an accepted whole-session deletion ended it, rather than a
   * closure. Changes no rule; it changes the sentence, and it is why this
   * path runs at all after a delete (ledger 135(f)).
   */
  sessionDeleted?: boolean;
}): Promise<SeatWorktreeCloseOutcome> {
  return invokeTauri<SeatWorktreeCloseOutcome>(
    "close_coding_session_seat_worktree",
    {
      sessionRef: input.sessionRef,
      seatLabel: input.seatLabel,
      executionLive: input.executionLive,
      settledForSecs: input.settledForSecs,
      sessionDeleted: input.sessionDeleted ?? false,
    },
  );
}
