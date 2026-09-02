import assert from "node:assert/strict";
import test from "node:test";

import {
  buildCodingSessionWakeOperationIndex,
  codingSessionWakeReading,
  codingSessionWakeReadingForText,
  parseCodingSessionWakePointer,
} from "./codingSessionWakeReading.ts";
import { codingSessionTeamWakeText } from "./codingSessionTeamWake.ts";

// Live run 2, channel d3e440ea-89f8-4aee-8a02-17edc3e7272e: the first decision
// request and the founder's own answer to it, read off the wire.
const REQUEST_ID =
  "2099cdb3e076ccb9c51cb4526710c4d3d7a551ad6c4fda6e61176f4d47b601cb";
const ANSWER_ID =
  "4847ff0645533c1b0c2464d9aaad80ba1d51c5b74bc77bf495dd9e617fdd2bf3";
const ASSIGNMENT_ID =
  "3234382f094519e81c1a572ce1383516fe093b0a6b179100525d6f47909e2a9a";
const REPORT_ID =
  "ac89ace845ae898a85c12e6c93102efb086da0a16a4ede347ebe1d4f73bb8c38";
const VERDICT_ID =
  "c7bd7d0fe90e830bb32861deaa31832c522321f39bfb20b2712f2e6a30b0a4e4";
const ACK_ID =
  "ebe938a8ebe938a8ebe938a8ebe938a8ebe938a8ebe938a8ebe938a8ebe938a8";
const NOTE_ID =
  "1349de2b1349de2b1349de2b1349de2b1349de2b1349de2b1349de2b1349de2b";
const TERMINAL_ID = "9".repeat(64);
const ABSENT_ID = "0".repeat(64);

// The founder's answer as it actually reached the lead: a 44220 whose whole
// `action.text` is this object (command `cli-wake-v1:4847ff06…`).
const LIVE_ANSWER_WAKE = JSON.stringify({
  operationId: ANSWER_ID,
  type: "decision.answer",
});

const CHOSEN_OPTION =
  "C: role always present, null when not seated in a session";

function liveIndex() {
  return buildCodingSessionWakeOperationIndex({
    assignments: [
      {
        sourceEventId: ASSIGNMENT_ID,
        assigneeRole: "builder",
        objective: "Add `bee sessions whoami`",
      },
    ],
    transactions: [
      {
        sourceEventId: REQUEST_ID,
        authorPubkey: LEAD_KEY,
        type: "decision.request",
        parentEventId: null,
        summary: "Should the JSON also carry the seat role?",
      },
      {
        sourceEventId: ANSWER_ID,
        authorPubkey: FOUNDER_KEY,
        type: "decision.answer",
        parentEventId: REQUEST_ID,
        summary: CHOSEN_OPTION,
      },
      {
        sourceEventId: ASSIGNMENT_ID,
        authorPubkey: LEAD_KEY,
        type: "assignment",
        parentEventId: null,
        summary: "",
      },
      {
        sourceEventId: REPORT_ID,
        authorPubkey: BUILDER_KEY,
        type: "report",
        parentEventId: ASSIGNMENT_ID,
        summary: "bee sessions whoami ships",
      },
      {
        sourceEventId: VERDICT_ID,
        authorPubkey: LEAD_KEY,
        type: "disposition",
        parentEventId: REPORT_ID,
        summary: "APPROVE-WITH-NOTES",
      },
      {
        sourceEventId: ACK_ID,
        authorPubkey: BUILDER_KEY,
        type: "acknowledgement",
        parentEventId: VERDICT_ID,
        summary: "Disposition received.",
      },
      {
        sourceEventId: NOTE_ID,
        authorPubkey: LEAD_KEY,
        type: "note",
        parentEventId: null,
        summary: "Builder hired for lane W1.",
      },
      {
        sourceEventId: TERMINAL_ID,
        authorPubkey: LEAD_KEY,
        type: "mission.completed",
        parentEventId: null,
        summary: "Done.",
      },
    ],
  });
}

test("L2.1: the live 4847ff06 pointer reads as one line naming its request", () => {
  const line = codingSessionWakeReadingForText({
    operations: liveIndex(),
    signerPubkey: FOUNDER_KEY,
    text: LIVE_ANSWER_WAKE,
    who: "You",
  });
  assert.equal(line, `You answered decision 2099cdb3: ${CHOSEN_OPTION}`);
  assert.ok(line.startsWith("You answered decision 2099cdb3: C"));
  // The answer's own id is never shown in the request's place (§1f).
  assert.doesNotMatch(line, /4847ff06/);
});

