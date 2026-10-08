import assert from "node:assert/strict";
import test from "node:test";

import {
  codingSessionMinimapGateLine,
  codingSessionMinimapOpenDecisionRulings,
  codingSessionMinimapOpenDecisionSource,
  codingSessionMinimapRulingNote,
  deriveCodingSessionMinimapMarks,
} from "./codingSessionTranscriptMinimapMarks.ts";

const turns = [
  { id: "t1", startedAtMs: 1_000, failed: false },
  { id: "t2", startedAtMs: 2_000, failed: true },
  { id: "t3", startedAtMs: 3_000, failed: false },
];

test("a failed turn carries the failed mark; others do not", () => {
  const marks = deriveCodingSessionMinimapMarks({ turns });
  assert.equal(marks.get("t1").failed, false);
  assert.equal(marks.get("t2").failed, true);
  assert.equal(marks.get("t2").gate, null);
});

test("a gate row lands in the window holding its signed time", () => {
  const marks = deriveCodingSessionMinimapMarks({
    turns,
    gates: [
      { gate: "cargo test", outcome: "passed", signedAtMs: 1_500 },
      { gate: "clippy", outcome: "failed", signedAtMs: 2_000 },
      { gate: "fmt", outcome: "passed", signedAtMs: 2_999 },
      // The newest window stays open.
      { gate: "e2e", outcome: "not-run", signedAtMs: 99_000 },
    ],
  });
  assert.deepEqual(marks.get("t1").gate, {
    passed: 1,
    failed: 0,
    notRun: 0,
    glyph: "pass",
    gates: [{ gate: "cargo test", outcome: "passed" }],
  });
  // Any failure inside the window shows ✗.
  assert.equal(marks.get("t2").gate.glyph, "fail");
  assert.equal(marks.get("t2").gate.passed, 1);
  assert.equal(marks.get("t2").gate.failed, 1);
  // Only a not-run row: a gate line, but no glyph that claims an outcome.
  assert.equal(marks.get("t3").gate.glyph, null);
  assert.equal(marks.get("t3").gate.notRun, 1);
});

test("facts before the first turn, or undated, mark nothing", () => {
  const marks = deriveCodingSessionMinimapMarks({
    turns,
    gates: [
      { gate: "early", outcome: "failed", signedAtMs: 500 },
      { gate: "undated", outcome: "failed", signedAtMs: null },
    ],
    rulings: [{ sinceAtMs: null }],
  });
  for (const id of ["t1", "t2", "t3"]) {
    assert.equal(marks.get(id).gate, null);
    assert.equal(marks.get(id).waitingRulings, 0);
  }
});

test("handovers (44247) and waiting rulings (DB8) count per window", () => {
  const marks = deriveCodingSessionMinimapMarks({
    turns,
    handovers: [
      { type: "checkpoint", signedAtMs: 1_100 },
      { type: "continuation", signedAtMs: 1_200 },
    ],
    rulings: [{ sinceAtMs: 3_500 }],
  });
  assert.equal(marks.get("t1").handovers, 2);
  assert.equal(marks.get("t2").handovers, 0);
  assert.equal(marks.get("t3").waitingRulings, 1);
});

test("an undated turn gets no window; its neighbours span the gap", () => {
  const marks = deriveCodingSessionMinimapMarks({
    turns: [
      { id: "t1", startedAtMs: 1_000, failed: false },
      { id: "t2", startedAtMs: null, failed: false },
      { id: "t3", startedAtMs: 3_000, failed: false },
    ],
    gates: [{ gate: "g", outcome: "passed", signedAtMs: 2_000 }],
  });
  assert.equal(marks.get("t1").gate.passed, 1);
  assert.equal(marks.get("t2").gate, null);
});

test("the card's gate line says which nothing it means", () => {
  assert.equal(
    codingSessionMinimapGateLine(null),
    "No gate row signed during this turn",
  );
  assert.equal(
    codingSessionMinimapGateLine({
      passed: 2,
      failed: 1,
      notRun: 0,
      glyph: "fail",
      gates: [],
    }),
    "Gates: 2 passed, 1 failed",
  );
});

test("only an open decision.request is a ruling mark (DB8)", () => {
  // Signed seconds; the turns above are in ms, so 3.5 s lands in t3.
  const transactions = [
    // An open assignment: a seat working, not waiting on a person.
    { sourceEventId: "a1", type: "assignment", createdAt: 1.5 },
    // A report with no verdict yet: not a ruling request either.
    { sourceEventId: "r1", type: "report", createdAt: 2.5 },
    { sourceEventId: "d-open", type: "decision.request", createdAt: 3.5 },
    { sourceEventId: "d-done", type: "decision.request", createdAt: 1.2 },
    // Listed by no fold row: unknown, so not marked.
    { sourceEventId: "d-unknown", type: "decision.request", createdAt: 2.2 },
  ];
  const decisions = [
    { requestId: "d-open", answeredBy: null },
    { requestId: "d-done", answeredBy: "f".repeat(64) },
  ];
  const rulings = codingSessionMinimapOpenDecisionRulings({
    transactions,
    decisions,
  });
  assert.deepEqual(rulings, [{ sinceAtMs: 3_500 }]);
  const marks = deriveCodingSessionMinimapMarks({ turns, rulings });
  assert.equal(marks.get("t1").waitingRulings, 0);
  assert.equal(marks.get("t2").waitingRulings, 0);
  assert.equal(marks.get("t3").waitingRulings, 1);
});

