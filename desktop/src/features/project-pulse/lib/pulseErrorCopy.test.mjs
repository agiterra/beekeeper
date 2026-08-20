/**
 * The wire→English seam. Every string the fold can record has to come out of
 * here as a sentence with no 64-hex id in it, and with the verbatim record
 * still reachable in the `title`.
 */
import assert from "node:assert/strict";
import { test } from "node:test";

import { summarizePulseErrors } from "./pulseErrorCopy.ts";

const NOW = 1_785_513_037;
const ALICE = "a1".repeat(32);
const id = (suffix) => suffix.padStart(64, "0");
const HEX = /\b[0-9a-f]{64}\b/i;

function entry(overrides = {}) {
  return {
    eventId: id("e1"),
    pubkey: ALICE,
    createdAt: NOW - 3_600,
    type: "plan",
    text: "First pass at the wire contract.",
    claimedAreas: [],
    branch: null,
    sessionRef: null,
    supersedes: null,
    supersededBy: [],
    active: true,
    ...overrides,
  };
}

const context = (entries = []) => ({
  entriesById: new Map(entries.map((row) => [row.eventId, row])),
  authorNames: new Map([[ALICE.toLowerCase(), "Alice"]]),
  nowSeconds: NOW,
});

test("every scope the fold can record renders without a hash", () => {
  const errors = [
    { scope: "entries", message: "entry read truncated at 500 events" },
    { scope: "entries", message: "relay unavailable" },
    {
      scope: "channels",
      message:
        "the project's channel set could not be read; session facts were not queried",
    },
    { scope: "sessions", message: "session read truncated at 1000 events" },
    { scope: "sessions", message: "fetch failed" },
    {
      scope: "invalid-entry",
      message: `entry ${id("71")} failed validation and was excluded`,
    },
    {
      scope: "invalid-event",
      message: `event ${id("c3")} (kind 44223) failed signature validation and was excluded`,
    },
    {
      scope: "unresolved-supersedes",
      message: `entry ${id("e1")} supersedes ${id("ff")}, which is not in the visible result set`,
    },
    // A scope this module has never seen is still shown — hiding it would be
    // the dishonesty these cards exist to prevent — with its ids redacted.
    { scope: "future-scope", message: `event ${id("aa")} did something new` },
  ];
  const notes = summarizePulseErrors(errors, context([entry()]));
  assert.equal(notes.length, errors.length);
  for (const [index, note] of notes.entries()) {
    assert.doesNotMatch(note.sentence, HEX, note.sentence);
    assert.match(note.sentence, /[.!?…]$/, note.sentence);
    assert.equal(
      note.title,
      `${errors[index].scope}: ${errors[index].message}`,
    );
  }
  assert.match(notes[8].sentence, /^An event did something new\.$/);
});

test("a known claimant is named by its own words and its author", () => {
  const [note] = summarizePulseErrors(
    [
      {
        scope: "unresolved-supersedes",
        message: `entry ${id("e1")} supersedes ${id("ff")}, which is not in the visible result set`,
      },
    ],
    context([entry()]),
  );
  assert.equal(
    note.sentence,
    "Alice's “First pass at the wire contract.” (Plan · 1h ago) says an entry " +
      "that is not visible in this read is resolved — nothing was replaced.",
  );
});

test("a claimant outside the visible set still gets a true sentence", () => {
  const [note] = summarizePulseErrors(
    [
      {
        scope: "unresolved-supersedes",
        message: `entry ${id("e9")} supersedes ${id("ff")}, which is not in the visible result set`,
      },
    ],
    context([]),
  );
  assert.equal(
    note.sentence,
    "An entry says another entry, not visible in this read, is resolved — nothing was replaced.",
  );
});

test("repeats collapse to one counted sentence that keeps every record", () => {
  const notes = summarizePulseErrors(
    [
      {
        scope: "invalid-event",
        message: `event ${id("c1")} (kind 44223) carried undecodable coding-session metadata and was excluded`,
      },
      {
        scope: "invalid-event",
        message: `event ${id("c2")} (kind 44223) carried undecodable coding-session metadata and was excluded`,
      },
      {
        scope: "invalid-event",
        message: `event ${id("c3")} (kind 44223) failed signature validation and was excluded`,
      },
    ],
    context([]),
  );
  assert.equal(notes.length, 2);
  assert.equal(
    notes[0].sentence,
    "2 coding-session updates were left out — their session details could not be read.",
  );
  assert.ok(notes[0].title.includes(id("c1")));
  assert.ok(notes[0].title.includes(id("c2")));
  assert.equal(
    notes[1].sentence,
    "A coding-session update was left out — its signature did not check out.",
  );
});

test("an error is never dropped on the way to a sentence", () => {
  const errors = Array.from({ length: 5 }, (_, index) => ({
    scope: "invalid-entry",
    message: `entry ${id(`7${index}`)} failed validation and was excluded`,
  }));
  const [note] = summarizePulseErrors(errors, context([]));
  assert.equal(
    note.sentence,
    "5 entries were left out — they did not pass validation.",
  );
  assert.equal(note.title.split("\n").length, 5);
});