test("L2.1: ordinary prose is not a wake and is never re-read", () => {
  for (const prose of [
    "LANE W1 — one correction before I rule; everything else is accepted.",
    "",
    "   ",
    "{not json",
    JSON.stringify({ operationId: ANSWER_ID }),
    JSON.stringify({ operationId: ANSWER_ID, type: "note", extra: 1 }),
    JSON.stringify({ operationId: "4847ff06", type: "note" }),
    JSON.stringify([{ operationId: ANSWER_ID, type: "note" }]),
  ]) {
    assert.equal(parseCodingSessionWakePointer(prose), null, prose);
    assert.equal(
      codingSessionWakeReadingForText({
        operations: liveIndex(),
        signerPubkey: FOUNDER_KEY,
        text: prose,
        who: "You",
      }),
      null,
      prose,
    );
  }
});

test("L2.1: a pointer naming an operation the fold does not hold guesses nothing", () => {
  const line = codingSessionWakeReadingForText({
    operations: liveIndex(),
    signerPubkey: LEAD_KEY,
    text: JSON.stringify({ operationId: ABSENT_ID, type: "decision.answer" }),
    who: "Keystone",
  });
  assert.equal(
    line,
    "Keystone sent a wake for operation 00000000 (decision.answer) — not in this session's records yet",
  );
});

test("L2.1: every frozen §1f row reads its own sentence", () => {
  const operations = liveIndex();
  const read = (operationId, type, who = "You", signerPubkey = FOUNDER_KEY) =>
    codingSessionWakeReading({
      operations,
      pointer: { kind: "operation", operationId, type },
      signerPubkey,
      who,
    });
  assert.equal(
    read(ASSIGNMENT_ID, "assignment", "You", LEAD_KEY),
    "You assigned builder: Add `bee sessions whoami`",
  );
  assert.equal(
    read(REPORT_ID, "report", "Bob", BUILDER_KEY),
    "Bob reported on assignment 3234382f",
  );
  assert.equal(
    read(VERDICT_ID, "verdict", "Keystone", LEAD_KEY),
    "Keystone ruled on report ac89ace8",
  );
  assert.equal(
    read(ACK_ID, "acknowledgement", "Bob", BUILDER_KEY),
    "Bob acknowledged verdict c7bd7d0f",
  );
  assert.equal(
    read(TERMINAL_ID, "mission.completed", "Keystone", LEAD_KEY),
    "Keystone closed the mission as completed",
  );
  assert.equal(
    read(NOTE_ID, "note", "Keystone", LEAD_KEY),
    "Keystone left a note",
  );
  assert.equal(
    read(REQUEST_ID, "decision.request", "Keystone", LEAD_KEY),
    "Keystone asked decision 2099cdb3: Should the JSON also carry the seat role?",
  );
});

test("L2.1: a terminal wake reads without needing the fold at all", () => {
  const text = codingSessionTeamWakeText({
    kind: "turn_ended_without_required_operation",
    sourceEventId: TERMINAL_ID,
    seatRole: "builder",
    causedByCommandId: null,
    sourceCreatedAtMs: 0,
    sourceEventSeq: null,
    sourceTargetKey: null,
    operationType: null,
  });
  assert.equal(
    codingSessionWakeReadingForText({
      operations: new Map(),
      signerPubkey: LEAD_KEY,
      text,
      who: "Keystone",
    }),
    "Keystone woke builder for turn_ended_without_required_operation",
  );
});

test("L2.1: the reading is byte-identical to what `codingSessionTeamWakeText` mints", () => {
  const text = codingSessionTeamWakeText({
    kind: "operation_ready",
    sourceEventId: ANSWER_ID,
    operationType: "decision.answer",
    sourceCreatedAtMs: 0,
    sourceEventSeq: null,
    sourceTargetKey: null,
    seatRole: null,
    causedByCommandId: null,
  });
  assert.equal(text, LIVE_ANSWER_WAKE);
  assert.deepEqual(parseCodingSessionWakePointer(text), {
    kind: "operation",
    operationId: ANSWER_ID,
    type: "decision.answer",
  });
});

test("L2.1: a quoted subject is bounded, never a paragraph in a bubble", () => {
  const long = "x".repeat(400);
  const operations = buildCodingSessionWakeOperationIndex({
    assignments: [],
    transactions: [
      {
        sourceEventId: ANSWER_ID,
        authorPubkey: FOUNDER_KEY,
        type: "decision.answer",
        parentEventId: REQUEST_ID,
        summary: long,
      },
    ],
  });
  const line = codingSessionWakeReadingForText({
    operations,
    signerPubkey: FOUNDER_KEY,
    text: LIVE_ANSWER_WAKE,
    who: "You",
  });
  assert.ok(line.length < 200, line.length);
  assert.ok(line.endsWith("…"));
});

// ── Fix round 1: F2 (author join), F3 (type join), §1f lens amendment ───────

const MALLORY = "cc".repeat(32);
const FOUNDER_KEY = "3d3b7169".repeat(8);
const LEAD_KEY = "ede63017".repeat(8);
const BUILDER_KEY = "1ddd35c6".repeat(8);

