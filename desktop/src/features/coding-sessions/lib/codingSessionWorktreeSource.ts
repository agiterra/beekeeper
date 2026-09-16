import { invokeTauri } from "@/shared/api/tauri";
import type { CodingSessionWorktreeBranches } from "@/shared/api/tauriCodingSessionWorktrees";

/**
 * The branch a session's worktree starts from.
 *
 * Sessions used to branch from whatever the checkout had checked out, and a
 * checkout parked on an old topic branch quietly became the ancestor of every
 * new session made from it. The rule now: the trunk (`main`, then `master`)
 * unless the person picks another existing branch.
 */

/**
 * The selection the source picker should sit on, given what the repository
 * actually has.
 *
 * A choice that still exists is kept; anything else — no choice yet, or a
 * branch that vanished, or a workdir pointing at a different repository —
 * resolves to the default: the trunk, else the checkout's own branch, else
 * the most recently committed one. Null only when there is nothing to offer,
 * in which case the picker has no business rendering.
 */
export function resolveWorktreeSourceSelection({
  branches,
  current,
}: {
  branches: CodingSessionWorktreeBranches | null;
  current: string | null;
}): string | null {
  if (!branches || branches.branches.length === 0) {
    return null;
  }
  if (current !== null && branches.branches.includes(current)) {
    return current;
  }
  return (
    branches.defaultBranch ??
    branches.headBranch ??
    branches.branches[0] ??
    null
  );
}

/**
 * What installing a seat's wip-share hooks reports back.
 *
 * Mirrors `CodingSessionSeatHooksInstalled` in
 * `desktop/src-tauri/src/commands/coding_session_seat_hooks.rs`.
 */
export type CodingSessionSeatHooksInstalled = {
  /** The ref this seat's commits will land on. */
  wipRef: string;
  /** The directory the hook files were written into. */
  hooksDir: string;
  /** The hook file names written. */
  hooksWritten: string[];
  /** `local` or `worktree` — which config file the lines went to. */
  configScope: string;
  /** `default`, `worktree`, or `external` — how git finds these hooks. */
  dispatch: string;
  /** False when a second install found everything already in place. */
  changed: boolean;
  /**
   * `keyfile` when the seat's commits will be signed, `unsigned` when no key
   * file could be named and the signing config was left out altogether.
   *
   * `unsigned` is a disclosure, not a failure: the seat still shares. The
   * signing lines are dropped rather than aimed at a path that does not
   * exist, because `commit.gpgsign = true` with no reachable key fails every
   * commit the seat makes.
   */
  signing: string;
};

/** The signing program the hooks configure. Resolved by git on `PATH`. */
const SEAT_GIT_SIGNER_PROGRAM = "git-sign-nostr";

/**
 * Install the wip-share git hooks into a seat's freshly cut worktree.
 *
 * After this the seat's commits reach Project Pulse on their own: `git commit`
 * is the whole of the seat's part, and nothing asks it to report.
 */
export function installCodingSessionSeatHooks(input: {
  worktreePath: string;
  seatRole: string;
  seatPubkey: string;
  keyfilePath: string | null;
  signerProgram: string;
  assignmentId: string | null;
  sessionRef: string | null;
  genesisRef: string | null;
  channelId: string | null;
  branch: string | null;
}): Promise<CodingSessionSeatHooksInstalled> {
  return invokeTauri<CodingSessionSeatHooksInstalled>(
    "install_coding_session_seat_hooks",
    { request: input },
  );
}

/**
 * The code {@link seatKeyfilePath} reports when there is no key file to name.
 *
 * Named rather than a bare `null` so a caller can tell "this host has not
 * looked" from "this host looked and there is nothing on disk".
 */
export const SEAT_KEY_IS_NOT_ON_DISK = "seat-key-is-not-on-disk";

/** Where a seat's signing key comes from, or why there is none to name. */
export type SeatSigningSource =
  | {
      readonly kind: "keyfile";
      /** Absolute path to a 0600 file holding the seat's `nsec`. */
      readonly path: string;
    }
  | {
      readonly kind: "none";
      /** Stable code — {@link SEAT_KEY_IS_NOT_ON_DISK}. */
      readonly code: typeof SEAT_KEY_IS_NOT_ON_DISK;
      /** The reason, in words a person can act on. */
      readonly why: string;
    };

/**
 * Where a seat's own Nostr key file lives, or why this host can name none.
 *
 * There is none, and the reason is a fact about this machine rather than an
 * omission. A seat's secret half exists in exactly three places and not one of
 * them is a file `git config nostr.keyfile` could point at:
 *
 * 1. The OS keyring, read only in Rust
 *    (`desktop/src-tauri/src/managed_agents/actor_seats.rs`).
 * 2. `actor-seats.json` beside the provider's `projects.json` — JSON keyed by
 *    the create's `commandId`, and the provider *deletes* the entry the moment
 *    it spawns the seat (same file, its module docs).
 * 3. The seat process's own environment: the provider injects
 *    `NOSTR_PRIVATE_KEY` into it
 *    (`crates/buzz-session-provider/src/actor_seats.rs:179`), and
 *    `buzz-dev-mcp`'s shim copies that into a 0600 file inside a tempdir that
 *    is destroyed when the process ends
 *    (`crates/buzz-dev-mcp/src/shim.rs:107`).
 *
 * The only stable key file on this computer is `~/.nostr/key`, which is the
 * *operator's* identity (`crates/buzz-cli/src/commands/git_setup.rs:157`), and
 * a seat signing its commits as the operator is a forged attribution.
 *
 * Nothing is lost by this: both `git-sign-nostr` and `git-credential-nostr`
 * read `$NOSTR_PRIVATE_KEY` before any key file, so a seat commanding git from
 * its own environment already signs and pushes as itself.
 */
