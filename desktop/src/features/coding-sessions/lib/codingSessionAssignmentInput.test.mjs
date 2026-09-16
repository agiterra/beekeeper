import assert from "node:assert/strict";
import test from "node:test";

import {
  CODING_SESSION_ASSIGNMENT_INPUT_REFUSALS,
  decodeCodingSessionAssignmentInputError,
  isCodingSessionInputBoundRole,
} from "./codingSessionAssignmentInput.ts";
import {
  CODING_SESSION_ASSIGNMENT_INPUT_NOT_ORDERED,
  CODING_SESSION_ASSIGNMENT_INPUT_RECORDED,
  codingSessionAssignmentInputCopy,
  codingSessionAssignmentInputState,
  codingSessionAssignmentInputStateFromRecord,
} from "./codingSessionAssignmentInputCopy.ts";

/**
 * The client's one job that matters: a contract refusal and "this build cannot
 * answer" are different facts, and neither is ever an establishment.
 */

class TauriInvokeError extends Error {
  constructor(message, payload) {
    super(message);
    this.name = "TauriInvokeError";
    this.payload = payload;
  }
}

test("a thrown contract error maps to its refusal code, verbatim", () => {
  for (const code of CODING_SESSION_ASSIGNMENT_INPUT_REFUSALS) {
    const outcome = decodeCodingSessionAssignmentInputError({
      code,
      message: `refused: ${code}`,
      detail: "raw host text",
    });
    assert.equal(outcome.kind, "refused");
    assert.equal(outcome.code, code);
    assert.equal(outcome.message, `refused: ${code}`);
    assert.equal(outcome.detail, "raw host text");
  }
});

test("a contract error wrapped by invokeTauri is still read as a refusal", () => {
  const payload = { code: "dirty_tree", message: "tree is dirty", changes: 3 };
  const outcome = decodeCodingSessionAssignmentInputError(
    new TauriInvokeError("tree is dirty", payload),
  );
  assert.equal(outcome.kind, "refused");
  assert.equal(outcome.code, "dirty_tree");
  assert.equal(outcome.changes, 3);
});

test("an unregistered command is unavailable, never a refusal and never success", () => {
  const outcome = decodeCodingSessionAssignmentInputError(
    new Error("Command coding_session_establish_assignment_input not found"),
  );
  assert.equal(outcome.kind, "unavailable");
  assert.match(outcome.message, /not found/);
  assert.equal("code" in outcome, false);
});

test("an error carrying a code this build never heard of is unavailable", () => {
  const outcome = decodeCodingSessionAssignmentInputError({
    code: "wormhole_collapsed",
    message: "???",
  });
  assert.equal(outcome.kind, "unavailable");
});

test("the dirty-tree count comes from the host's field, never from its sentence", () => {
  const typed = decodeCodingSessionAssignmentInputError({
    code: "dirty_tree",
    message:
      "the seat's worktree has 7 uncommitted change(s); nothing was changed",
    detail: " M src/a.ts",
    changes: 7,
  });
  assert.equal(typed.changes, 7);
  assert.match(
    codingSessionAssignmentInputCopy(
      codingSessionAssignmentInputState("abc1234", typed),
    ).sentence,
    /7 uncommitted changes in the seat's tree; nothing was discarded\./,
  );

  // No field: the sentence is not mined for a number that was not published.
  const untyped = decodeCodingSessionAssignmentInputError({
    code: "dirty_tree",
    message:
      "the seat's worktree has 7 uncommitted change(s); nothing was changed",
  });
  assert.equal(untyped.changes, null);
  assert.equal(
    codingSessionAssignmentInputCopy(
      codingSessionAssignmentInputState("abc1234", untyped),
    ).sentence,
    "Verification input not established: Uncommitted changes in the seat's tree; nothing was discarded.",
  );
});