test("an open assignment alone gives no ruling mark", () => {
  const rulings = codingSessionMinimapOpenDecisionRulings({
    transactions: [{ sourceEventId: "a1", type: "assignment", createdAt: 1.5 }],
    decisions: [],
  });
  assert.deepEqual(rulings, []);
  const marks = deriveCodingSessionMinimapMarks({ turns, rulings });
  for (const id of ["t1", "t2", "t3"]) {
    assert.equal(marks.get(id).waitingRulings, 0);
  }
});

test("the ruling source is null until both the fold and the request rows are read", () => {
  const decisions = [{ requestId: "r1", answeredBy: null }];
  assert.equal(codingSessionMinimapOpenDecisionSource(null), null);
  assert.equal(
    codingSessionMinimapOpenDecisionSource({ decisions: null }),
    null,
  );
  assert.equal(codingSessionMinimapOpenDecisionSource({ decisions }), null);
  const transactions = [
    { sourceEventId: "r1", type: "decision.request", createdAt: 2 },
  ];
  const source = codingSessionMinimapOpenDecisionSource({
    decisions,
    decisionRequests: transactions,
  });
  assert.deepEqual(source, { transactions, decisions });
  assert.deepEqual(codingSessionMinimapOpenDecisionRulings(source), [
    { sinceAtMs: 2_000 },
  ]);
});

test("an open ruling read from the ctx draws the amber mark on its turn", () => {
  const source = codingSessionMinimapOpenDecisionSource({
    decisions: [
      { requestId: "r1", answeredBy: null, answerId: null },
      { requestId: "r2", answeredBy: "npub", answerId: "a2" },
    ],
    decisionRequests: [
      { sourceEventId: "r1", type: "decision.request", createdAt: 2.5 },
      { sourceEventId: "r2", type: "decision.request", createdAt: 1.5 },
    ],
  });
  const marks = deriveCodingSessionMinimapMarks({
    turns,
    rulings: codingSessionMinimapOpenDecisionRulings(source),
  });
  assert.equal(marks.get("t1").waitingRulings, 0);
  assert.equal(marks.get("t2").waitingRulings, 1);
  assert.equal(marks.get("t3").waitingRulings, 0);
});

test("the ruling note says not read, unplaced, or nothing", () => {
  assert.equal(
    codingSessionMinimapRulingNote(null),
    "Open ruling requests are not read in this view",
  );
  assert.equal(
    codingSessionMinimapRulingNote({ decisions: null }),
    "Open ruling requests are not read in this view",
  );
  assert.equal(codingSessionMinimapRulingNote({ decisions: [] }), null);
  assert.equal(
    codingSessionMinimapRulingNote({
      decisions: [{ requestId: "r1", answeredBy: "x" }],
    }),
    null,
  );
  assert.equal(
    codingSessionMinimapRulingNote({
      decisions: [{ requestId: "r1", answeredBy: null }],
    }),
    "1 open ruling request is not marked: its signed request was not read in this view",
  );
  assert.equal(
    codingSessionMinimapRulingNote({
      decisions: [
        { requestId: "r1", answeredBy: null },
        { requestId: "r2", answeredBy: null },
      ],
      decisionRequests: [
        { sourceEventId: "r1", type: "decision.request", createdAt: 1 },
      ],
    }),
    "1 open ruling request is not marked: its signed request was not read in this view",
  );
  assert.equal(
    codingSessionMinimapRulingNote({
      decisions: [{ requestId: "r1", answeredBy: null }],
      decisionRequests: [
        { sourceEventId: "r1", type: "decision.request", createdAt: 1 },
      ],
    }),
    null,
  );
});

test("SV-29: a rewind marks the first turn that began at or after it", async () => {
  const { codingSessionMinimapRewinds } = await import(
    "./codingSessionTranscriptMinimapMarks.ts"
  );
  const rewinds = codingSessionMinimapRewinds([
    {
      id: "r1",
      type: "lifecycle",
      renderClass: "status",
      title: "Rewound",
      text: "Rewound to before this turn · files kept · new conversation seeded from the record",
      timestamp: new Date(2_500).toISOString(),
    },
    // An ordinary status row is not a rewind.
    {
      id: "s1",
      type: "lifecycle",
      renderClass: "status",
      title: "Status",
      text: "session_rewound",
      timestamp: new Date(1_200).toISOString(),
    },
  ]);
  assert.deepEqual(rewinds, [{ atMs: 2_500 }]);
  const marks = deriveCodingSessionMinimapMarks({ turns, rewinds });
  // Not the turn the rewind happened "during" (t2), but the one after it.
  assert.equal(marks.get("t2").rewound, false);
  assert.equal(marks.get("t3").rewound, true);
  assert.equal(marks.get("t1").rewound, false);
});

test("SV-29: a rewind after every turn, or undated, marks nothing", () => {
  const marks = deriveCodingSessionMinimapMarks({
    turns,
    rewinds: [{ atMs: 9_000 }, { atMs: null }],
  });
  for (const id of ["t1", "t2", "t3"]) {
    assert.equal(marks.get(id).rewound, false);
  }
});
