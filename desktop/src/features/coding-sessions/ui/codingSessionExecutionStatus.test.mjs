/**
 * The rail's status vocabulary must cover every status the provider signs.
 * A status the map forgets renders "Status unknown" beside a header that
 * reads Idle — three surfaces disagreeing about one signed fact.
 */
import assert from "node:assert/strict";
import test from "node:test";

import { executionStatus } from "./CodingSessionExecutionRail.tsx";

// Exactly the vocabulary in crates/buzz-core/src/coding_session_payload.rs.
const SIGNED_STATUSES = [
  "starting",
  "idle",
  "running",
  "waiting_for_input",
  "completed",
  "stopped",
  "failed",
  "interrupted",
  "disconnected",
  "unknown",
];

test("every signed status maps to a label, and only `unknown` is unknown", () => {
  for (const status of SIGNED_STATUSES) {
    const { label } = executionStatus(status);
    if (status === "unknown") {
      assert.equal(label, "Status unknown");
    } else {
      assert.notEqual(
        label,
        "Status unknown",
        `${status} fell through to "Status unknown"`,
      );
    }
  }
});

test("idle reads Idle — the regression two machines showed at once", () => {
  assert.equal(executionStatus("idle").label, "Idle");
  assert.equal(executionStatus("running").label, "Working");
  assert.equal(executionStatus("waiting_for_input").label, "Waiting");
  assert.equal(executionStatus("disconnected").label, "Needs attention");
});
