import type {
  CodingSessionAssignmentInputEstablished,
  CodingSessionAssignmentInputOutcome,
  CodingSessionAssignmentInputRecord,
} from "./codingSessionAssignmentInput";

/**
 * The sentences a verifier's or a runner's assignment row says about the
 * revision this computer put into that seat's tree.
 *
 * Frozen copy, in one place, for the same reason the delivery table is: a
 * person reading a mission has to be able to tell "the seat is on the commit"
 * from "the seat is on whatever the trunk held", and a sentence rewritten per
 * surface is how those two came to look alike.
 *
 * Every established sentence carries the ordering disclosure. Establishing the
 * input is not sequenced against the lead's wake — the seat can be woken, and
 * can take a whole turn, before this host moves its tree — and a row that
 * stated the commit without saying so would be read as a guarantee it is not.
 */

/** The clause every established sentence ends with. */
export const CODING_SESSION_ASSIGNMENT_INPUT_NOT_ORDERED =
  "This is not ordered against the lead's wake, so a turn may have started first.";

/**
 * The clause that marks an answer as this computer's durable record rather
 * than something checked just now.
 *
 * A record is what happened once, not what is true now: the tree may have
 * moved since, by a person or by another surface. Saying so is the difference
 * between "the seat is on the commit" and "the seat was put on the commit at
 * some point", and only the second is what a record can support.
 */
export const CODING_SESSION_ASSIGNMENT_INPUT_RECORDED =
  "Recorded earlier on this computer, not checked again now.";

/** What one assignment row has to say about its verification input. */
export type CodingSessionAssignmentInputState =
  /**
   * The assignment is one whose role starts from a named revision, and it
   * names none.
   *
   * Its own arm, not a refusal: nothing was attempted and nothing could be.
   * Real verifier assignments were published without a `baseSha` (observed on
   * hive 2026-09-14, event `0aaf3387…`) and historical folding stays
   * tolerant of them, so these rows keep appearing in a mission's history and
   * a row that showed nothing would read as a seat correctly placed.
   */
  | { readonly kind: "unnamed" }
  /**
   * This computer's durable record of an earlier attempt **for this same
   * commit**, read on mount before anything was tried again.
   */
  | {
      readonly kind: "recorded";
      readonly commit: string;
      /** The reading of the recorded outcome, in the ordinary vocabulary. */
      readonly inner: CodingSessionAssignmentInputState;
      readonly record: CodingSessionAssignmentInputRecord;
    }
  /**
   * A record exists, and it is for a different commit — so there is **no**
   * attempt for the one this assignment names. Never shown as if it were.
   */
  | {
      readonly kind: "stale-record";
      readonly commit: string;
      readonly recordedCommit: string | null;
    }
  /** Attempting now. */
  | { readonly kind: "pending"; readonly commit: string }
  | {
      readonly kind: "established";
      readonly commit: string;
      readonly result: CodingSessionAssignmentInputEstablished;
    }
  | {
      readonly kind: "refused";
      readonly commit: string;
      readonly code: string;
      readonly message: string;
      readonly detail: string | null;
      readonly changes: number | null;
    }
  | {
      readonly kind: "unavailable";
      readonly commit: string;
      readonly message: string;
      readonly detail: string | null;
    };

/** Build the row's state from one attempt's outcome. */
export function codingSessionAssignmentInputState(
  commit: string,
  outcome: CodingSessionAssignmentInputOutcome,
): CodingSessionAssignmentInputState {
  if (outcome.kind === "established") {
    return { kind: "established", commit, result: outcome.result };
  }
  if (outcome.kind === "refused") {
    return {
      kind: "refused",
      commit,
      code: outcome.code,
      message: outcome.message,
      detail: outcome.detail,
      changes: outcome.changes,
    };
  }
  return {
    kind: "unavailable",
    commit,
    message: outcome.message,
    detail: outcome.detail,
  };
}

/**
 * Read one durable record as the state its outcome describes.
 *
 * The record's own `outcome` word decides, never its other fields: a record
 * whose word this build does not know stays `unavailable` — named, not
 * guessed at from a path that happens to be filled in.
 */
export function codingSessionAssignmentInputStateFromRecord(
  record: CodingSessionAssignmentInputRecord,
): CodingSessionAssignmentInputState {
  const commit = record.commit ?? "";
  if (
    record.outcome === "established" ||
    record.outcome === "already_current"
  ) {
    return {
      kind: "established",
      commit,
      result: {
        path: record.path ?? "",
        branch: record.branch ?? "",
        commit,
        remote: record.remote,
        alreadyCurrent: record.outcome === "already_current",
      },
    };
  }
  if (
    record.outcome in CODING_SESSION_ASSIGNMENT_INPUT_REFUSAL_COPY ||
    record.outcome === "dirty_tree"
  ) {
    return {
      kind: "refused",
      commit,
      code: record.outcome,
      message: record.message ?? record.outcome,
      detail: null,
      changes: record.changes,
    };
  }
  return {
    kind: "unavailable",
    commit,
    message: `This computer recorded an outcome this build does not know: ${record.outcome}.`,
    detail: null,
  };
}

