import assert from "node:assert/strict";
import test from "node:test";

import {
  codingSessionGoalRejectionSentence,
  selectCodingSessionUmbrellaGoal,
  isCodingSessionPrivateContextMarker,
  codingSessionPrivateContextLine,
} from "./codingSessionMissionInspectorModel.ts";

// Live run 3, 12:36: the goal the launch form published, on the wire, while the
// Inspector's Current goal card read "No accepted mission goal published".
const CHANNEL = "c0066ddd-8214-4baf-81d2-3046fead0d32";
const SESSION = "48791133-ab06-4a38-8de5-d6a37c6b9d81";
const FOUNDER = "3d3b7169".repeat(8);
const STRANGER = "22".repeat(32);

function goal(overrides = {}) {
  return {
    channelId: CHANNEL,
    content:
      "Verify branch `whoami/cli` at e532192c on origin against main before it is merged.",
    createdAt: 1_788_366_485,
    eventId: "f29f23e8".repeat(8),
    founderPubkey: FOUNDER,
    sessionRef: SESSION,
    ...overrides,
  };
}

test("L2.5: the launch-published 44227 is the umbrella's goal", () => {
  const selected = selectCodingSessionUmbrellaGoal({
    goals: [goal()],
    channelId: CHANNEL,
    sessionRef: SESSION,
    founderPubkey: FOUNDER,
  });
  assert.equal(selected.kind, "available");
  assert.equal(selected.goal.eventId, goal().eventId);
  assert.deepEqual(selected.foreignAuthors, []);
});

test("L2.5: a founder key spelled in another case still matches its own goal", () => {
  // The one join that produced the live defect class: a map keyed on the
  // lowercase signer, looked up with whatever case the umbrella record holds.
  const selected = selectCodingSessionUmbrellaGoal({
    goals: [goal()],
    channelId: CHANNEL.toUpperCase(),
    sessionRef: SESSION.toUpperCase(),
    founderPubkey: FOUNDER.toUpperCase(),
  });
  assert.equal(selected.kind, "available");
});

test("L2.5: the newest goal wins, ties broken by event id", () => {
  const older = goal({ createdAt: 1, eventId: "a".repeat(64) });
  const newer = goal({
    createdAt: 2,
    eventId: "b".repeat(64),
    content: "newer",
  });
  const selected = selectCodingSessionUmbrellaGoal({
    goals: [newer, older],
    channelId: CHANNEL,
    sessionRef: SESSION,
    founderPubkey: FOUNDER,
  });
  assert.equal(selected.goal.content, "newer");
});

test("L2.5: a goal signed by anyone else is disclosed, never shown as the goal", () => {
  const selected = selectCodingSessionUmbrellaGoal({
    goals: [goal({ founderPubkey: STRANGER, eventId: "c".repeat(64) })],
    channelId: CHANNEL,
    sessionRef: SESSION,
    founderPubkey: FOUNDER,
  });
  assert.equal(selected.kind, "rejected");
  assert.deepEqual(selected.disagreements, ["founder"]);
  assert.deepEqual(selected.foreignAuthors, [STRANGER]);
});

test("L2.5: a goal for another session names `session`, not `founder`", () => {
  const selected = selectCodingSessionUmbrellaGoal({
    goals: [goal({ sessionRef: "11111111-2222-3333-4444-555555555555" })],
    channelId: CHANNEL,
    sessionRef: SESSION,
    founderPubkey: FOUNDER,
  });
  assert.equal(selected.kind, "rejected");
  assert.deepEqual(selected.disagreements, ["session"]);
  assert.match(
    codingSessionGoalRejectionSentence(selected.disagreements),
    /names a different session\. This surface will not show a goal it cannot bind to this mission\./,
  );
});

test("L2.5: no goal at all is absent, which is not the same as rejected", () => {
  const selected = selectCodingSessionUmbrellaGoal({
    goals: [],
    channelId: CHANNEL,
    sessionRef: SESSION,
    founderPubkey: FOUNDER,
  });
  assert.equal(selected.kind, "absent");
  assert.deepEqual(selected.foreignAuthors, []);
});

test("L2.5: an umbrella with no session ref or founder selects nothing", () => {
  assert.equal(
    selectCodingSessionUmbrellaGoal({
      goals: [goal()],
      channelId: CHANNEL,
      sessionRef: null,
      founderPubkey: FOUNDER,
    }).kind,
    "absent",
  );
});

test("L2.6: the elision marker is recognised, never printed as a path", () => {
  const marker = `[elided private context: 183 bytes, sha256:${"ab".repeat(32)}]`;
  assert.equal(isCodingSessionPrivateContextMarker(marker), true);
  assert.equal(
    codingSessionPrivateContextLine(3),
    "3 file edits · paths private to the seat's host",
  );
  assert.equal(
    codingSessionPrivateContextLine(1),
    "1 file edit · paths private to the seat's host",
  );
  assert.equal(
    codingSessionPrivateContextLine(null),
    "File paths private to the seat's host",
  );
});

test("L2.6: an ordinary path is not a marker", () => {
  for (const value of [
    "desktop/src/features/coding-sessions/ui/CodingSessionUmbrellaWorkspace.tsx",
    // Critique A2's own near-miss: a path that merely contains the word.
    "src/elided.rs",
    "[elided private context: 183 bytes]",
    `[elided private context: many bytes, sha256:${"ab".repeat(32)}]`,
    "[elided private context: 183 bytes, sha256:nothex]",
    "",
  ]) {
    assert.equal(isCodingSessionPrivateContextMarker(value), false, value);
  }
});
