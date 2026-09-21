import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { test } from "node:test";

import { decodeNativeCodingSessionTeamFoldResponse } from "./invokeCodingSessionTeamFold.ts";
import { awaitingSentence } from "../ui/CodingSessionMissionSettlement.tsx";

// The same file `buzz-core`, `buzz-cli` and the provider load. Ledger 204's
// rule: a change to what a closed record means lands with one shared fixture
// that every strict reader runs, so the reader left behind fails its own test
// instead of a live mission. Lane 210's settlement rule is that change.
const VECTORS = JSON.parse(
  readFileSync(
    resolve(
      dirname(fileURLToPath(import.meta.url)),
      "../../../../../conformance/team-settlement/fixtures/settlement-vectors.json",
    ),
    "utf8",
  ),
);

/** A distinct, valid 64-hex id per symbolic name, stable within a vector. */
function eventIds(vector) {
  const ids = new Map();
  vector.records.forEach((record, index) => {
    ids.set(record.id, (index + 1).toString(16).padStart(2, "0").repeat(32));
  });
  return ids;
}

const ASSIGNEE = "22".repeat(32);

/** The adapter response a vector's expectations describe. */
function adapterResponse(vector) {
  const ids = eventIds(vector);
  const inputEventIds = [...ids.values()];
  const excluded = (vector.expected.excluded ?? []).map((symbol) => ({
    eventId: ids.get(symbol),
    code: "unauthorized",
    reason: "the vector's excluded set",
  }));
  const excludedIds = new Set(excluded.map((item) => item.eventId));
  const pending = vector.expected.pendingCompletion;
  return {
    response: {
      schema: "buzz-coding-session-team-fold-adapter/v1",
      implementation: "buzz-core",
      inputEventIds,
      context: {
        channelRef: "05ef0ecf-745f-5fb8-b7ff-f9cba21e01c2",
        sessionRef: "5b7e1c2a-90d4-4b0e-a1f3-7c2d8e6f4a10",
        genesisRef: "ab".repeat(32),
        founderPubkey: "11".repeat(32),
        authorityHeadEventId: null,
        authorityHeadSeq: 0,
        verifierRequired: false,
      },
      includedEventIds: inputEventIds.filter((id) => !excludedIds.has(id)),
      excluded,
      conflicts: [],
      assignments: vector.expected.assignments.map((row) => ({
        assignmentEventId: ids.get(row.assignment),
        governedReportEventId: row.governedReport
          ? ids.get(row.governedReport)
          : null,
        dispositionEventId: row.disposition ? ids.get(row.disposition) : null,
        acknowledgementEventId: row.acknowledgement
          ? ids.get(row.acknowledgement)
          : null,
        settled: row.settled,
        settledBy: row.settledBy,
        awaiting: row.awaiting
          ? {
              link: row.awaiting.link,
              owedByRole: row.awaiting.owedByRole,
              owedByActor: row.awaiting.owedByActor ? ASSIGNEE : null,
            }
          : null,
      })),
      unseatedReports: [],
      notes: [],
      decisions: [],
      waitingOnDecision: null,
      canonicalTerminal: vector.expected.terminal
        ? {
            eventId: ids.get(vector.expected.terminal),
            type: "mission.completed",
          }
        : null,
      pendingCompletion: pending
        ? {
            eventId: ids.get(pending.eventId),
            code: pending.code,
            reason: "the vector's held completion",
            unsettledAssignmentEventIds: pending.unsettled.map((symbol) =>
              ids.get(symbol),
            ),
          }
        : null,
    },
    inputEventIds,
  };
}

test("the strict decoder accepts every vector's settlement projection", () => {
  assert.ok(VECTORS.vectors.length > 0);
  for (const vector of VECTORS.vectors) {
    const { response } = adapterResponse(vector);
    const decoded = decodeNativeCodingSessionTeamFoldResponse(response);
    decoded.assignments.forEach((assignment, index) => {
      const row = vector.expected.assignments[index];
      assert.equal(assignment.settled, row.settled, vector.name);
      assert.equal(assignment.settledBy, row.settledBy, vector.name);
      if (row.settled) {
        assert.equal(assignment.awaiting, null, vector.name);
        // The rule this lane exists for: a settled assignment never reads as
        // awaiting an acknowledgement.
        assert.notEqual(assignment.settledBy, null, vector.name);
      }
    });
  }
});

test("a projection carrying a key this decoder does not know is refused", () => {
  const vector = VECTORS.vectors[0];
  const { response } = adapterResponse(vector);
  const extended = structuredClone(response);
  extended.assignments[0].settledAt = 1;
  assert.throws(
    () => decodeNativeCodingSessionTeamFoldResponse(extended),
    /native coding-session team fold/,
    "an unknown assignment key must fail the decoder, not be ignored — ledger 204",
  );
});

test("a projection missing settledBy is refused rather than read as acknowledged", () => {
  const vector = VECTORS.vectors[0];
  const { response } = adapterResponse(vector);
  const older = structuredClone(response);
  delete older.assignments[0].settledBy;
  assert.throws(() => decodeNativeCodingSessionTeamFoldResponse(older));
});

test("the Mission panel says which rule settled each assignment", () => {
  const settledWithoutAsk = {
    assignmentEventId: "aa".repeat(32),
    settled: true,
    settledBy: "approving_disposition_without_ask",
    awaiting: null,
  };
  const sentence = awaitingSentence(settledWithoutAsk);
  assert.match(sentence, /asked for nothing/);
  assert.doesNotMatch(
    sentence,
    /acknowledged|delivered|idle/,
    "auto-settled must never render as acknowledged, delivered or idle",
  );
  assert.equal(
    awaitingSentence({
      assignmentEventId: "bb".repeat(32),
      settled: true,
      settledBy: "acknowledgement",
      awaiting: null,
    }),
    "settled by the assignee's acknowledgement",
  );
});

test("every vector's native response carries the settlement rule", () => {
  // The projection copies this field verbatim and derives none; what this
  // asserts is the contract it copies — every settlement row the native
  // adapter emits carries the fold's own word.
  for (const vector of VECTORS.vectors) {
    const { response } = adapterResponse(vector);
    for (const assignment of response.assignments) {
      assert.ok(
        Object.hasOwn(assignment, "settledBy"),
        `${vector.name}: the native response must always carry settledBy`,
      );
    }
  }
});
