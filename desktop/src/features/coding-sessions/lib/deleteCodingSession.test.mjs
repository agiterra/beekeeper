import assert from "node:assert/strict";
import { test } from "node:test";

import {
  selectionHasGenesis,
  sessionOwnedEventIds,
} from "./deleteCodingSession.ts";

const SESSION_A = "5b7e1c2a-90d4-4b0e-a1f3-7c2d8e6f4a10";
const SESSION_B = "5b7e1c2a-90d4-4b0e-a1f3-7c2d8e6f4a11";

function owned(id, kind, sessionRef) {
  return {
    id,
    kind,
    pubkey: "f".repeat(64),
    created_at: 1,
    content: "",
    tags: [
      ["h", "chan"],
      ["d", sessionRef],
    ],
  };
}

test("the selection takes the whole chain of one session", () => {
  // The relay refuses a chain that leaves any live closure behind, so a
  // selection that dropped one would produce a refusal, not a partial delete.
  const events = [
    owned("a1", 44226, SESSION_A),
    owned("a2", 44230, SESSION_A),
    owned("a3", 44230, SESSION_A),
    owned("a4", 44225, SESSION_A),
    owned("a5", 44223, SESSION_A),
    owned("a6", 44227, SESSION_A),
    owned("a7", 44229, SESSION_A),
    owned("a8", 44244, SESSION_A),
  ];
  assert.equal(sessionOwnedEventIds(events, SESSION_A).length, 8);
});

test("the selection never reaches a neighbouring session", () => {
  // A channel holds many sessions. The relay would accept a deletion naming a
  // neighbour's events — same channel, and a genesis is present — so this
  // boundary is the client's to hold.
  const events = [
    owned("a1", 44226, SESSION_A),
    owned("b1", 44226, SESSION_B),
    owned("b2", 44225, SESSION_B),
  ];
  assert.deepEqual(sessionOwnedEventIds(events, SESSION_A), ["a1"]);
});

test("the selection ignores kinds a session does not own", () => {
  // Nothing stops a client publishing a chat message with a matching `d` tag.
  // It must not ride in on the authorship exemption a session delete carries.
  const events = [
    owned("a1", 44226, SESSION_A),
    owned("m1", 40002, SESSION_A),
    owned("c1", 44220, SESSION_A),
  ];
  assert.deepEqual(sessionOwnedEventIds(events, SESSION_A), ["a1"]);
});

test("the selection is sorted and de-duplicated", () => {
  const events = [
    owned("a2", 44230, SESSION_A),
    owned("a1", 44226, SESSION_A),
    owned("a2", 44230, SESSION_A),
  ];
  assert.deepEqual(sessionOwnedEventIds(events, SESSION_A), ["a1", "a2"]);
});

test("an event with no session ref is never selected", () => {
  const events = [
    {
      id: "x1",
      kind: 44225,
      pubkey: "f".repeat(64),
      created_at: 1,
      content: "",
      tags: [["h", "chan"]],
    },
  ];
  assert.deepEqual(sessionOwnedEventIds(events, SESSION_A), []);
});

test("a selection with no genesis is not a deletable session", () => {
  // Without a genesis the relay has nothing to authorize against and refuses
  // every target as "must be event author".
  const events = [owned("a4", 44225, SESSION_A)];
  assert.equal(selectionHasGenesis(events, SESSION_A), false);
  assert.equal(
    selectionHasGenesis([owned("a1", 44226, SESSION_A)], SESSION_A),
    true,
  );
});

test("a genesis for another session does not make this one deletable", () => {
  assert.equal(
    selectionHasGenesis([owned("b1", 44226, SESSION_B)], SESSION_A),
    false,
  );
});
