import assert from "node:assert/strict";
import test from "node:test";

import {
  CODING_SESSION_READDRESS_LABEL,
  CODING_SESSION_TURN_DROPPED_MESSAGE,
  CODING_SESSION_TURN_REFUSED_MESSAGE,
  forgetCodingSessionTurn,
  formatCodingSessionTurnRefusal,
  isCodingSessionReaddressableRefusal,
  MAX_WATCHED_CODING_SESSION_TURNS,
  resolveCodingSessionReaddress,
  restoreCodingSessionDraft,
  watchCodingSessionTurn,
} from "./codingSessionTurnRefusal.ts";
import { MAX_PENDING_CODING_SESSION_TURNS_PER_TARGET } from "./codingSessionPendingTurns.ts";

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
    // D9: the crew allowance refusal is a code like any other — read from the
    // receipt, never translated into a guess about permissions.
    "BUDGET_EXHAUSTED",
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

test("a spent crew allowance reaches the pending row with its two numbers", () => {
  assert.equal(
    formatCodingSessionTurnRefusal({
      code: "BUDGET_EXHAUSTED",
      message:
        'this team session has started 200 of its 200 allowed turns; the session founder can still send turns, and raising "Turns per team session" takes effect the next time the provider starts',
      outcome: "refused",
    }),
    'Turn refused (BUDGET_EXHAUSTED): this team session has started 200 of its 200 allowed turns; the session founder can still send turns, and raising "Turns per team session" takes effect the next time the provider starts',
  );
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
    Array.from(
      { length: MAX_WATCHED_CODING_SESSION_TURNS },
      (_unused, index) => `csc-${index + 2}`,
    ),
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
    watched
      .map((turn) => turn.commandId)
      .filter((commandId) => commandId !== "csc-3"),
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

test("every pending row a person can hold has a watch to retire it", () => {
  // A row the provider has signed for is exempt from the pending TTL, and its
  // watch is exempt from the refusal deadline. So a row whose watch was
  // evicted has nothing left that can retire it: not the TTL, not the
  // deadline, and not the `turn_dropped` receipt, which now arrives to a
  // closed subscription. The watch set must therefore be at least as large as
  // the number of rows that can exist.
  assert.ok(
    MAX_WATCHED_CODING_SESSION_TURNS >=
      MAX_PENDING_CODING_SESSION_TURNS_PER_TARGET,
    `watches (${MAX_WATCHED_CODING_SESSION_TURNS}) must cover every pending row (${MAX_PENDING_CODING_SESSION_TURNS_PER_TARGET})`,
  );

  let watched = [];
  for (
    let index = 0;
    index < MAX_PENDING_CODING_SESSION_TURNS_PER_TARGET;
    index += 1
  ) {
    watched = watchCodingSessionTurn(watched, {
      commandId: `csc-${index}`,
      draft: `draft ${index}`,
    });
  }
  assert.equal(watched.length, MAX_PENDING_CODING_SESSION_TURNS_PER_TARGET);
  assert.ok(
    watched.some((turn) => turn.commandId === "csc-0"),
    "the first turn a person sent is still watched when the last one lands",
  );
});

test("only the two owed-turn codes offer to be re-addressed", () => {
  for (const code of ["NO_LIVE_EXECUTION", "STALE_GENERATION"]) {
    assert.equal(
      isCodingSessionReaddressableRefusal({
        code,
        message: "the generation this turn addressed is gone",
      }),
      true,
      `${code} is the receipt that says the words never ran`,
    );
  }
  // Everything else is a decision about the sender or the provider's own
  // queue: resending the same words to a newer generation would not change
  // the answer, and offering it would be a lie about what happened.
  for (const code of [
    "UNAUTHORIZED_OPERATOR",
    "UNKNOWN_TARGET",
    "SESSION_CLOSED",
    "QUEUE_FULL",
    "QUEUE_FULL_TURN_KEPT",
    // A newer generation of the same crew session shares the same spent
    // allowance, so a resend would be refused again for the same reason.
    "BUDGET_EXHAUSTED",
    "",
  ]) {
    assert.equal(
      isCodingSessionReaddressableRefusal({ code, message: "no" }),
      false,
      `${code} must not offer a resend`,
    );
  }
});

test("a resend is offered only when a later generation exists to resend into", () => {
  // Ruling R1: the sender re-addresses an owed turn, and the generation it
  // resolves to is the execution's *current* one — never the one that refused.
  assert.deepEqual(
    resolveCodingSessionReaddress({
      refusedGeneration: 1,
      currentGeneration: 2,
      isEnded: false,
    }),
    {
      kind: "offer",
      generation: 2,
      label: CODING_SESSION_READDRESS_LABEL,
    },
  );
});

test("nothing resumed means no offer, and the row says why", () => {
  const same = resolveCodingSessionReaddress({
    refusedGeneration: 3,
    currentGeneration: 3,
    isEnded: false,
  });
  assert.equal(same.kind, "unavailable");
  assert.match(same.reason, /resumed/i);
  // A generation cannot go backwards, but a stale prop must not be read as a
  // resume either.
  assert.equal(
    resolveCodingSessionReaddress({
      refusedGeneration: 4,
      currentGeneration: 3,
      isEnded: false,
    }).kind,
    "unavailable",
  );
});

test("an ended execution is never offered a resend", () => {
  const ended = resolveCodingSessionReaddress({
    refusedGeneration: 1,
    currentGeneration: 2,
    isEnded: true,
  });
  assert.equal(ended.kind, "unavailable");
  assert.match(ended.reason, /ended/i);
});

test("a turn whose generation this client forgot is not guessed at", () => {
  const unknown = resolveCodingSessionReaddress({
    refusedGeneration: undefined,
    currentGeneration: 9,
    isEnded: false,
  });
  assert.equal(unknown.kind, "unavailable");
  assert.match(unknown.reason, /which generation/i);
});
