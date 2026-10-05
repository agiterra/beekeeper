import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import test from "node:test";

import {
  CODING_SESSION_OBSERVATION_FOLD_COMMAND,
  CODING_SESSION_OBSERVATION_FOLD_REQUEST_SCHEMA,
} from "./codingSessionObservationWire.ts";
import { invokeCodingSessionObservationFold } from "./invokeCodingSessionObservationFold.ts";

const FIXTURE = JSON.parse(
  readFileSync(
    fileURLToPath(
      new URL(
        "./codingSessionObservationFoldAdapterResponse.fixture.json",
        import.meta.url,
      ),
    ),
    "utf8",
  ),
);

const SESSION = FIXTURE.sessionRef;
const GENESIS = FIXTURE.genesisRef;

function events(createdAt = 1_756_800_000) {
  return FIXTURE.inputEventIds.map((id, index) => ({
    id,
    pubkey: "11".repeat(32),
    created_at: createdAt + index,
    kind: 44246,
    tags: [],
    content: "{}",
    sig: "22".repeat(64),
  }));
}

function invoker(response, calls) {
  return async (command, args) => {
    calls.push({ command, args });
    return structuredClone(response);
  };
}

test("the request carries the closed schema and the events it was handed", async () => {
  const calls = [];
  const result = await invokeCodingSessionObservationFold({
    sessionRef: SESSION,
    genesisRef: GENESIS,
    knownAssignmentRefs: ["cd".repeat(32)],
    providerPubkeys: null,
    events: events(),
    invoke: invoker(FIXTURE, calls),
  });

  assert.equal(calls.length, 1);
  assert.equal(calls[0].command, CODING_SESSION_OBSERVATION_FOLD_COMMAND);
  assert.equal(
    calls[0].args.request.schema,
    CODING_SESSION_OBSERVATION_FOLD_REQUEST_SCHEMA,
  );
  assert.equal(calls[0].args.request.sessionRef, SESSION);
  assert.deepEqual(calls[0].args.request.knownAssignmentRefs, [
    "cd".repeat(32),
  ]);
  assert.equal(
    calls[0].args.request.events.length,
    FIXTURE.inputEventIds.length,
  );
  assert.equal(result.fold.gates.length, 2);
});

test("the signed created_at rides beside the fold, never inside it", async () => {
  const result = await invokeCodingSessionObservationFold({
    sessionRef: SESSION,
    genesisRef: GENESIS,
    knownAssignmentRefs: [],
    providerPubkeys: null,
    events: events(1_756_800_000),
    invoke: invoker(FIXTURE, []),
  });
  // The fold itself reads no clock, so a caller that wants to *place* a row on
  // a time axis joins it here, from the events it fetched.
  assert.equal(result.signedAt.size, FIXTURE.inputEventIds.length);
  assert.equal(result.signedAt.get(FIXTURE.inputEventIds[0]), 1_756_800_000);
  assert.equal(Object.hasOwn(result.fold, "createdAt"), false);
});

test("a fold about another session is refused", async () => {
  const foreign = structuredClone(FIXTURE);
  foreign.sessionRef = "00000000-0000-4000-8000-000000000000";
  await assert.rejects(
    invokeCodingSessionObservationFold({
      sessionRef: SESSION,
      genesisRef: GENESIS,
      knownAssignmentRefs: [],
      providerPubkeys: null,
      events: events(),
      invoke: invoker(foreign, []),
    }),
    /does not name the session and genesis it was asked about/,
  );
});

test("a fold that does not echo the exact ids it was handed is refused", async () => {
  const dropped = structuredClone(FIXTURE);
  dropped.inputEventIds = dropped.inputEventIds.slice(1);
  await assert.rejects(
    invokeCodingSessionObservationFold({
      sessionRef: SESSION,
      genesisRef: GENESIS,
      knownAssignmentRefs: [],
      providerPubkeys: null,
      events: events(),
      invoke: invoker(dropped, []),
    }),
    /does not echo the exact event ids it was handed/,
  );
});

test("a fold citing an event nobody handed it is refused", async () => {
  const invented = structuredClone(FIXTURE);
  invented.ignored.push({
    eventId: "99".repeat(32),
    reason: "invented",
  });
  await assert.rejects(
    invokeCodingSessionObservationFold({
      sessionRef: SESSION,
      genesisRef: GENESIS,
      knownAssignmentRefs: [],
      providerPubkeys: null,
      events: events(),
      invoke: invoker(invented, []),
    }),
    /cites an event id it was not handed/,
  );
});

test("a finding's own pointers are never required to be inside the fold", async () => {
  // `refs` and `assignmentRef` are pointers their author supplied; the fixture
  // carries one of each that names an event outside the input set, and that is
  // not an error — it is the reason the fold calls them pointers.
  const result = await invokeCodingSessionObservationFold({
    sessionRef: SESSION,
    genesisRef: GENESIS,
    knownAssignmentRefs: [],
    providerPubkeys: null,
    events: events(),
    invoke: invoker(FIXTURE, []),
  });
  assert.equal(result.fold.findings[0].refs.length, 1);
  assert.equal(
    result.fold.findings[0].refs.includes(FIXTURE.inputEventIds[0]),
    false,
  );
});