export function seatKeyfilePath(_seatPubkey: string): SeatSigningSource {
  return {
    kind: "none",
    code: SEAT_KEY_IS_NOT_ON_DISK,
    why:
      "a seat's key is never written to a file this host can name — it lives in " +
      "the OS keyring and in the seat process's own $NOSTR_PRIVATE_KEY, so the " +
      "seat's commits are shared but not signed by this arming",
  };
}

/** What arming one seat's worktree did, or why it did nothing. */
export type SeatWipShareOutcome =
  | {
      readonly kind: "installed";
      /** What the installer wrote. */
      readonly installed: CodingSessionSeatHooksInstalled;
      /** Where the signing key came from, or why there was none. */
      readonly signing: SeatSigningSource;
    }
  | {
      readonly kind: "failed";
      /** The installer's own words. */
      readonly why: string;
    };

/**
 * Arm a newly cut seat worktree so its commits are shared.
 *
 * Best-effort by construction: it never throws, because an install that fails
 * must not cost the hire the seat it just cut. It is never silent either — a
 * failure comes back named, so a caller can say `not shared` and why rather
 * than saying nothing.
 */
export async function installSeatWipHooks(
  created: { path: string; branch: string },
  seat: {
    actor: string;
    role: string;
    sessionRef: string;
    genesisRef: string;
    channelId: string;
  },
  assignmentId: string | null,
): Promise<SeatWipShareOutcome> {
  const signing = seatKeyfilePath(seat.actor);
  try {
    const installed = await installCodingSessionSeatHooks({
      worktreePath: created.path,
      seatRole: seat.role,
      seatPubkey: seat.actor,
      keyfilePath: signing.kind === "keyfile" ? signing.path : null,
      signerProgram: SEAT_GIT_SIGNER_PROGRAM,
      assignmentId,
      sessionRef: seat.sessionRef,
      genesisRef: seat.genesisRef,
      channelId: seat.channelId,
      branch: created.branch,
    });
    return { kind: "installed", installed, signing };
  } catch (error) {
    return {
      kind: "failed",
      why: error instanceof Error ? error.message : String(error),
    };
  }
}

/** What arming the seat's own worktree to share its commits actually did. */
export type CodingSessionHireWipShare = {
  /** The ref the seat's commits will be force-pushed to, when it was armed. */
  ref: string | null;
  /**
   * `shared` when the hooks are in place, `unarmed` when the install failed,
   * `no-checkout` when this host had no working copy to cut a worktree from.
   */
  state: "shared" | "unarmed" | "no-checkout";
  /**
   * The named reason behind {@link CodingSessionHireWipShare.state}, or — for
   * a `shared` seat — the disclosure that its commits are unsigned because
   * this host can name no key file for it. Null when there is nothing to say.
   */
  why: string | null;
};

/**
 * The standing of a hire this host had no working copy to cut a tree from.
 *
 * **Unreachable from the hire path since 2026-09-16, and kept deliberately.**
 * A hire with no recorded checkout is now *refused*
 * (`HIRE_CHECKOUT_NOT_RECORDED`, `codingSessionHireCheckout.ts`) rather than
 * seated without a tree, so no live outcome carries `no-checkout` any more.
 * The state stays in the type because outcomes recorded before that change
 * carry it, and a renderer that met one would otherwise have no name for it.
 */
export function seatWipShareWithoutCheckout(): CodingSessionHireWipShare {
  return {
    ref: null,
    state: "no-checkout",
    why: "this host has no working copy for the channel, so the seat has no worktree of its own to share from",
  };
}

/**
 * Arm a seat's worktree and say, in one value, what that did.
 *
 * Wraps {@link installSeatWipHooks} so the hire path holds one call rather
 * than a branch: a seat that shares nothing says so here — on the outcome and
 * in the log — instead of the hire host going quiet, which is the failure
 * REVIEW-L9 F2 found.
 */
export async function armSeatWorktreeForSharing(
  created: { path: string; branch: string },
  seat: {
    actor: string;
    role: string;
    sessionRef: string;
    genesisRef: string;
    channelId: string;
  },
  assignmentId: string | null,
): Promise<CodingSessionHireWipShare> {
  const armed = await installSeatWipHooks(created, seat, assignmentId);
  if (armed.kind === "installed") {
    return {
      ref: armed.installed.wipRef,
      state: "shared",
      why: armed.signing.kind === "none" ? armed.signing.why : null,
    };
  }
  console.warn(
    `[coding-sessions] seat ${seat.role} shares nothing: ${armed.why}`,
  );
  return { ref: null, state: "unarmed", why: armed.why };
}