function authoredIndex() {
  return buildCodingSessionWakeOperationIndex({
    assignments: [
      {
        sourceEventId: ASSIGNMENT_ID,
        assigneeRole: "builder",
        objective: "Add `bee sessions whoami`",
      },
    ],
    transactions: [
      {
        sourceEventId: REQUEST_ID,
        type: "decision.request",
        authorPubkey: LEAD_KEY,
        parentEventId: null,
        summary: "Should the JSON also carry the seat role?",
      },
      {
        sourceEventId: ANSWER_ID,
        type: "decision.answer",
        authorPubkey: FOUNDER_KEY,
        parentEventId: REQUEST_ID,
        summary: CHOSEN_OPTION,
      },
      {
        sourceEventId: REPORT_ID,
        type: "report",
        authorPubkey: BUILDER_KEY,
        parentEventId: ASSIGNMENT_ID,
        summary: "bee sessions whoami ships",
      },
      {
        sourceEventId: VERDICT_ID,
        type: "disposition",
        authorPubkey: LEAD_KEY,
        parentEventId: REPORT_ID,
        summary: "APPROVE-WITH-NOTES",
      },
    ],
  });
}

test("F2: a member's turn carrying somebody else's pointer never reads as their act", () => {
  // The reviewer's Mallory case: Mallory types the founder's own
  // `decision.answer` pointer into the composer. Her 44220 is signed by her;
  // the record it names is signed by the founder.
  const line = codingSessionWakeReadingForText({
    operations: authoredIndex(),
    signerPubkey: MALLORY,
    text: LIVE_ANSWER_WAKE,
    who: "Mallory",
  });
  assert.equal(
    line,
    "Mallory sent a wake for operation 4847ff06 (decision.answer) — signed by Mallory, not the operation's author",
  );
  assert.doesNotMatch(line, /answered decision/);
});

test("F2: the operation's own author still reads as the act", () => {
  assert.equal(
    codingSessionWakeReadingForText({
      operations: authoredIndex(),
      signerPubkey: FOUNDER_KEY,
      text: LIVE_ANSWER_WAKE,
      who: "You",
    }),
    `You answered decision 2099cdb3: ${CHOSEN_OPTION}`,
  );
  // Case is not identity: a signer spelled in another case is the same key.
  assert.equal(
    codingSessionWakeReadingForText({
      operations: authoredIndex(),
      signerPubkey: FOUNDER_KEY.toUpperCase(),
      text: LIVE_ANSWER_WAKE,
      who: "You",
    }),
    `You answered decision 2099cdb3: ${CHOSEN_OPTION}`,
  );
});

test("F2: an unknown signer is not an author match", () => {
  const line = codingSessionWakeReadingForText({
    operations: authoredIndex(),
    signerPubkey: null,
    text: LIVE_ANSWER_WAKE,
    who: "Operator not recorded",
  });
  assert.match(line, /not the operation's author$/);
});

test("F3: a pointer whose type disagrees with the record names nothing", () => {
  // The reviewer's A1d: a report's id carried under `type: "verdict"` used to
  // render `You ruled on report <the assignment's 8hex>`, confidently wrong.
  const line = codingSessionWakeReadingForText({
    operations: authoredIndex(),
    signerPubkey: BUILDER_KEY,
    text: JSON.stringify({ operationId: REPORT_ID, type: "verdict" }),
    who: "Bob",
  });
  assert.equal(
    line,
    "Bob sent a wake for operation ac89ace8 (verdict) — the record is a report, not a verdict",
  );
  assert.doesNotMatch(line, /ruled on report/);
});

test("F3: `verdict` still matches its two signed subtypes", () => {
  assert.equal(
    codingSessionWakeReadingForText({
      operations: authoredIndex(),
      signerPubkey: LEAD_KEY,
      text: JSON.stringify({ operationId: VERDICT_ID, type: "verdict" }),
      who: "Keystone",
    }),
    "Keystone ruled on report ac89ace8",
  );
});

test("§1f amendment: a lens holding no records says so, and points at Mission", () => {
  assert.equal(
    codingSessionWakeReadingForText({
      operations: new Map(),
      signerPubkey: FOUNDER_KEY,
      text: LIVE_ANSWER_WAKE,
      who: "You",
    }),
    "You sent a wake for operation 4847ff06 — this lens holds no session records; open Mission to read it.",
  );
  // A lens that DOES hold records keeps §1f's original sentence.
  assert.equal(
    codingSessionWakeReadingForText({
      operations: authoredIndex(),
      signerPubkey: FOUNDER_KEY,
      text: JSON.stringify({ operationId: ABSENT_ID, type: "decision.answer" }),
      who: "You",
    }),
    "You sent a wake for operation 00000000 (decision.answer) — not in this session's records yet",
  );
});
