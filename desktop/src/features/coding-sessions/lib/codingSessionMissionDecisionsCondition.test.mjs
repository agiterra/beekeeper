/**
 * The class a ruling covers, on the decision-queue row.
 *
 * A separate file from `codingSessionMissionDecisions.test.mjs` on purpose:
 * another lane owns that suite in this batch, and a new file cannot collide.
 */
import assert from "node:assert/strict";
import test from "node:test";

import { deriveCodingSessionMissionInspectorModel } from "./codingSessionMissionInspectorModel.ts";

// Live run 2, channel d3e440ea: the founder's standing ruling, and the request
// that had to be asked twice before it existed (finding 21).
const REQUEST =
  "95901be7d9dd3fa06a04d1c3d70e6a7a19cbb8ec4c9ba7e5e10e93bd7e3e2f01";
const ANSWER =
  "429e8545b0f6b4d0a9a1a0a1e18c7a2e30a1b5c8e6f4a2d9c1b0e7f3a5d2c4b6";
const ASSIGNMENT =
  "3234382f094519e81c1a572ce1383516fe093b0a6b179100525d6f47909e2a9a";
const LEAD = "e".repeat(64);
const FOUNDER = "3".repeat(64);
const ASKED_AT = 1_788_359_882;

const CONDITION = "any SHA whose buzz-acp diff against origin/main is empty";

function input({ condition, answered = true }) {
  return {
    goal: { kind: "absent" },
    acceptedPlan: { kind: "absent" },
    assignments: [],
    seatPlans: [],
    reports: [],
    transactions: [
      {
        sourceEventId: REQUEST,
        type: "decision.request",
        authorPubkey: LEAD,
        createdAt: ASKED_AT,
        counterpartyPubkey: null,
        parentEventId: null,
        summary: "Push with --no-verify, or hold?",
        decision: null,
        requiredAction: null,
        fileCount: null,
        testCount: null,
        unseated: false,
      },
      {
        sourceEventId: ANSWER,
        type: "decision.answer",
        authorPubkey: FOUNDER,
        createdAt: ASKED_AT + 60,
        counterpartyPubkey: LEAD,
        parentEventId: REQUEST,
        summary: "push with --no-verify",
        decision: null,
        requiredAction: null,
        fileCount: null,
        testCount: null,
        unseated: false,
        condition,
      },
    ],
    observedChanges: { files: [], unreportedEditCount: 0 },
    participants: [],
    contextLoads: new Map(),
    missionState: {
      kind: "running",
      sourceEventId: ASSIGNMENT,
      phase: "assigned",
      detail: "Assigned.",
      canonicalChain: [],
    },
    usage: null,
    rejectedEventCount: 0,
    rejectionsTruncated: false,
    rejectedReasons: [],
    conflicts: [],
    founderPubkey: FOUNDER,
    resolveActorLabel: () => null,
    decisions: [
      {
        requestId: REQUEST,
        heldOn: "founder",
        blocks: [ASSIGNMENT],
        answeredBy: answered ? FOUNDER : null,
        answerId: answered ? ANSWER : null,
      },
    ],
    waitingOnDecision: null,
    leadHasOpenTurn: false,
  };
}

test("an answered row carries the class the ruling covers", () => {
  const model = deriveCodingSessionMissionInspectorModel(
    input({ condition: CONDITION }),
  );
  const [row] = model.decisions;
  assert.equal(row.state, "answered");
  assert.equal(row.condition, CONDITION);
});

test("a ruling that named no class carries none", () => {
  for (const condition of [null, undefined, "", "   "]) {
    const model = deriveCodingSessionMissionInspectorModel(
      input({ condition }),
    );
    assert.equal(
      model.decisions[0].condition,
      null,
      `condition ${JSON.stringify(condition)} must read as none`,
    );
  }
});

test("an open row never claims a condition, because nothing has ruled", () => {
  const model = deriveCodingSessionMissionInspectorModel(
    input({ condition: CONDITION, answered: false }),
  );
  assert.equal(model.decisions[0].state, "open");
  assert.equal(model.decisions[0].condition, null);
});

test("a long condition is bounded like the question beside it", () => {
  const model = deriveCodingSessionMissionInspectorModel(
    input({ condition: "x".repeat(400) }),
  );
  const { condition } = model.decisions[0];
  assert.equal(condition.length, 200);
  assert.ok(condition.endsWith("…"));
});

test("an answer that fell outside the row bound reads unknown, not empty", () => {
  // REVIEW-L7 F10 / I9. The answer exists and the fold says the row is
  // answered, but its transaction row aged out of the bounded window, so this
  // surface has not read its `condition`. That is "not read", never "named no
  // class" — a renderer must be able to tell them apart.
  const base = input({ condition: CONDITION });
  const model = deriveCodingSessionMissionInspectorModel({
    ...base,
    // Keep the request row (so the question still renders) and drop only the
    // answer's row, which is exactly what the bound does to an old answer.
    transactions: base.transactions.filter(
      (row) => row.type !== "decision.answer",
    ),
  });
  const [row] = model.decisions;
  assert.equal(row.state, "answered");
  assert.equal(row.condition, "unknown");
  // And the row still says who answered: the fold knows that without the row.
  assert.ok(row.stateWord.startsWith("Answered by"));
});

test("a null answerId on an answered row is unknown, never a named class", () => {
  const base = input({ condition: CONDITION });
  const model = deriveCodingSessionMissionInspectorModel({
    ...base,
    decisions: [{ ...base.decisions[0], answerId: null }],
  });
  // `answeredBy` without `answerId` cannot be checked against any row.
  assert.equal(model.decisions[0].condition, "unknown");
});
