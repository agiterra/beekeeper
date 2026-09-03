import assert from "node:assert/strict";
import test from "node:test";

import {
  codingSessionMissionAskedRelative,
  deriveCodingSessionMissionInspectorModel,
  MAX_CODING_SESSION_MISSION_DECISION_ROWS,
} from "./codingSessionMissionInspectorModel.ts";

// Live run 2 again: one request held on the founder with `blocks: []`, and one
// held on an actor that blocks the open assignment.
const REQUEST_FOUNDER =
  "2099cdb3e076ccb9c51cb4526710c4d3d7a551ad6c4fda6e61176f4d47b601cb";
const ANSWER_FOUNDER =
  "4847ff0645533c1b0c2464d9aaad80ba1d51c5b74bc77bf495dd9e617fdd2bf3";
const REQUEST_ACTOR =
  "215525d114edd56439aa3210f28e0018010ba4a18ff272263099cbd40604e27c";
const ASSIGNMENT =
  "3234382f094519e81c1a572ce1383516fe093b0a6b179100525d6f47909e2a9a";
const LEAD = "e".repeat(64);
const BUILDER = "b".repeat(64);
const FOUNDER = "3".repeat(64);

const ASKED_AT = 1_788_359_882;

function baseInput(overrides = {}) {
  return {
    goal: { kind: "absent" },
    acceptedPlan: { kind: "absent" },
    assignments: [
      {
        sourceEventId: ASSIGNMENT,
        authorLabel: LEAD,
        assigneeRole: "builder",
        objective: "Add `bee sessions whoami`",
        brief: "",
        fileOwnership: [],
      },
    ],
    seatPlans: [],
    reports: [],
    transactions: [
      {
        sourceEventId: REQUEST_FOUNDER,
        type: "decision.request",
        authorPubkey: LEAD,
        createdAt: ASKED_AT,
        counterpartyPubkey: null,
        parentEventId: null,
        summary: "Should the JSON also carry the seat role?",
        decision: null,
        requiredAction: null,
        fileCount: null,
        testCount: null,
        unseated: false,
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
    resolveActorLabel: (pubkey) =>
      pubkey === LEAD ? "Keystone" : pubkey === BUILDER ? "Bob" : null,
    ...overrides,
  };
}

test("L2.2: a founder-held open request with blocks:[] says it holds up no assignment yet", () => {
  const model = deriveCodingSessionMissionInspectorModel(
    baseInput({
      decisions: [
        {
          requestId: REQUEST_FOUNDER,
          heldOn: "founder",
          blocks: [],
          answeredBy: null,
          answerId: null,
        },
      ],
      waitingOnDecision: { requestId: REQUEST_FOUNDER, heldOn: "founder" },
      leadHasOpenTurn: false,
    }),
  );
  assert.equal(model.decisions.length, 1);
  const [row] = model.decisions;
  assert.equal(row.state, "open");
  assert.equal(row.stateWord, "Open · held on the founder");
  assert.equal(row.blocksWord, "holds up no assignment yet");
  assert.equal(row.question, "Should the JSON also carry the seat role?");
  assert.equal(row.shortId, "2099cdb3");
  assert.equal(row.askedAtMs, ASKED_AT * 1000);
  assert.equal(model.waiting?.line, "Waiting on the founder");
  assert.equal(model.waiting?.placement, "state-line");
});

test("L2.2: with the lead working the waiting fact is present but does not take the state line", () => {
  const model = deriveCodingSessionMissionInspectorModel(
    baseInput({
      decisions: [
        {
          requestId: REQUEST_FOUNDER,
          heldOn: "founder",
          blocks: [],
          answeredBy: null,
          answerId: null,
        },
      ],
      waitingOnDecision: { requestId: REQUEST_FOUNDER, heldOn: "founder" },
      leadHasOpenTurn: true,
    }),
  );
  assert.equal(model.waiting?.line, "Waiting on the founder");
  assert.equal(model.waiting?.placement, "beside");
  // The mission's own state is never overwritten by the waiting fact.
  assert.equal(model.missionState.kind, "running");
});

test("L2.2: an actor-held request names the actor and the assignment it blocks", () => {
  const model = deriveCodingSessionMissionInspectorModel(
    baseInput({
      transactions: [
        {
          sourceEventId: REQUEST_ACTOR,
          type: "decision.request",
          authorPubkey: BUILDER,
          createdAt: ASKED_AT + 100,
          counterpartyPubkey: LEAD,
          parentEventId: null,
          summary:
            "Pre-push hook fails on a pre-existing test. Push --no-verify?",
          decision: null,
          requiredAction: null,
          fileCount: null,
          testCount: null,
          unseated: false,
        },
      ],
      decisions: [
        {
          requestId: REQUEST_ACTOR,
          heldOn: LEAD,
          blocks: [ASSIGNMENT],
          answeredBy: null,
          answerId: null,
        },
      ],
      waitingOnDecision: { requestId: REQUEST_ACTOR, heldOn: LEAD },
      leadHasOpenTurn: false,
    }),
  );
  const [row] = model.decisions;
  assert.equal(row.stateWord, "Open · held on Keystone");
  assert.equal(row.blocksWord, "holds up 1 assignment: 3234382f");
  assert.equal(model.waiting?.line, "Waiting on Keystone");
});

test("L2.2: an answered request is answered by a name and stops the waiting line", () => {
  const model = deriveCodingSessionMissionInspectorModel(
    baseInput({
      decisions: [
        {
          requestId: REQUEST_FOUNDER,
          heldOn: "founder",
          blocks: [],
          answeredBy: FOUNDER,
          answerId: ANSWER_FOUNDER,
        },
      ],
      waitingOnDecision: null,
      leadHasOpenTurn: false,
    }),
  );
  const [row] = model.decisions;
  assert.equal(row.state, "answered");
  assert.equal(row.stateWord, "Answered by the founder");
  assert.equal(model.waiting, null);
});

test("L2.2: no decisions is the §1g empty state, never a blank panel", () => {
  const model = deriveCodingSessionMissionInspectorModel(
    baseInput({ decisions: [], waitingOnDecision: null }),
  );
  assert.deepEqual(model.decisions, []);
  assert.equal(model.decisionsTruncated, 0);
  assert.equal(model.waiting, null);
});

test("L2.2: an absent fold is unknown, not an empty queue", () => {
  const model = deriveCodingSessionMissionInspectorModel(baseInput());
  assert.equal(model.decisionsKnown, false);
  const folded = deriveCodingSessionMissionInspectorModel(
    baseInput({ decisions: [], waitingOnDecision: null }),
  );
  assert.equal(folded.decisionsKnown, true);
});

test("L2.2: the queue is bounded at 50 rows with the omission disclosed", () => {
  const decisions = Array.from({ length: 63 }, (_, index) => ({
    requestId: `${index.toString(16).padStart(2, "0")}`.repeat(32),
    heldOn: "founder",
    blocks: [],
    answeredBy: FOUNDER,
    answerId: null,
  }));
  const model = deriveCodingSessionMissionInspectorModel(
    baseInput({ decisions, waitingOnDecision: null }),
  );
  assert.equal(MAX_CODING_SESSION_MISSION_DECISION_ROWS, 50);
  assert.equal(model.decisions.length, 50);
  assert.equal(model.decisionsTruncated, 13);
});

test("L2.2: open rows sort before answered ones, newest asked first", () => {
  const model = deriveCodingSessionMissionInspectorModel(
    baseInput({
      transactions: [
        {
          sourceEventId: REQUEST_FOUNDER,
          type: "decision.request",
          authorPubkey: LEAD,
          createdAt: ASKED_AT,
          counterpartyPubkey: null,
          parentEventId: null,
          summary: "older",
          decision: null,
          requiredAction: null,
          fileCount: null,
          testCount: null,
          unseated: false,
        },
        {
          sourceEventId: REQUEST_ACTOR,
          type: "decision.request",
          authorPubkey: BUILDER,
          createdAt: ASKED_AT + 500,
          counterpartyPubkey: null,
          parentEventId: null,
          summary: "newer",
          decision: null,
          requiredAction: null,
          fileCount: null,
          testCount: null,
          unseated: false,
        },
      ],
      decisions: [
        {
          requestId: REQUEST_FOUNDER,
          heldOn: "founder",
          blocks: [],
          answeredBy: FOUNDER,
          answerId: ANSWER_FOUNDER,
        },
        {
          requestId: REQUEST_ACTOR,
          heldOn: LEAD,
          blocks: [],
          answeredBy: null,
          answerId: null,
        },
      ],
      waitingOnDecision: null,
    }),
  );
  assert.deepEqual(
    model.decisions.map((row) => row.shortId),
    ["215525d1", "2099cdb3"],
  );
});

test("L2.2: `asked` is relative and never zero", () => {
  const now = ASKED_AT * 1000;
  assert.equal(codingSessionMissionAskedRelative(null, now), null);
  assert.equal(codingSessionMissionAskedRelative(now, now), null);
  assert.equal(codingSessionMissionAskedRelative(now - 59_000, now), null);
  assert.equal(
    codingSessionMissionAskedRelative(now - 12 * 60_000, now),
    "12m",
  );
  assert.equal(
    codingSessionMissionAskedRelative(now - 3 * 3_600_000, now),
    "3h",
  );
  assert.equal(
    codingSessionMissionAskedRelative(now - 2 * 86_400_000, now),
    "2d",
  );
  // A stamp in the future is not a duration; it says nothing rather than a lie.
  assert.equal(codingSessionMissionAskedRelative(now + 60_000, now), null);
});

// ── Fix round 1: F4 (terminal), F12 (you), F13 (truncation copy), F11 ───────

const VIEWER = "aa".repeat(32);

test("F4: a terminal state is never replaced by the waiting line", () => {
  for (const [missionState, word] of [
    [
      {
        kind: "completed",
        sourceEventId: ASSIGNMENT,
        summary: "Done.",
        landedShas: [],
        followUps: [],
        canonicalChain: [],
      },
      "Mission completed",
    ],
    [
      {
        kind: "blocked",
        sourceEventId: ASSIGNMENT,
        summary: "Stuck.",
        blockers: ["a"],
        requiredAction: "rule",
        canonicalChain: [],
      },
      "Mission blocked",
    ],
    [{ kind: "conflict", eventIds: [ASSIGNMENT] }, "Mission state conflict"],
  ]) {
    const model = deriveCodingSessionMissionInspectorModel(
      baseInput({
        missionState,
        decisions: [
          {
            requestId: REQUEST_FOUNDER,
            heldOn: "founder",
            blocks: [],
            answeredBy: null,
            answerId: null,
          },
        ],
        waitingOnDecision: { requestId: REQUEST_FOUNDER, heldOn: "founder" },
        leadHasOpenTurn: false,
      }),
    );
    assert.equal(model.waiting?.placement, "appended", word);
    assert.equal(model.waiting?.line, "Waiting on the founder");
    assert.equal(model.missionState.kind, missionState.kind);
  }
});

test("F4: a non-terminal state with no open lead turn still takes the state line", () => {
  const model = deriveCodingSessionMissionInspectorModel(
    baseInput({
      decisions: [
        {
          requestId: REQUEST_FOUNDER,
          heldOn: "founder",
          blocks: [],
          answeredBy: null,
          answerId: null,
        },
      ],
      waitingOnDecision: { requestId: REQUEST_FOUNDER, heldOn: "founder" },
      leadHasOpenTurn: false,
    }),
  );
  assert.equal(model.waiting?.placement, "state-line");
});

test("F12: a ruling held on the viewer says `you`, not their key", () => {
  const model = deriveCodingSessionMissionInspectorModel(
    baseInput({
      currentUserPubkey: BUILDER,
      decisions: [
        {
          requestId: REQUEST_FOUNDER,
          heldOn: BUILDER,
          blocks: [],
          answeredBy: null,
          answerId: null,
        },
      ],
      waitingOnDecision: { requestId: REQUEST_FOUNDER, heldOn: BUILDER },
    }),
  );
  assert.equal(model.decisions[0].stateWord, "Open · held on you");
  assert.equal(model.waiting?.line, "Waiting on you");
});

test("F12: the founder reading their own held request also sees `you`", () => {
  const model = deriveCodingSessionMissionInspectorModel(
    baseInput({
      currentUserPubkey: FOUNDER,
      decisions: [
        {
          requestId: REQUEST_FOUNDER,
          heldOn: "founder",
          blocks: [],
          answeredBy: null,
          answerId: null,
        },
      ],
      waitingOnDecision: { requestId: REQUEST_FOUNDER, heldOn: "founder" },
    }),
  );
  assert.equal(model.decisions[0].stateWord, "Open · held on you");
  // Somebody who is not the founder still reads `the founder`.
  const other = deriveCodingSessionMissionInspectorModel(
    baseInput({
      currentUserPubkey: VIEWER,
      decisions: [
        {
          requestId: REQUEST_FOUNDER,
          heldOn: "founder",
          blocks: [],
          answeredBy: null,
          answerId: null,
        },
      ],
    }),
  );
  assert.equal(other.decisions[0].stateWord, "Open · held on the founder");
});

test("F11: an unresolvable party is the canonical truncation, not a hand-rolled slice", () => {
  const model = deriveCodingSessionMissionInspectorModel(
    baseInput({
      decisions: [
        {
          requestId: REQUEST_FOUNDER,
          heldOn: VIEWER,
          blocks: [],
          answeredBy: null,
          answerId: null,
        },
      ],
    }),
  );
  assert.equal(
    model.decisions[0].stateWord,
    `Open · held on ${"a".repeat(8)}…aaaa`,
  );
});

test("F13: the truncation row says what was actually dropped", () => {
  const answered = Array.from({ length: 63 }, (_, index) => ({
    requestId: `${index.toString(16).padStart(2, "0")}`.repeat(32),
    heldOn: "founder",
    blocks: [],
    answeredBy: FOUNDER,
    answerId: null,
  }));
  const model = deriveCodingSessionMissionInspectorModel(
    baseInput({ decisions: answered, waitingOnDecision: null }),
  );
  assert.equal(model.decisionsTruncated, 13);
  assert.equal(
    model.decisionsTruncatedNotice,
    "13 older answered decisions not shown",
  );
  // Open rows sort first, so five of them push five more answered rows out —
  // and the notice is still honestly about answered rows.
  const mixed = [
    ...answered,
    ...Array.from({ length: 5 }, (_, index) => ({
      requestId: `f${index}`.repeat(32),
      heldOn: "founder",
      blocks: [],
      answeredBy: null,
      answerId: null,
    })),
  ];
  const mixedModel = deriveCodingSessionMissionInspectorModel(
    baseInput({ decisions: mixed, waitingOnDecision: null }),
  );
  assert.equal(mixedModel.decisionsTruncated, 18);
  assert.equal(
    mixedModel.decisionsTruncatedNotice,
    "18 older answered decisions not shown",
  );
  // Only when the bound actually drops an OPEN ruling does the notice stop
  // claiming everything hidden was answered — the case a reader must not be
  // reassured about.
  const allOpen = Array.from({ length: 55 }, (_, index) => ({
    requestId: `${(index + 100).toString(16)}`.repeat(16),
    heldOn: "founder",
    blocks: [],
    answeredBy: null,
    answerId: null,
  }));
  const openModel = deriveCodingSessionMissionInspectorModel(
    baseInput({ decisions: allOpen, waitingOnDecision: null }),
  );
  assert.equal(openModel.decisionsTruncated, 5);
  assert.equal(openModel.decisionsTruncatedNotice, "5 decisions not shown");
});

test("F13: a very long question is bounded on the row", () => {
  const model = deriveCodingSessionMissionInspectorModel(
    baseInput({
      transactions: [
        {
          sourceEventId: REQUEST_FOUNDER,
          type: "decision.request",
          authorPubkey: LEAD,
          createdAt: ASKED_AT,
          counterpartyPubkey: null,
          parentEventId: null,
          summary: "q".repeat(400),
          decision: null,
          requiredAction: null,
          fileCount: null,
          testCount: null,
          unseated: false,
        },
      ],
      decisions: [
        {
          requestId: REQUEST_FOUNDER,
          heldOn: "founder",
          blocks: [],
          answeredBy: null,
          answerId: null,
        },
      ],
    }),
  );
  assert.ok(
    model.decisions[0].question.length <= 200,
    model.decisions[0].question.length,
  );
  assert.ok(model.decisions[0].question.endsWith("…"));
});

// ── L8: the row an Answer control needs ──────────────────────────────────────

test("L8.1: a row carries the request's own declared options, and invents none", () => {
  const model = deriveCodingSessionMissionInspectorModel(
    baseInput({
      decisions: [
        {
          requestId: REQUEST_FOUNDER,
          heldOn: "founder",
          blocks: [],
          answeredBy: null,
          answerId: null,
        },
        {
          requestId: REQUEST_ACTOR,
          heldOn: BUILDER,
          blocks: [ASSIGNMENT],
          answeredBy: null,
          answerId: null,
        },
      ],
      channelRef: "e0d3f1b8-8c66-4c62-9ef1-3fa933b32f86",
      sessionRef: "5b7e1c2a-90d4-4b0e-a1f3-7c2d8e6f4a10",
      genesisRef: "ab".repeat(32),
      currentUserPubkey: FOUNDER,
      founderPubkey: FOUNDER,
      decisionRequests: [
        {
          requestId: REQUEST_FOUNDER,
          question: "Should the JSON also carry the seat role?",
          options: ["yes", "no", "only for leads"],
          recommendation: "yes",
          createdAt: ASKED_AT,
        },
        {
          requestId: REQUEST_ACTOR,
          question: "Rebase or merge?",
          options: [],
          recommendation: null,
          createdAt: ASKED_AT,
        },
      ],
    }),
  );
  const founderRow = model.decisions.find(
    (row) => row.requestId === REQUEST_FOUNDER,
  );
  assert.deepEqual(founderRow.options, ["yes", "no", "only for leads"]);
  assert.equal(founderRow.recommendation, "yes");
  assert.equal(founderRow.channelRef, "e0d3f1b8-8c66-4c62-9ef1-3fa933b32f86");

  const actorRow = model.decisions.find(
    (row) => row.requestId === REQUEST_ACTOR,
  );
  // `options: []` is a real answer from the wire, not a gap: the form offers
  // free text alone rather than inventing buttons the asker never wrote.
  assert.deepEqual(actorRow.options, []);
});

test("L8.1: a row held on somebody else disables with §1l's sentence, and is never hidden", () => {
  const model = deriveCodingSessionMissionInspectorModel(
    baseInput({
      decisions: [
        {
          requestId: REQUEST_FOUNDER,
          heldOn: "founder",
          blocks: [],
          answeredBy: null,
          answerId: null,
        },
        {
          requestId: REQUEST_ACTOR,
          heldOn: BUILDER,
          blocks: [ASSIGNMENT],
          answeredBy: null,
          answerId: null,
        },
      ],
      currentUserPubkey: FOUNDER,
      founderPubkey: FOUNDER,
      resolveActorLabel: (pubkey) => (pubkey === BUILDER ? "Bob" : null),
    }),
  );
  const founderRow = model.decisions.find(
    (row) => row.requestId === REQUEST_FOUNDER,
  );
  assert.equal(founderRow.viewerIsHolder, true);
  assert.equal(founderRow.heldElsewhereSentence, null);

  const actorRow = model.decisions.find(
    (row) => row.requestId === REQUEST_ACTOR,
  );
  assert.equal(actorRow.viewerIsHolder, false);
  assert.equal(
    actorRow.heldElsewhereSentence,
    "This ruling is held on Bob, so only they can answer it. You can read it here.",
  );
  // The row is still in the queue — disabled, never hidden.
  assert.ok(model.decisions.some((row) => row.requestId === REQUEST_ACTOR));
});

test("L8.1: a viewer this surface cannot name is not permitted — unknown disables (F9)", () => {
  const model = deriveCodingSessionMissionInspectorModel(
    baseInput({
      decisions: [
        {
          requestId: REQUEST_FOUNDER,
          heldOn: "founder",
          blocks: [],
          answeredBy: null,
          answerId: null,
        },
        {
          requestId: REQUEST_ACTOR,
          heldOn: BUILDER,
          blocks: [ASSIGNMENT],
          answeredBy: null,
          answerId: null,
        },
      ],
      currentUserPubkey: null,
      founderPubkey: FOUNDER,
    }),
  );
  for (const row of model.decisions) {
    // The third state survives on the model — this surface does not know.
    assert.equal(row.viewerIsHolder, null);
    // REVIEW-L8 F9: and unknown is not rendered as permitted. §1l's sentence
    // prints, naming the party the wire says holds the ruling, and the form
    // disables. Rendering an enabled control here would be a guess in the one
    // direction §8 I9 forbids.
    assert.match(
      row.heldElsewhereSentence,
      /^This ruling is held on .+, so only they can answer it\. You can read it here\.$/,
    );
  }
});

test("L8.4: an answered row reads its choice and its condition, and discloses the clamp", () => {
  const long = `every commit on lane/batch3-l8-founder that ${"keeps the gate green ".repeat(20)}`;
  const model = deriveCodingSessionMissionInspectorModel(
    baseInput({
      decisions: [
        {
          requestId: REQUEST_FOUNDER,
          heldOn: "founder",
          blocks: [],
          answeredBy: FOUNDER,
          answerId: ANSWER_FOUNDER,
        },
      ],
      currentUserPubkey: FOUNDER,
      founderPubkey: FOUNDER,
      decisionRequests: [
        {
          requestId: REQUEST_FOUNDER,
          question: "Should the JSON also carry the seat role?",
          options: ["yes", "no", "only for leads"],
          recommendation: null,
          createdAt: ASKED_AT,
        },
      ],
      decisionAnswers: [
        {
          answerId: ANSWER_FOUNDER,
          requestRef: REQUEST_FOUNDER,
          choice: 2,
          note: null,
          condition: long,
        },
      ],
    }),
  );
  const row = model.decisions.find((r) => r.requestId === REQUEST_FOUNDER);
  assert.equal(row.state, "answered");
  // The index is resolved through the *request's* own options.
  assert.equal(row.answerChoiceWord, "only for leads");
  assert.equal(row.answerCondition.text.length, 200);
  assert.ok(row.answerCondition.truncated > 0);
});

test("L8.4: an answer with no condition renders none, and absent is never blank", () => {
  const model = deriveCodingSessionMissionInspectorModel(
    baseInput({
      decisions: [
        {
          requestId: REQUEST_FOUNDER,
          heldOn: "founder",
          blocks: [],
          answeredBy: FOUNDER,
          answerId: ANSWER_FOUNDER,
        },
      ],
      currentUserPubkey: FOUNDER,
      founderPubkey: FOUNDER,
      decisionAnswers: [
        {
          answerId: ANSWER_FOUNDER,
          requestRef: REQUEST_FOUNDER,
          choice: "rebase onto main first",
          note: null,
          condition: null,
        },
      ],
    }),
  );
  const row = model.decisions.find((r) => r.requestId === REQUEST_FOUNDER);
  assert.equal(row.answerCondition, null);
  assert.equal(row.answerChoiceWord, "rebase onto main first");
});
