import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import test from "node:test";

import {
  buildCodingSessionObservationFilters,
  CODING_SESSION_OBSERVATION_HISTORY_LIMIT,
  readCodingSessionObservations,
} from "./useCodingSessionObservations.ts";

const FIXTURE = JSON.parse(
  readFileSync(
    fileURLToPath(
      new URL(
        "../lib/codingSessionObservationFoldAdapterResponse.fixture.json",
        import.meta.url,
      ),
    ),
    "utf8",
  ),
);

const SCOPE = {
  channelRef: "d3e440ea-89f8-4aee-8a02-17edc3e7272e",
  sessionRef: FIXTURE.sessionRef,
  genesisRef: FIXTURE.genesisRef,
};

test("the filter names its kinds, and scopes by channel, umbrella and genesis", () => {
  const filters = buildCodingSessionObservationFilters(
    SCOPE,
    CODING_SESSION_OBSERVATION_HISTORY_LIMIT,
  );
  assert.equal(filters.length, 1);
  // Omitting `kinds` trips the relay's p-gate (403), so it is never omitted.
  assert.deepEqual(filters[0].kinds, [44246]);
  assert.deepEqual(filters[0]["#h"], [SCOPE.channelRef]);
  assert.deepEqual(filters[0]["#d"], [SCOPE.sessionRef]);
  assert.deepEqual(filters[0]["#csob-genesis"], [SCOPE.genesisRef]);
  assert.equal(filters[0].limit, CODING_SESSION_OBSERVATION_HISTORY_LIMIT);
});

test("the read hands the relay's events to the native fold, unfiltered", async () => {
  const fetched = FIXTURE.inputEventIds.map((id, index) => ({
    id,
    pubkey: "11".repeat(32),
    created_at: 1_756_800_000 + index,
    kind: 44246,
    tags: [],
    content: "{}",
    sig: "22".repeat(64),
  }));
  const calls = [];
  const result = await readCodingSessionObservations({
    scope: SCOPE,
    knownAssignmentRefs: ["cd".repeat(32)],
    providerPubkeys: ["66".repeat(32)],
    client: { fetchEventsCoalesced: async () => fetched },
    invoke: async (command, args) => {
      calls.push({ command, args });
      return structuredClone(FIXTURE);
    },
  });

  assert.equal(calls.length, 1);
  // Nothing is pre-filtered: the fold verifies each signature itself and lists
  // what it cannot read, and an event dropped here would vanish instead of
  // being disclosed.
  assert.equal(calls[0].args.request.events.length, fetched.length);
  // REVIEW-L5 F2: the provider set rides with the request, so Rust decides
  // whether an `observed` claim was earned.
  assert.deepEqual(calls[0].args.request.providerPubkeys, ["66".repeat(32)]);
  assert.equal(result.fold.gates.length, 2);
  assert.equal(result.signedAt.get(fetched[0].id), 1_756_800_000);
});

test("a relay that returns nothing folds to an empty answer, not to an error", async () => {
  const empty = {
    ...structuredClone(FIXTURE),
    inputEventIds: [],
    checkpoints: [],
    gates: [],
    findings: [],
    phases: [],
    unresolved: [],
    ignored: [],
  };
  const result = await readCodingSessionObservations({
    scope: SCOPE,
    knownAssignmentRefs: [],
    providerPubkeys: null,
    client: { fetchEventsCoalesced: async () => [] },
    invoke: async () => structuredClone(empty),
  });
  assert.deepEqual(result.fold.gates, []);
  assert.equal(result.signedAt.size, 0);
});
