import assert from "node:assert/strict";
import test from "node:test";

import { projectCodingSessionMissionState } from "./codingSessionMissionStateProjection.ts";

function record(status, statusEventId = "a".repeat(64), statusAt = 1) {
  return { status, statusEventId, statusAt };
}

test("canonical terminal truth outranks seat metadata", () => {
  const canonical = {
    kind: "completed",
    sourceEventId: "c".repeat(64),
    summary: "Done",
    landedShas: [],
    followUps: [],
    canonicalChain: [],
  };
  assert.equal(
    projectCodingSessionMissionState({
      canonical,
      seats: [{ label: "Builder", record: record("waiting_for_input") }],
    }),
    canonical,
  );
});

test("accepted nonterminal canonical truth outranks provider waiting and failure", () => {
  const canonical = {
    kind: "running",
    sourceEventId: "c".repeat(64),
    phase: "reported",
    detail:
      "The canonical fold contains an accepted report awaiting disposition.",
    canonicalChain: [
      {
        type: "report",
        sourceEventId: "c".repeat(64),
        authorPubkey: "d".repeat(64),
        createdAt: 2,
        summary: "Report accepted into the canonical fold.",
      },
    ],
  };
  for (const status of ["waiting_for_input", "failed", "disconnected"]) {
    assert.equal(
      projectCodingSessionMissionState({
        canonical,
        seats: [{ label: "Builder", record: record(status) }],
      }),
      canonical,
    );
  }
});

test("signed waiting_for_input is waiting on a person with provenance", () => {
  assert.deepEqual(
    projectCodingSessionMissionState({
      canonical: { kind: "unknown", detail: null },
      seats: [{ label: "Builder", record: record("waiting_for_input") }],
    }),
    {
      kind: "waiting-on-person",
      sourceEventId: "a".repeat(64),
      requiredAction: "Reply to Builder or provide the requested input.",
      heldOn: "Builder",
    },
  );
});

test("only explicit signed failed/disconnected states become stalled", () => {
  for (const status of ["failed", "disconnected"]) {
    assert.equal(
      projectCodingSessionMissionState({
        canonical: { kind: "unknown", detail: "No terminal." },
        seats: [{ label: "Verifier", record: record(status) }],
      }).kind,
      "stalled",
    );
  }
  assert.equal(
    projectCodingSessionMissionState({
      canonical: { kind: "unknown", detail: "No terminal." },
      seats: [{ label: "Verifier", record: record("failed", null) }],
    }).kind,
    "unknown",
  );
});

test("idle, age, and silence never become completion or stalled", () => {
  for (const status of ["idle", "running", "completed", "unknown"]) {
    assert.equal(
      projectCodingSessionMissionState({
        canonical: { kind: "unknown", detail: "No terminal." },
        seats: [{ label: "Builder", record: record(status) }],
      }).kind,
      "unknown",
    );
  }
});
