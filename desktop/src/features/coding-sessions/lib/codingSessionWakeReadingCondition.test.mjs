/**
 * A ruling that names a class reads that class where the ruling is read.
 *
 * A separate file from `codingSessionWakeReading.test.mjs` on purpose: another
 * lane owns that suite in this batch, and a new file cannot collide with it.
 */
import assert from "node:assert/strict";
import { test } from "node:test";

import {
  buildCodingSessionWakeOperationIndex,
  codingSessionWakeReading,
  MAX_CODING_SESSION_WAKE_READING_SUBJECT_CHARS,
} from "./codingSessionWakeReading.ts";

// Live run 2, channel d3e440ea-89f8-4aee-8a02-17edc3e7272e: the founder's own
// answer, and the request it answered.
const REQUEST_ID =
  "2099cdb3e076ccb9c51cb4526710c4d3d7a551ad6c4fda6e61176f4d47b601cb";
const ANSWER_ID =
  "4847ff0645533c1b0c2464d9aaad80ba1d51c5b74bc77bf495dd9e617fdd2bf3";
const FOUNDER_KEY = "3d".repeat(32);

function index(condition) {
  return buildCodingSessionWakeOperationIndex({
    assignments: [],
    transactions: [
      {
        sourceEventId: REQUEST_ID,
        authorPubkey: FOUNDER_KEY,
        type: "decision.request",
        parentEventId: null,
        summary: "Push with --no-verify, or hold?",
      },
      {
        sourceEventId: ANSWER_ID,
        authorPubkey: FOUNDER_KEY,
        type: "decision.answer",
        parentEventId: REQUEST_ID,
        summary: "push with --no-verify",
        condition,
      },
    ],
  });
}

function read(operations) {
  return codingSessionWakeReading({
    operations,
    pointer: {
      kind: "operation",
      operationId: ANSWER_ID,
      type: "decision.answer",
    },
    signerPubkey: FOUNDER_KEY,
    who: "You",
  });
}

test("an answer that named a class reads the class it covers", () => {
  const condition =
    "any SHA whose beekeeper-acp diff against origin/main is empty";
  assert.equal(
    read(index(condition)),
    `You answered decision 2099cdb3: push with --no-verify — condition: ${condition}`,
  );
});

test("an answer that named no class reads exactly as it did before", () => {
  // §1f's row, byte-for-byte: the suffix exists only when a condition does.
  const before = "You answered decision 2099cdb3: push with --no-verify";
  assert.equal(read(index(null)), before);
  assert.equal(read(index(undefined)), before);
  // A blank condition is not a class; it is refused at the wire and, if one
  // ever reached this surface, it is not rendered as one.
  assert.equal(read(index("   ")), before);
});

test("a long condition is elided at the wake reading's own bound", () => {
  const line = read(index("x".repeat(400)));
  assert.ok(line.includes("— condition: "));
  assert.ok(line.endsWith("…"));
  // The bound this is about, not a loose upper limit (REVIEW-L7 §7): the
  // condition is clamped to MAX_CODING_SESSION_WAKE_READING_SUBJECT_CHARS,
  // ellipsis included, exactly as every other quoted string in a wake line.
  const condition = line.slice(
    line.indexOf("— condition: ") + "— condition: ".length,
  );
  assert.equal(condition.length, MAX_CODING_SESSION_WAKE_READING_SUBJECT_CHARS);
  assert.equal(
    condition,
    `${"x".repeat(MAX_CODING_SESSION_WAKE_READING_SUBJECT_CHARS - 1)}…`,
  );
});