test("a record reads back as the state its outcome word names", () => {
  const base = {
    assignmentId: "a".repeat(64),
    sessionRef: null,
    seatLabel: "Vera",
    commit: "d698c7773",
    branch: "main",
    path: "/tree",
    remote: "origin",
    message: null,
    changes: null,
    recordedAt: 1,
  };
  assert.equal(
    codingSessionAssignmentInputStateFromRecord({
      ...base,
      outcome: "established",
    }).kind,
    "established",
  );
  assert.equal(
    codingSessionAssignmentInputStateFromRecord({
      ...base,
      outcome: "already_current",
    }).result.alreadyCurrent,
    true,
  );
  const refused = codingSessionAssignmentInputStateFromRecord({
    ...base,
    outcome: "dirty_tree",
    changes: 3,
  });
  assert.equal(refused.kind, "refused");
  assert.equal(refused.changes, 3);
  // A word this build does not know is named, not guessed from the filled path.
  assert.equal(
    codingSessionAssignmentInputStateFromRecord({
      ...base,
      outcome: "moonbeam",
    }).kind,
    "unavailable",
  );
});

test("a recorded answer keeps its sentence and says it is a record", () => {
  const copy = codingSessionAssignmentInputCopy({
    kind: "recorded",
    commit: "d698c7773",
    inner: {
      kind: "established",
      commit: "d698c7773",
      result: {
        path: "/tree",
        branch: "main",
        commit: "d698c7773",
        remote: "origin",
        alreadyCurrent: false,
      },
    },
    record: {},
  });
  assert.equal(
    copy.sentence,
    "Verification input established: d698c77 in /tree. " +
      CODING_SESSION_ASSIGNMENT_INPUT_NOT_ORDERED +
      " " +
      CODING_SESSION_ASSIGNMENT_INPUT_RECORDED,
  );
  assert.equal(copy.badge, "recorded: established");
  assert.equal(copy.retryable, true);
});

test("a record for another commit is stated as no attempt for this one", () => {
  const copy = codingSessionAssignmentInputCopy({
    kind: "stale-record",
    commit: "d698c7773",
    recordedCommit: "ecd4336f4",
  });
  assert.equal(
    copy.sentence,
    "No attempt is recorded for this commit: this computer's last recorded attempt was for ecd4336, not d698c77.",
  );
  assert.equal(copy.retryable, true);
});

test("only verifier and runner are input-bound roles", () => {
  assert.equal(isCodingSessionInputBoundRole("verifier"), true);
  assert.equal(isCodingSessionInputBoundRole("Runner"), true);
  assert.equal(isCodingSessionInputBoundRole("builder"), false);
  assert.equal(isCodingSessionInputBoundRole(null), false);
});

test("the established sentence names the commit, the tree, and the ordering", () => {
  const copy = codingSessionAssignmentInputCopy(
    codingSessionAssignmentInputState("d698c7773aaa", {
      kind: "established",
      result: {
        path: "/Users/x/Code/repo-verifier-1",
        branch: "work/verify",
        commit: "d698c7773aaa",
        remote: "origin",
        alreadyCurrent: false,
      },
    }),
  );
  assert.equal(
    copy.sentence,
    "Verification input established: d698c77 in /Users/x/Code/repo-verifier-1. " +
      CODING_SESSION_ASSIGNMENT_INPUT_NOT_ORDERED,
  );
  assert.equal(copy.detail, null);
});

test("an unavailable build says so and keeps the host text out of the headline", () => {
  const copy = codingSessionAssignmentInputCopy(
    codingSessionAssignmentInputState("d698c777", {
      kind: "unavailable",
      message: "Command not found",
      detail: null,
    }),
  );
  assert.match(
    copy.sentence,
    /^Verification input not established: this build/,
  );
  assert.equal(copy.detail, "Command not found");
});

test("a dirty tree names its count and says nothing was discarded", () => {
  const one = codingSessionAssignmentInputCopy({
    kind: "refused",
    commit: "abc1234",
    code: "dirty_tree",
    message: "dirty",
    detail: null,
    changes: 1,
  });
  assert.equal(
    one.sentence,
    "Verification input not established: 1 uncommitted change in the seat's tree; nothing was discarded.",
  );
  const many = codingSessionAssignmentInputCopy({
    kind: "refused",
    commit: "abc1234",
    code: "dirty_tree",
    message: "dirty",
    detail: null,
    changes: 4,
  });
  assert.match(many.sentence, /4 uncommitted changes in the seat's tree/);
});

test("an assignment that names no commit is its own state, with no retry", () => {
  const copy = codingSessionAssignmentInputCopy({ kind: "unnamed" });
  assert.equal(
    copy.sentence,
    "No verification input named: this assignment does not name a commit to verify, so nothing was established.",
  );
  assert.equal(copy.retryable, false);
});
