import assert from "node:assert/strict";
import test from "node:test";

import { buildCodingSessionTeamFoldRequest } from "./invokeCodingSessionTeamFold.ts";
import { projectCodingSessionMissionEvidence } from "./codingSessionMissionEvidenceModel.ts";

const CHANNEL = "e0d3f1b8-8c66-4c62-9ef1-3fa933b32f86";
const SESSION = "5b7e1c2a-90d4-4b0e-a1f3-7c2d8e6f4a10";
const GENESIS = "ab".repeat(32);
const FOUNDER = "cd".repeat(32);
const RELAY = "ef".repeat(32);

const AUTHORITY = {
  channelRef: CHANNEL,
  genesisRef: GENESIS,
  founderPubkey: FOUNDER,
  headEventId: null,
  headSeq: 0,
  acceptedEventIds: [],
  activeSeats: [],
  activeGrants: [],
  policyGrants: [],
};

// ── The request that actually crosses the Tauri boundary ────────────────────
//
// REVIEW-L8 F5: fix round 1 asserted on an object handed to an *injected*
// fold, and was green while `invokeCodingSessionTeamFold` dropped the key on
// the floor. These assert the real production request builder — the same
// function `invokeCodingSessionTeamFold` calls — so a boundary that stops
// carrying the field turns them red.

test("L8.3: a policy that requires a verifier puts `true` in the native request", () => {
  const request = buildCodingSessionTeamFoldRequest({
    channelRef: CHANNEL,
    sessionRef: SESSION,
    genesisRef: GENESIS,
    authority: AUTHORITY,
    inputEventIds: [],
    events: [],
    verifierRequired: true,
  });
  assert.equal(request.context.verifierRequired, true);
});

test("L8.3: a policy that sets no verifier requirement puts `false` in it", () => {
  const request = buildCodingSessionTeamFoldRequest({
    channelRef: CHANNEL,
    sessionRef: SESSION,
    genesisRef: GENESIS,
    authority: AUTHORITY,
    inputEventIds: [],
    events: [],
    verifierRequired: false,
  });
  assert.equal(request.context.verifierRequired, false);
});

test("L8.3: no policy record crosses as `false`, which is what the fold requires", () => {
  // The native field is required (L7): a caller that has not read the policy
  // set says `false` on purpose, and the fold then behaves exactly as it did
  // before the field existed. The difference between "no record reached us"
  // and "the policy says no" is kept where a person can read it — the state
  // panel's own sentence — not smuggled through the fold as a third value.
  const request = buildCodingSessionTeamFoldRequest({
    channelRef: CHANNEL,
    sessionRef: SESSION,
    genesisRef: GENESIS,
    authority: AUTHORITY,
    inputEventIds: [],
    events: [],
  });
  assert.equal(request.context.verifierRequired, false);
});

test("L8.3: the request still carries the exact scope and authority it always did", () => {
  const request = buildCodingSessionTeamFoldRequest({
    channelRef: CHANNEL,
    sessionRef: SESSION,
    genesisRef: GENESIS,
    authority: AUTHORITY,
    inputEventIds: ["11".repeat(32)],
    events: [],
    verifierRequired: true,
  });
  assert.equal(request.schema, "buzz-coding-session-team-fold-request/v1");
  assert.equal(request.context.channelRef, CHANNEL);
  assert.equal(request.context.sessionRef, SESSION);
  assert.equal(request.context.genesisRef, GENESIS);
  assert.equal(request.context.founderPubkey, FOUNDER);
  assert.equal(request.context.authorityHeadSeq, 0);
  assert.deepEqual([...request.inputEventIds], ["11".repeat(32)]);
});

// ── The call site that supplies it ──────────────────────────────────────────

/**
 * Record the fold input and stop.
 *
 * The native wrapper stamps its own responses and
 * `requireIssuedNativeCodingSessionTeamFold` refuses anything else, so a test
 * cannot hand back a fake fold. What this asserts is only that the evidence
 * layer passes the caller's value down unchanged; what the *boundary* does
 * with it is asserted above, against the real builder.
 */
class StopAfterRequest extends Error {}

function snapshot() {
  return {
    transactions: [],
    transitions: [],
    receipts: [],
    rejected: [],
    rejectedTotal: 0,
    rejectedOmitted: 0,
    rejectionsTruncated: false,
    overflowed: false,
  };
}

async function foldInputFor(verifierRequired) {
  const seen = [];
  await assert.rejects(
    projectCodingSessionMissionEvidence({
      scope: {
        channelRef: CHANNEL,
        sessionRef: SESSION,
        genesisRef: GENESIS,
        founderPubkey: FOUNDER,
      },
      relayPubkey: RELAY,
      snapshot: snapshot(),
      ...(verifierRequired === undefined ? {} : { verifierRequired }),
      foldTransactions: async (input) => {
        seen.push(input);
        throw new StopAfterRequest("stop");
      },
    }),
    StopAfterRequest,
  );
  assert.equal(seen.length, 1, "the fold is invoked exactly once");
  return seen[0];
}

test("L8.3: the evidence layer hands the fold the caller's own value", async () => {
  assert.equal((await foldInputFor(true)).verifierRequired, true);
  assert.equal((await foldInputFor(false)).verifierRequired, false);
  // A caller that read no policy folds as `false` — what the native field
  // requires of a caller that cannot answer. Whether a record was read at all
  // is a separate fact, and the state panel says it in words.
  assert.equal((await foldInputFor(undefined)).verifierRequired, false);
  assert.equal((await foldInputFor(null)).verifierRequired, false);
});
