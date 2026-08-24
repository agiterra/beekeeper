import assert from "node:assert/strict";
import test from "node:test";

import {
  CODING_SESSION_CAPACITY_MAX,
  codingSessionCapacityChoice,
  codingSessionCapacityLabel,
  codingSessionCapacityPending,
  codingSessionCapacityValue,
  parseCodingSessionCapacityInput,
} from "./codingSessionCapacity.ts";

test("the three states round-trip", () => {
  for (const [stored, kind] of [
    [null, "default"],
    [0, "unlimited"],
    [6, "limit"],
  ]) {
    const choice = codingSessionCapacityChoice(stored);
    assert.equal(choice.kind, kind);
    assert.equal(codingSessionCapacityValue(choice), stored);
  }
});

test("clearing the field never silently means unlimited", () => {
  // 0 is unlimited only when chosen deliberately.
  assert.equal(parseCodingSessionCapacityInput("", 4), 4);
  assert.equal(parseCodingSessionCapacityInput("   ", 4), 4);
  assert.equal(parseCodingSessionCapacityInput("nonsense", 4), 4);
  assert.equal(parseCodingSessionCapacityInput("0", 4), 4);
  assert.equal(parseCodingSessionCapacityInput("-3", 4), 4);
  assert.equal(parseCodingSessionCapacityInput("7", 4), 7);
  assert.equal(
    parseCodingSessionCapacityInput("9000", 4),
    CODING_SESSION_CAPACITY_MAX,
    "a typo is bounded rather than obeyed",
  );
});

test("the label names the default's number rather than the word", () => {
  assert.equal(codingSessionCapacityLabel(null, 4), "4 sessions");
  assert.equal(codingSessionCapacityLabel(1, 4), "1 session");
  assert.equal(codingSessionCapacityLabel(0, 4), "Unlimited");
});

// A saved ceiling reaches the child only at its next start (§2 item 41's rule:
// never show a setting as in force when it is not).
test("a change that is not yet in force says so, and one that is stays quiet", () => {
  assert.equal(
    codingSessionCapacityPending({
      maxSessions: 8,
      defaultMaxSessions: 4,
      runningMaxSessions: null,
      providerRunning: true,
    }),
    "The provider running now started with 4 sessions. Your change applies the next time it starts.",
  );
  assert.equal(
    codingSessionCapacityPending({
      maxSessions: 8,
      defaultMaxSessions: 4,
      runningMaxSessions: 8,
      providerRunning: true,
    }),
    null,
  );
  // Nothing is running, so nothing is being enforced to contradict.
  assert.equal(
    codingSessionCapacityPending({
      maxSessions: 8,
      defaultMaxSessions: 4,
      runningMaxSessions: null,
      providerRunning: false,
    }),
    null,
  );
});