/** Seven hex characters, the length git itself abbreviates to. */
export function shortCommit(commit: string): string {
  return commit.trim().slice(0, 7);
}

/** The plain sentence behind each contracted refusal code. */
export const CODING_SESSION_ASSIGNMENT_INPUT_REFUSAL_COPY: Readonly<
  Record<string, string>
> = {
  unrecorded_tree:
    "This computer did not cut that seat's worktree, so it established nothing.",
  missing_tree:
    "The seat's recorded worktree is no longer on disk, so nothing was checked out.",
  no_remote:
    "The seat's tree has no remote to fetch the commit from, so nothing was checked out.",
  ambiguous_remote:
    "The seat's tree has more than one remote and none could be chosen, so nothing was checked out.",
  fetch_failed:
    "Fetching the commit from the seat's remote failed, so nothing was checked out.",
  unknown_commit: "The commit is not on the resolved remote.",
  checkout_failed:
    "The seat's tree could not be moved onto the commit; nothing was discarded.",
  invalid_input:
    "This computer could not read the request for that verification input.",
};

/** The dirty-tree sentence, which names the count when the host gave one. */
export function codingSessionDirtyTreeSentence(changes: number | null): string {
  if (changes === null) {
    return "Uncommitted changes in the seat's tree; nothing was discarded.";
  }
  return `${changes} uncommitted change${changes === 1 ? "" : "s"} in the seat's tree; nothing was discarded.`;
}

/** What one row shows: a short word, a sentence, and the host's raw detail. */
export type CodingSessionAssignmentInputCopy = {
  /** Badge word. */
  badge: string;
  /** The whole sentence, which is what a person actually reads. */
  sentence: string;
  /** Raw host text for the row's `<details>` block, or null. */
  detail: string | null;
  /** True when a person may ask this computer to try the move again. */
  retryable: boolean;
};

/**
 * The one reading of an assignment's verification input.
 *
 * A refusal code this build has never seen still gets a sentence — its own
 * token, said out loud — rather than reading as nothing happened.
 */
export function codingSessionAssignmentInputCopy(
  state: CodingSessionAssignmentInputState,
): CodingSessionAssignmentInputCopy {
  if (state.kind === "unnamed") {
    return {
      badge: "no commit named",
      sentence:
        "No verification input named: this assignment does not name a commit to verify, so nothing was established.",
      detail: null,
      retryable: false,
    };
  }
  if (state.kind === "recorded") {
    const inner = codingSessionAssignmentInputCopy(state.inner);
    return {
      badge: `recorded: ${inner.badge}`,
      sentence: `${inner.sentence} ${CODING_SESSION_ASSIGNMENT_INPUT_RECORDED}`,
      detail: inner.detail,
      retryable: true,
    };
  }
  if (state.kind === "stale-record") {
    return {
      badge: "no attempt for this commit",
      sentence:
        state.recordedCommit === null
          ? "No attempt is recorded for this commit on this computer."
          : `No attempt is recorded for this commit: this computer's last recorded attempt was for ${shortCommit(state.recordedCommit)}, not ${shortCommit(state.commit)}.`,
      detail: null,
      retryable: true,
    };
  }
  if (state.kind === "pending") {
    return {
      badge: "establishing",
      sentence: `Establishing verification input ${shortCommit(state.commit)} in the seat's tree…`,
      detail: null,
      retryable: false,
    };
  }
  if (state.kind === "established") {
    const lead = state.result.alreadyCurrent
      ? "Verification input already current"
      : "Verification input established";
    return {
      badge: state.result.alreadyCurrent ? "already current" : "established",
      sentence: `${lead}: ${shortCommit(state.result.commit)} in ${state.result.path}. ${CODING_SESSION_ASSIGNMENT_INPUT_NOT_ORDERED}`,
      detail: null,
      retryable: true,
    };
  }
  if (state.kind === "unavailable") {
    return {
      badge: "unavailable",
      sentence:
        "Verification input not established: this build cannot establish it, so the seat is on whatever its tree already held.",
      detail: state.detail ?? state.message,
      retryable: true,
    };
  }
  const reason =
    state.code === "dirty_tree"
      ? codingSessionDirtyTreeSentence(state.changes)
      : (CODING_SESSION_ASSIGNMENT_INPUT_REFUSAL_COPY[state.code] ??
        `This computer refused with ${state.code}.`);
  return {
    badge: state.code.replaceAll("_", " "),
    sentence: `Verification input not established: ${reason}`,
    detail: state.detail ?? state.message,
    retryable: true,
  };
}
