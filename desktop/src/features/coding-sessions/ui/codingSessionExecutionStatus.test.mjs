/**
 * The rail's status vocabulary must cover every status the provider signs.
 * A status the map forgets renders "status unknown" beside a header that
 * reads idle — three surfaces disagreeing about one signed fact.
 *
 * The rail no longer owns a vocabulary of its own: it prints W1's word for
 * the resolved status. So the coverage this file has always asserted is now
 * asserted against that one pipeline (signed status → W1 → word), which is
 * the point — a second vocabulary here is what let the rail paint `Working`
 * over a provider nobody was answering for (WALK-2026-08-29 finding 1).
 */
import assert from "node:assert/strict";
import test from "node:test";

import { codingSessionDispositionWord } from "../lib/codingSessionUmbrellaModel.ts";
import { codingSessionWireWorkspaceStatus } from "../lib/codingSessionWorkspaceModel.ts";
import { executionStatusTone } from "./CodingSessionExecutionRail.tsx";

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

function word(status, canSteer = false) {
  return codingSessionDispositionWord(
    codingSessionWireWorkspaceStatus(status),
    canSteer,
  );
}

test("every signed status maps to a word, and only `unknown` is unknown", () => {
  for (const status of SIGNED_STATUSES) {
    const label = word(status);
    if (status === "unknown") {
      assert.equal(label, "status unknown");
    } else {
      assert.notEqual(
        label,
        "status unknown",
        `${status} fell through to "status unknown"`,
      );
    }
  }
});

test("idle reads idle — the regression two machines showed at once", () => {
  assert.equal(word("idle"), "idle");
  assert.equal(word("running"), "live");
  assert.equal(word("starting"), "live");
  assert.equal(word("waiting_for_input", true), "waiting for you");
  assert.equal(word("disconnected"), "disconnected");
  assert.equal(word("failed"), "needs attention");
  assert.equal(word("stopped"), "released");
  assert.equal(word("completed"), "idle");
});

test("tone says how loudly to print the word, never which word it is", () => {
  assert.equal(
    executionStatusTone(codingSessionWireWorkspaceStatus("running")),
    "text-blue-500",
  );
  assert.equal(
    executionStatusTone(codingSessionWireWorkspaceStatus("waiting_for_input")),
    "text-amber-500",
  );
  assert.equal(
    executionStatusTone(codingSessionWireWorkspaceStatus("idle")),
    "text-muted-foreground",
  );
  // A signed lifecycle put this one here, so it is attention-worthy; a merely
  // unread status is not.
  assert.equal(
    executionStatusTone(codingSessionWireWorkspaceStatus("disconnected")),
    "text-destructive",
  );
  assert.equal(
    executionStatusTone(codingSessionWireWorkspaceStatus("unknown")),
    "text-muted-foreground",
  );
});
