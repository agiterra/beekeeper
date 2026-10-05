import assert from "node:assert/strict";
import { test } from "node:test";

import {
  findGenesis,
  SESSION_OWNED_KINDS,
  selectionHasGenesis,
  sessionOwnedEventIds,
} from "./deleteCodingSession.ts";

// Every fixture below is the shape a real relay returns, copied from live
// events on a local relay. The first version of this file assumed one `d`
// tag grouped every kind; that is true for four of them and false for the
// three that matter most, and the product refused every real session with
// "no genesis event" as a result. These fixtures exist so that cannot
// silently come back.

const SESSION_A = "d9c338f4-85ec-4d61-aac6-9ffbe5b1bf42";
const SESSION_B = "6b0fbc42-dc5c-4911-be06-75dc8ffb4349";
const CHANNEL = "9422dafe-618d-4563-b33d-f2f634683e28";
const TARGET_A =
  "coding-session/v1|16:claude-agent-acp16:b051835ed3dc579836:ea97f1ab-dd38-47f5-bd86-a293bc4153ba1:1";
const TARGET_B =
  "coding-session/v1|16:claude-agent-acp16:b051835ed3dc579836:99999999-dd38-47f5-bd86-a293bc4153ba1:1";

const base = { pubkey: "f".repeat(64), created_at: 1 };

/** kind:44226 — no `d` tag; sessionRef in `csg-session` and in content. */
function genesis(id, sessionRef) {
  return {
    ...base,
    id,
    kind: 44226,
    content: JSON.stringify({ sessionRef, v: 1 }),
    tags: [
      ["h", CHANNEL],
      ["csg-v", "csg1-1"],
      ["csg-session", sessionRef],
    ],
  };
}

/** kind:44223 — tags carry only `cs-target`; sessionRef lives in content. */
function metadata(id, sessionRef, target) {
  return {
    ...base,
    id,
    kind: 44223,
    content: JSON.stringify({
      schema: "buzz-coding-session-metadata/v1",
      sessionRef,
      status: "stopped",
    }),
    tags: [
      ["h", CHANNEL],
      ["csm-v", "csm1-1"],
      ["cs-target", target],
      ["csm-key", `coding-session-metadata/v1|${target}`],
    ],
  };
}

/** kind:44225 — `cs-target` only. Nothing on it names the umbrella. */
function transcript(id, target, seq) {
  return {
    ...base,
    id,
    kind: 44225,
    content: "{}",
    tags: [
      ["h", CHANNEL],
      ["cst-v", "cst1-1"],
      ["cs-target", target],
      ["cst-seq", String(seq)],
    ],
  };
}

/** kind:44231 — the NIP-CSCK envelope; reaches its umbrella by `cs-target`. */
function checkpoint(id, target, seq) {
  return {
    ...base,
    id,
    kind: 44231,
    content: "{}",
    tags: [
      ["h", CHANNEL],
      ["csck-v", "csck1-1"],
      ["cs-target", target],
      ["csck-seq", String(seq)],
      ["csck-key", `coding-session-checkpoint/v1|${target}|turn|${seq}`],
    ],
  };
}

/** kinds 44227 / 44229 / 44230 / 44244 — plain `d` tag. */
function dTagged(id, kind, sessionRef) {
  return {
    ...base,
    id,
    kind,
    content: "",
    tags: [
      ["h", CHANNEL],
      ["d", sessionRef],
    ],
  };
}

test("the genesis is found by its canonical event id", () => {
  // NIP-CSG: the `csg-session` tag exists for the relay's uniqueness probe
  // and diagnostics, and consumers are told not to select by it.
  const events = [genesis("g-a", SESSION_A), genesis("g-b", SESSION_B)];
  assert.equal(findGenesis(events, SESSION_A, "g-a")?.id, "g-a");
  assert.equal(selectionHasGenesis(events, SESSION_A, "g-a"), true);
});

test("without a genesisRef the genesis is found by its content claim", () => {
  // The CLI is given only a sessionRef. It reads the payload, never the tag.
  const events = [genesis("g-a", SESSION_A)];
  assert.equal(findGenesis(events, SESSION_A, null)?.id, "g-a");
  assert.equal(findGenesis(events, SESSION_B, null), null);
});

test("a real session with no `d` tag on its genesis is still deletable", () => {
  // The exact regression: a genesis carries no `d` tag at all, so grouping
  // everything by `d` found none and refused every real session.
  const events = [genesis("g-a", SESSION_A)];
  assert.equal(selectionHasGenesis(events, SESSION_A, "g-a"), true);
  assert.deepEqual(sessionOwnedEventIds(events, SESSION_A, "g-a"), ["g-a"]);
});

test("the transcript is reached through its execution's metadata", () => {
  // Two hops: metadata names both the umbrella (content) and the execution
  // (`cs-target`); the transcript names only the execution.
  const events = [
    genesis("g-a", SESSION_A),
    metadata("m-a", SESSION_A, TARGET_A),
    transcript("t-1", TARGET_A, 1),
    transcript("t-2", TARGET_A, 2),
  ];
  assert.deepEqual(sessionOwnedEventIds(events, SESSION_A, "g-a"), [
    "g-a",
    "m-a",
    "t-1",
    "t-2",
  ]);
});

