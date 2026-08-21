import assert from "node:assert/strict";
import { test } from "node:test";

import {
  remoteTerminalsFromEvents,
  rosterFromAnnounce,
} from "./useProjectTerminals.ts";

const OWNER = "feedface".repeat(8);
const ME = "deadbeef".repeat(8);
const PROJECT = `30621:${ME}:platform`;

function announce(overrides = {}) {
  const {
    sessionId = "sess-1",
    status = "open",
    project = PROJECT,
    title = "build shell",
    pubkey = OWNER,
    createdAt = 100,
  } = overrides;
  return {
    id: "x",
    pubkey,
    created_at: createdAt,
    kind: 30623,
    tags: [
      ["d", sessionId],
      ["a", project],
      ["title", title],
      ["status", status],
      ["dims", "24x80"],
    ],
    content: "",
    sig: "00",
  };
}

test("keeps only open announces for the project, excluding my own", () => {
  const terminals = remoteTerminalsFromEvents(
    [
      announce(),
      announce({ sessionId: "closed", status: "closed" }),
      announce({ sessionId: "other-proj", project: `30621:${ME}:other` }),
      announce({ sessionId: "mine", pubkey: ME }),
    ],
    PROJECT,
    ME,
  );
  assert.deepEqual(
    terminals.map((t) => t.sessionId),
    ["sess-1"],
  );
  assert.equal(terminals[0].ownerPubkey, OWNER);
  assert.equal(terminals[0].title, "build shell");
  assert.equal(terminals[0].projectRef, PROJECT);
  assert.equal(terminals[0].dims, "24x80");
});

test("sorts by title and tolerates missing optional tags", () => {
  const bare = announce({ sessionId: "bare", title: "zzz" });
  bare.tags = bare.tags.filter((t) => t[0] !== "dims");
  const terminals = remoteTerminalsFromEvents(
    [bare, announce({ sessionId: "a", title: "aaa" })],
    PROJECT,
    null,
  );
  assert.deepEqual(
    terminals.map((t) => t.sessionId),
    ["a", "bare"],
  );
  assert.equal(terminals[1].dims, null);
});

test("parses arity-4 p-tag rosters, skipping malformed entries", () => {
  const collab = "ab".repeat(32);
  const viewer = "cd".repeat(32);
  const event = announce();
  event.tags.push(
    ["p", collab, "", "collaborator"],
    ["p", viewer, "", "viewer"],
    // Malformed: arity-2 (no role), bad role, non-hex pubkey — all skipped.
    ["p", "ef".repeat(32)],
    ["p", "12".repeat(32), "", "admin"],
    ["p", "not-hex", "", "viewer"],
  );
  assert.deepEqual(rosterFromAnnounce(event), [
    { pubkey: collab, role: "collaborator" },
    { pubkey: viewer, role: "viewer" },
  ]);
  const terminals = remoteTerminalsFromEvents([event], PROJECT, null);
  assert.equal(terminals[0].roster.length, 2);
});

test("ignores events of other kinds or malformed shape", () => {
  const wrongKind = { ...announce(), kind: 30617 };
  const noSession = announce();
  noSession.tags = noSession.tags.filter((t) => t[0] !== "d");
  const terminals = remoteTerminalsFromEvents(
    [wrongKind, noSession],
    PROJECT,
    null,
  );
  assert.deepEqual(terminals, []);
});
