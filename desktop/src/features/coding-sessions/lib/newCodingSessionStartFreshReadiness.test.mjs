import assert from "node:assert/strict";
import test from "node:test";

import {
  canStartFreshNewCodingSessionCreate,
  newCodingSessionStartFreshReadiness,
} from "./newCodingSessionModel.ts";

const failed = {
  isPublishing: false,
  lifecycleIsLoading: false,
  lifecycleErrorMessage: null,
  lifecycleState: "failed",
  publishState: "published",
};

test("discard waits for receipt history even when a live failure is already visible", () => {
  // Frozen browser probe: the failed receipt arrived before initial history
  // settled. An enabled Discard accepted a click its handler could not honor.
  const reading = { ...failed, lifecycleIsLoading: true };
  assert.deepEqual(newCodingSessionStartFreshReadiness(reading), {
    allowed: false,
    reason:
      "Reading the provider's receipt history before discarding this attempt…",
  });
  assert.equal(canStartFreshNewCodingSessionCreate(reading), false);
  assert.deepEqual(newCodingSessionStartFreshReadiness(failed), {
    allowed: true,
    reason: null,
  });
  assert.equal(canStartFreshNewCodingSessionCreate(failed), true);
});

test("discard explains failed history and remains unavailable while publishing", () => {
  assert.deepEqual(
    newCodingSessionStartFreshReadiness({
      ...failed,
      lifecycleErrorMessage: "query unavailable",
    }),
    {
      allowed: false,
      reason:
        "The provider's receipt history could not be read. This attempt is retained until its outcome can be checked.",
    },
  );
  assert.deepEqual(
    newCodingSessionStartFreshReadiness({
      ...failed,
      isPublishing: true,
    }),
    {
      allowed: false,
      reason: "Publishing this request. Wait before discarding the attempt.",
    },
  );
});

test("discard availability preserves exact-retry safety for uncertain and successful creates", () => {
  for (const publishState of ["publishing", "published", "ambiguous"]) {
    for (const lifecycleState of [
      null,
      "pending",
      "created",
      "metadata",
      "conflict",
    ]) {
      const input = { ...failed, publishState, lifecycleState };
      const readiness = newCodingSessionStartFreshReadiness(input);
      assert.equal(
        readiness.allowed,
        false,
        `${publishState}/${lifecycleState}`,
      );
      assert.ok(readiness.reason);
      assert.equal(
        readiness.allowed,
        canStartFreshNewCodingSessionCreate(input),
      );
    }
  }
  // An unsent prepared draft can still be discarded after the read settles.
  assert.deepEqual(
    newCodingSessionStartFreshReadiness({
      ...failed,
      publishState: "prepared",
      lifecycleState: "pending",
    }),
    { allowed: true, reason: null },
  );
});