test("a transcript whose execution belongs to another session is left alone", () => {
  const events = [
    genesis("g-a", SESSION_A),
    metadata("m-a", SESSION_A, TARGET_A),
    metadata("m-b", SESSION_B, TARGET_B),
    transcript("t-1", TARGET_A, 1),
    transcript("t-9", TARGET_B, 1),
  ];
  const ids = sessionOwnedEventIds(events, SESSION_A, "g-a");
  assert.ok(ids.includes("t-1"));
  assert.ok(!ids.includes("t-9"), "another umbrella's transcript must not go");
  assert.ok(!ids.includes("m-b"));
});

test("an orphan transcript with no metadata is not swept in", () => {
  // Nothing on the wire ties it to this umbrella, so nothing here may claim
  // it does.
  const events = [genesis("g-a", SESSION_A), transcript("t-1", TARGET_A, 1)];
  assert.deepEqual(sessionOwnedEventIds(events, SESSION_A, "g-a"), ["g-a"]);
});

test("goal, name, closure and team transactions match on the d tag", () => {
  const events = [
    genesis("g-a", SESSION_A),
    dTagged("goal", 44227, SESSION_A),
    dTagged("name", 44229, SESSION_A),
    dTagged("close", 44230, SESSION_A),
    dTagged("txn", 44244, SESSION_A),
    dTagged("other", 44230, SESSION_B),
  ];
  const ids = sessionOwnedEventIds(events, SESSION_A, "g-a");
  assert.deepEqual(ids, ["close", "g-a", "goal", "name", "txn"]);
  assert.ok(!ids.includes("other"));
});

test("a generated title goes with its session, and only its own", () => {
  // 44252 summarises the person's first message. The relay admits it to a
  // whole-session delete by its `d` tag; the client must name it, or the
  // summary outlives the session it describes.
  assert.ok(SESSION_OWNED_KINDS.includes(44252));
  const events = [
    genesis("g-a", SESSION_A),
    dTagged("title-a", 44252, SESSION_A),
    dTagged("title-b", 44252, SESSION_B),
  ];
  assert.deepEqual(sessionOwnedEventIds(events, SESSION_A, "g-a"), [
    "g-a",
    "title-a",
  ]);
});

test("undecodable content is left alone rather than guessed at", () => {
  const broken = {
    ...base,
    id: "m-x",
    kind: 44223,
    content: "not json",
    tags: [
      ["h", CHANNEL],
      ["cs-target", TARGET_A],
    ],
  };
  const events = [genesis("g-a", SESSION_A), broken];
  assert.deepEqual(sessionOwnedEventIds(events, SESSION_A, "g-a"), ["g-a"]);
});

test("the selection is sorted and de-duplicated", () => {
  const events = [
    dTagged("close", 44230, SESSION_A),
    genesis("g-a", SESSION_A),
    dTagged("close", 44230, SESSION_A),
  ];
  assert.deepEqual(sessionOwnedEventIds(events, SESSION_A, "g-a"), [
    "close",
    "g-a",
  ]);
});

test("a kind the session does not own is never selected", () => {
  // A chat message with a matching `d` tag must not ride in on the
  // authorship exemption a session delete carries.
  const message = {
    ...base,
    id: "msg",
    kind: 40002,
    content: "",
    tags: [
      ["h", CHANNEL],
      ["d", SESSION_A],
    ],
  };
  const events = [genesis("g-a", SESSION_A), message];
  assert.deepEqual(sessionOwnedEventIds(events, SESSION_A, "g-a"), ["g-a"]);
});

test("turn checkpoints are fetched and deleted with their session", () => {
  // A checkpoint names repo paths, branches and head SHAs. A session reported
  // as deleted must not leave them on the relay, and it can only go if the
  // fetch asks for the kind at all.
  assert.ok(SESSION_OWNED_KINDS.includes(44231));
  const events = [
    genesis("g-a", SESSION_A),
    metadata("m-a", SESSION_A, TARGET_A),
    metadata("m-b", SESSION_B, TARGET_B),
    transcript("t-1", TARGET_A, 1),
    checkpoint("ck-1", TARGET_A, 1),
    checkpoint("ck-9", TARGET_B, 1),
  ];
  const ids = sessionOwnedEventIds(events, SESSION_A, "g-a");
  assert.deepEqual(ids, ["ck-1", "g-a", "m-a", "t-1"]);
  assert.ok(!ids.includes("ck-9"), "another umbrella's checkpoint must not go");
});

test("an orphan checkpoint with no metadata is not swept in", () => {
  const events = [genesis("g-a", SESSION_A), checkpoint("ck-1", TARGET_A, 1)];
  assert.deepEqual(sessionOwnedEventIds(events, SESSION_A, "g-a"), ["g-a"]);
});
