import assert from "node:assert/strict";
import test from "node:test";

import {
  CODING_SESSION_TURN_DROPPED_MESSAGE,
  CODING_SESSION_TURN_REFUSED_MESSAGE,
  forgetCodingSessionTurn,
  formatCodingSessionTurnRefusal,
  MAX_WATCHED_CODING_SESSION_TURNS,
  restoreCodingSessionDraft,
  watchCodingSessionTurn,
} from "./codingSessionTurnRefusal.ts";

test("a refusal is named in the provider's own words", () => {
  assert.equal(
    formatCodingSessionTurnRefusal({
      code: "UNAUTHORIZED_OPERATOR",
      message:
        "only the session founder or a granted operator may steer this execution",
    }),
    "Turn refused (UNAUTHORIZED_OPERATOR): only the session founder or a granted operator may steer this execution",
  );
  // A receipt with no readable message still has to say that the turn was
  // refused rather than trail off.
  assert.equal(
    formatCodingSessionTurnRefusal({
      code: "UNAUTHORIZED_OPERATOR",
      message: "   ",
    }),
    `Turn refused (UNAUTHORIZED_OPERATOR): ${CODING_SESSION_TURN_REFUSED_MESSAGE}`,
  );
});

test("the new refusal codes each reach the composer verbatim", () => {
  for (const code of [
    "UNAUTHORIZED_OPERATOR",
    "UNKNOWN_TARGET",
    "STALE_GENERATION",
    "SESSION_CLOSED",
  ]) {
    assert.equal(
      formatCodingSessionTurnRefusal({
        code,
        message: "the provider said why",
        outcome: "refused",
      }),
      `Turn refused (${code}): the provider said why`,
    );
  }
});

test("a dropped turn is not called a refusal", () => {
  // Nobody was refused: the provider accepted the turn and then its own queue
  // overflowed. Saying "refused" would send the person looking for a
  // permission they already have.
  assert.equal(
    formatCodingSessionTurnRefusal({
      code: "QUEUE_FULL",
      message: "the session queue is full",
      outcome: "dropped",
    }),
    "Turn dropped (QUEUE_FULL): the session queue is full",
  );
  assert.equal(
    formatCodingSessionTurnRefusal({
      code: "QUEUE_FULL",
      message: " ",
      outcome: "dropped",
    }),
    `Turn dropped (QUEUE_FULL): ${CODING_SESSION_TURN_DROPPED_MESSAGE}`,
  );
});

test("watches are bounded, newest kept, and never doubled", () => {
  let watched = [];
  for (
    let index = 0;
    index < MAX_WATCHED_CODING_SESSION_TURNS + 2;
    index += 1
  ) {
    watched = watchCodingSessionTurn(watched, {
      commandId: `csc-${index}`,
      draft: `draft ${index}`,
    });
  }
  assert.equal(watched.length, MAX_WATCHED_CODING_SESSION_TURNS);
  assert.deepEqual(
    watched.map((turn) => turn.commandId),
    ["csc-2", "csc-3", "csc-4", "csc-5"],
  );

  // Re-arming the same command replaces its watch instead of holding two.
  const rearmed = watchCodingSessionTurn(watched, {
    commandId: "csc-3",
    draft: "edited",
  });
  assert.equal(rearmed.filter((turn) => turn.commandId === "csc-3").length, 1);
  assert.equal(rearmed.at(-1).draft, "edited");

  assert.deepEqual(
    forgetCodingSessionTurn(rearmed, "csc-3").map((turn) => turn.commandId),
    ["csc-2", "csc-4", "csc-5"],
  );
});

test("restoring refused words never costs the person a newer draft", () => {
  assert.equal(restoreCodingSessionDraft("", "refused words"), "refused words");
  assert.equal(
    restoreCodingSessionDraft("   ", "refused words"),
    "refused words",
  );
  assert.equal(
    restoreCodingSessionDraft("new thought", "refused words"),
    "refused words\n\nnew thought",
  );
  // A replayed refusal is the same fact, not a second copy of the message.
  assert.equal(
    restoreCodingSessionDraft("refused words\n\nnew", "refused words"),
    "refused words\n\nnew",
  );
  assert.equal(restoreCodingSessionDraft("kept", "  "), "kept");
});
