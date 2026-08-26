/**
 * Exact-key discipline for the 44224 receipt decoder.
 *
 * A receipt is a signed claim about a turn this client sent, and the surfaces
 * that read one act on it — a row is relabelled, a draft comes back, a queued
 * turn stops claiming it started. So an object that is *nearly* a receipt is a
 * rejection, never a partial accept: one unexpected key means the producer and
 * this decoder disagree about the contract, and guessing which half is right
 * is how a wrong sentence ends up on screen under a signature.
 */
import assert from "node:assert/strict";
import test from "node:test";

import {
  CODING_SESSION_TURN_RECEIPT_STATUSES,
  codingSessionReceiptSemanticKey,
  isCodingSessionTurnReceiptStatus,
  parseCodingSessionLifecycleReceipt,
} from "./codingSessionIngressPayloads.ts";

const SESSION = {
  driver: "claude-agent-acp",
  instanceId: "instance-1",
  sessionId: "session-1",
  generation: 2,
};

function receipt(overrides) {
  return JSON.stringify({
    schema: "buzz-coding-session-lifecycle-receipt/v1",
    commandId: "csc-1",
    session: SESSION,
    error: null,
    ...overrides,
  });
}

test("a steer the provider could not honour decodes as a five-key turn_degraded", () => {
  const parsed = parseCodingSessionLifecycleReceipt(
    receipt({
      status: "turn_degraded",
      error: {
        code: "STEER_UNSUPPORTED",
        message: "This runtime advertised no native steering.",
      },
    }),
  );
  assert.ok(parsed, "a well-formed turn_degraded must decode");
  assert.equal(parsed.status, "turn_degraded");
  assert.deepEqual(parsed.session, SESSION);
  assert.equal(parsed.error.code, "STEER_UNSUPPORTED");
  assert.deepEqual(Object.keys(parsed), [
    "schema",
    "commandId",
    "status",
    "session",
    "error",
  ]);
});

test("a six-key turn_degraded is rejected outright", () => {
  assert.equal(
    parseCodingSessionLifecycleReceipt(
      receipt({
        status: "turn_degraded",
        error: { code: "STEER_UNSUPPORTED", message: "no native steering" },
        turnId: "turn-9",
      }),
    ),
    null,
    "turnId belongs to turn_started alone",
  );
  assert.equal(
    parseCodingSessionLifecycleReceipt(
      receipt({ status: "turn_degraded", error: null }),
    ),
    null,
    "a degraded turn always says why it was degraded",
  );
});

test("an issued interrupt decodes as a five-key interrupt_delivered", () => {
  const parsed = parseCodingSessionLifecycleReceipt(
    receipt({ status: "interrupt_delivered" }),
  );
  assert.ok(parsed, "a well-formed interrupt_delivered must decode");
  assert.equal(parsed.error, null);
  assert.deepEqual(parsed.session, SESSION);
  assert.deepEqual(Object.keys(parsed), [
    "schema",
    "commandId",
    "status",
    "session",
    "error",
  ]);
  assert.equal(
    parseCodingSessionLifecycleReceipt(
      receipt({
        status: "interrupt_delivered",
        error: { code: "X", message: "y" },
      }),
    ),
    null,
    "a delivered interrupt is not an error",
  );
  assert.equal(
    parseCodingSessionLifecycleReceipt(
      receipt({ status: "interrupt_delivered", turnId: "turn-9" }),
    ),
    null,
  );
});

test("a turn refusal or drop takes any bounded code the provider grows", () => {
  for (const status of ["turn_refused", "turn_dropped"]) {
    const parsed = parseCodingSessionLifecycleReceipt(
      receipt({
        status,
        error: {
          code: "NO_LIVE_EXECUTION",
          message: "No execution is running for this session.",
        },
      }),
    );
    assert.ok(parsed, `${status} must accept a code this client never saw`);
    assert.equal(parsed.error.code, "NO_LIVE_EXECUTION");
  }
});

test("both new statuses are turn statuses, so their keys name the stage", () => {
  assert.deepEqual(
    [...CODING_SESSION_TURN_RECEIPT_STATUSES],
    [
      "turn_queued",
      "turn_started",
      "turn_degraded",
      "turn_dropped",
      "turn_refused",
      "interrupt_delivered",
    ],
  );
  // Literal, not a call compared to itself: the earlier form was true for any
  // implementation, including one that returned a constant.
  const expectedKeys = {
    turn_degraded:
      "coding-session-lifecycle-receipt/v1|5:csc-113:turn_degraded",
    interrupt_delivered:
      "coding-session-lifecycle-receipt/v1|5:csc-119:interrupt_delivered",
  };
  for (const status of ["turn_degraded", "interrupt_delivered"]) {
    assert.equal(isCodingSessionTurnReceiptStatus(status), true);
    assert.equal(
      codingSessionReceiptSemanticKey("csc-1", status),
      expectedKeys[status],
    );
    assert.notEqual(
      codingSessionReceiptSemanticKey("csc-1", status),
      codingSessionReceiptSemanticKey("csc-1", "turn_queued"),
      "a stage that shares a key with another is fenced out of the relay",
    );
  }
  // A lifecycle receipt keeps the historical single-field key.
  assert.equal(
    codingSessionReceiptSemanticKey("csc-1", "created"),
    "coding-session-lifecycle-receipt/v1|5:csc-1",
  );
});
