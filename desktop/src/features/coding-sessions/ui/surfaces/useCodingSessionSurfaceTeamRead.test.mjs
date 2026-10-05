import assert from "node:assert/strict";
import test from "node:test";

import {
  codingSessionFounderHoldLabel,
  codingSessionObservationFoldUnchecked,
  codingSessionSurfaceEvidenceScope,
  deriveCodingSessionSurfaceOpenRulings,
} from "./useCodingSessionSurfaceTeamRead.ts";

const FOUNDER = "f".repeat(64);
const SEAT = "a".repeat(64);
const CHANNEL = "channel-1";

function record(overrides = {}) {
  return {
    generationId: "gen-1",
    title: "Builder",
    agentRef: SEAT,
    role: "builder",
    model: null,
    runtime: null,
    commandTarget: null,
    transcript: [],
    ...overrides,
  };
}

const UMBRELLA = {
  umbrellaKey: "session-1",
  sessionRef: "session-1",
  genesisRef: "e".repeat(64),
  founderPubkey: FOUNDER,
  title: "Session",
  executions: [
    {
      executionKey: "exec-1",
      signerPubkey: "b".repeat(64),
      activeGeneration: record(),
      priorGenerations: [],
      operatorPubkey: FOUNDER,
    },
  ],
};

function transaction(overrides) {
  return {
    sourceEventId: "1".repeat(64),
    type: "assignment",
    authorPubkey: FOUNDER,
    createdAt: 1_790_000_000,
    counterpartyPubkey: SEAT,
    parentEventId: null,
    summary: "Build the drawer",
    decision: null,
    requiredAction: null,
    fileCount: null,
    testCount: null,
    unseated: false,
    ...overrides,
  };
}

test("the evidence scope takes no lens: a genesis session is read in Conversation too", () => {
  assert.deepEqual(codingSessionSurfaceEvidenceScope(CHANNEL, UMBRELLA), {
    channelRef: CHANNEL,
    sessionRef: "session-1",
    genesisRef: UMBRELLA.genesisRef,
    founderPubkey: FOUNDER,
  });
  // Only a missing genesis (or session, or founder) leaves nothing to read.
  assert.equal(
    codingSessionSurfaceEvidenceScope(CHANNEL, {
      ...UMBRELLA,
      genesisRef: null,
    }),
    null,
  );
});

test("a genesis session's open assignment becomes a non-null ruling for ctx", () => {
  const rulings = deriveCodingSessionSurfaceOpenRulings({
    umbrella: UMBRELLA,
    transactions: [transaction({})],
    resolveActorName: (pubkey) => (pubkey === SEAT ? "Ira" : null),
    founderLabel: "you",
    nowMs: 1_790_000_060_000,
  });
  assert.notEqual(rulings, null);
  assert.equal(rulings.holds.length, 1);
  assert.equal(rulings.holds[0].holding, "assignment");
  assert.equal(rulings.holds[0].waiterLabel, "you");
  assert.equal(rulings.holds[0].sinceMs, 60_000);
});

test("an answered assignment is no longer open", () => {
  const rulings = deriveCodingSessionSurfaceOpenRulings({
    umbrella: UMBRELLA,
    transactions: [
      transaction({}),
      transaction({
        sourceEventId: "2".repeat(64),
        type: "report",
        authorPubkey: SEAT,
        counterpartyPubkey: FOUNDER,
        parentEventId: "1".repeat(64),
      }),
    ],
    resolveActorName: () => null,
    nowMs: 1_790_000_060_000,
  });
  // The report itself is now the open fact (no verdict yet).
  assert.deepEqual(
    rulings.holds.map((hold) => hold.holding),
    ["report"],
  );
});

test("the founder is `you` to the founder and named to everyone else", () => {
  assert.equal(
    codingSessionFounderHoldLabel({
      currentUserPubkey: FOUNDER.toUpperCase(),
      founderPubkey: FOUNDER,
      resolveActorName: () => "Brian",
    }),
    "you",
  );
  assert.equal(
    codingSessionFounderHoldLabel({
      currentUserPubkey: SEAT,
      founderPubkey: FOUNDER,
      resolveActorName: () => "Brian",
    }),
    "Brian",
  );
});

test("an unchecked fold discloses no unresolved pointers", () => {
  const fold = {
    unresolved: [{ eventId: "x", assignmentRef: "y" }],
    truncated: { unresolved: 3, gates: 1 },
    gates: [],
  };
  const unchecked = codingSessionObservationFoldUnchecked(fold);
  assert.deepEqual(unchecked.unresolved, []);
  assert.equal(unchecked.truncated.unresolved, 0);
  assert.equal(unchecked.truncated.gates, 1);
  assert.equal(codingSessionObservationFoldUnchecked(null), null);
});
