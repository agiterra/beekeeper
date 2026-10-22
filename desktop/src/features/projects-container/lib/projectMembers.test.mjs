import assert from "node:assert/strict";
import { mock, test } from "node:test";

import { relayClient } from "@/shared/api/relayClient";
import {
  buildPutRosterTags,
  buildRemoveRosterTags,
  fetchProjectRoster,
  parseRosterEventMembers,
  rosterWithOwner,
} from "./projectMembers.ts";

const OWNER = "a".repeat(64);
const ALICE = "b".repeat(64);
const BOB = "c".repeat(64);
const ADDRESS = `30621:${OWNER}:platform`;

function makeRosterEvent(tags) {
  return {
    id: "roster-event",
    pubkey: "f".repeat(64),
    kind: 39010,
    created_at: 100,
    content: "",
    tags: [["d", ADDRESS], ...tags],
  };
}

test("parseRosterEventMembers reads arity-4 role tags", () => {
  const members = parseRosterEventMembers(
    makeRosterEvent([
      ["p", ALICE, "", "owner"],
      ["p", BOB, "", "viewer"],
    ]),
  );
  assert.deepEqual(members, [
    { pubkey: ALICE, role: "owner" },
    { pubkey: BOB, role: "viewer" },
  ]);
});

test("parseRosterEventMembers defaults legacy role-less tags to collaborator", () => {
  const members = parseRosterEventMembers(makeRosterEvent([["p", ALICE]]));
  assert.deepEqual(members, [{ pubkey: ALICE, role: "collaborator" }]);
});

test("parseRosterEventMembers falls back to collaborator for unknown roles", () => {
  const members = parseRosterEventMembers(
    makeRosterEvent([["p", ALICE, "", "superadmin"]]),
  );
  assert.deepEqual(members, [{ pubkey: ALICE, role: "collaborator" }]);
});

test("parseRosterEventMembers drops malformed pubkeys and dedups", () => {
  const members = parseRosterEventMembers(
    makeRosterEvent([
      ["p", "not-a-pubkey", "", "owner"],
      ["p", ALICE.toUpperCase(), "", "viewer"],
      ["p", ALICE, "", "owner"],
    ]),
  );
  // Uppercase normalizes to lowercase; the duplicate keeps the first entry.
  assert.deepEqual(members, [{ pubkey: ALICE, role: "viewer" }]);
});

test("put tags round-trip through the roster parser", () => {
  const members = [
    { pubkey: ALICE, role: "owner" },
    { pubkey: BOB, role: "collaborator" },
  ];
  const tags = buildPutRosterTags(ADDRESS, members);
  assert.deepEqual(tags[0], ["a", ADDRESS]);
  // Arity exactly 4 with an empty relay hint, per the kind:9010 contract.
  for (const tag of tags.slice(1)) {
    assert.equal(tag.length, 4);
    assert.equal(tag[2], "");
  }
  assert.deepEqual(parseRosterEventMembers(makeRosterEvent(tags.slice(1))), [
    { pubkey: ALICE, role: "owner" },
    { pubkey: BOB, role: "collaborator" },
  ]);
});

test("buildRemoveRosterTags emits arity-2 p tags", () => {
  assert.deepEqual(buildRemoveRosterTags(ADDRESS, [ALICE.toUpperCase(), BOB]), [
    ["a", ADDRESS],
    ["p", ALICE],
    ["p", BOB],
  ]);
});

test("rosterWithOwner pins the creator first and drops stray creator rows", () => {
  const entries = rosterWithOwner({ owner: OWNER }, [
    { pubkey: ALICE, role: "viewer" },
    { pubkey: OWNER, role: "collaborator" },
  ]);
  assert.deepEqual(entries, [
    { pubkey: OWNER, role: "owner", isCreator: true },
    { pubkey: ALICE, role: "viewer", isCreator: false },
  ]);
});

test("rosterWithOwner omits the creator for the ownerless local General", () => {
  const entries = rosterWithOwner({ owner: "" }, [
    { pubkey: ALICE, role: "collaborator" },
  ]);
  assert.deepEqual(entries, [
    { pubkey: ALICE, role: "collaborator", isCreator: false },
  ]);
});

// The roster gates every project screen's write access. A WebSocket REQ for it
// queued behind the socket's read budget on a cold load (~5 s of "Checking
// whether you can edit…" in Planning), so it goes over the coalesced
// `POST /query` path and never touches the socket.
test("fetchProjectRoster reads the newest projection over POST /query, not a socket REQ", async () => {
  const filters = [];
  mock.method(relayClient, "fetchEventsCoalesced", (filter) => {
    filters.push(filter);
    return Promise.resolve([
      { ...makeRosterEvent([["p", ALICE, "", "viewer"]]), created_at: 100 },
      { ...makeRosterEvent([["p", BOB, "", "collaborator"]]), created_at: 200 },
    ]);
  });
  mock.method(relayClient, "fetchEvents", () => {
    throw new Error("the roster read must not use a WebSocket REQ");
  });
  try {
    const members = await fetchProjectRoster({
      owner: OWNER,
      address: ADDRESS,
      members: [],
    });
    assert.deepEqual(filters, [{ kinds: [39010], "#d": [ADDRESS], limit: 1 }]);
    assert.deepEqual(members, [{ pubkey: BOB, role: "collaborator" }]);
  } finally {
    mock.reset();
  }
});
