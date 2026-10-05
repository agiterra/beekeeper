import assert from "node:assert/strict";
import { test } from "node:test";

import { foldAgentProgress } from "@/features/agent-progress/lib/agentProgressFold";

const NOW = 1_785_513_037;
const GENERATED = {
  origin: "generated",
  model: "claude-haiku-4-5",
  signerPubkey: "d4".repeat(32),
};

function session(sessionKey, name) {
  return {
    sessionKey,
    sessionRef: sessionKey,
    name,
    goal: null,
    lifecycle: "open",
    coordinationState: "open_unverified",
    latestObservationAt: NOW - 60,
    observedAgeSeconds: 60,
    generations: [],
    sourceEventIds: [],
  };
}

function lanes(sessions, nameOriginsBySession, detailBySessionRef = new Map()) {
  return foldAgentProgress({
    sessions,
    channelsBySession: new Map(sessions.map((row) => [row.sessionKey, ["c"]])),
    nameOriginsBySession,
    detailBySessionRef,
    nowSeconds: NOW,
    complete: true,
  }).lanes;
}

test("a lane labelled by a generated title carries its origin", () => {
  const [lane] = lanes(
    [session("s1", "Login redirect fix")],
    new Map([["s1", GENERATED]]),
  );
  assert.equal(lane.label, "Login redirect fix");
  assert.deepEqual(lane.labelOrigin, GENERATED);
});

test("a lane not labelled by the signed name carries no origin", () => {
  // Unnamed: the label is the local row or the fallback, never the title's.
  const [unnamed] = lanes(
    [session("s1", null)],
    new Map([["s1", GENERATED]]),
    new Map([["s1", { label: "Local label", transcript: [] }]]),
  );
  assert.equal(unnamed.labelOrigin, null);
  // Origins not read: unknown, so no marker is guessed.
  const [unknown] = lanes([session("s2", "Something")], undefined);
  assert.equal(unknown.labelOrigin, null);
});
