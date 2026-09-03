import assert from "node:assert/strict";
import test from "node:test";

import { codingSessionAnswerFieldRefusal } from "./useCodingSessionDecisionAnswer.ts";

const BOUNDS = {
  choiceMaxBytes: 2 * 1024,
  noteMaxBytes: 8 * 1024,
  conditionMaxBytes: 512,
};
const REQUEST = "22".repeat(32);

function draft(overrides = {}) {
  return {
    requestRef: REQUEST,
    choice: 0,
    note: null,
    condition: null,
    ...overrides,
  };
}

test("L8.1: a free-text answer over 2 KiB is refused before signing, naming the field", () => {
  const refusal = codingSessionAnswerFieldRefusal(
    draft({ choice: "x".repeat(BOUNDS.choiceMaxBytes + 1) }),
    BOUNDS,
  );
  assert.match(refusal, /choice/);
  assert.match(refusal, /nothing was signed/);
});

test("L8.4: a condition over 512 B is refused before signing, naming the field", () => {
  const refusal = codingSessionAnswerFieldRefusal(
    draft({ condition: "y".repeat(BOUNDS.conditionMaxBytes + 1) }),
    BOUNDS,
  );
  assert.match(refusal, /condition/);
  assert.match(refusal, /512 bytes/);
});

test("L8.1: an over-long note is refused, naming the note rather than the choice", () => {
  const refusal = codingSessionAnswerFieldRefusal(
    draft({ note: "z".repeat(BOUNDS.noteMaxBytes + 1) }),
    BOUNDS,
  );
  assert.match(refusal, /note/);
  assert.ok(!/choice/.test(refusal));
});

test("L8.1: the bound is bytes, not characters — a multi-byte answer counts its bytes", () => {
  // 512 U+00E9 is 1,024 bytes: a character count would have let it through.
  const refusal = codingSessionAnswerFieldRefusal(
    draft({ condition: "é".repeat(512) }),
    BOUNDS,
  );
  assert.match(refusal, /condition/);
});

test("L8.1: an answer inside every bound is not refused", () => {
  assert.equal(
    codingSessionAnswerFieldRefusal(
      draft({
        choice: "rebase first",
        note: "the gate is green",
        condition: null,
      }),
      BOUNDS,
    ),
    null,
  );
});

test("F12: a numeric choice is bounded — range and integrality, before signing", () => {
  const bounds = { ...BOUNDS, declaredOptions: 3 };
  assert.match(
    codingSessionAnswerFieldRefusal(draft({ choice: 3 }), bounds),
    /names option 4, and this request declared 3/,
  );
  assert.match(
    codingSessionAnswerFieldRefusal(draft({ choice: -1 }), bounds),
    /is not an option index/,
  );
  assert.match(
    codingSessionAnswerFieldRefusal(draft({ choice: 1.5 }), bounds),
    /is not an option index/,
  );
  assert.equal(
    codingSessionAnswerFieldRefusal(draft({ choice: 2 }), bounds),
    null,
  );
  // With no declared count in hand the range is unknown, so only integrality
  // is claimed — never a bound this surface cannot check.
  assert.equal(
    codingSessionAnswerFieldRefusal(draft({ choice: 7 }), BOUNDS),
    null,
  );
});
