/**
 * The provenance reads: four budgets, chunked channels, and a sentence for
 * every event and every page this client could not use.
 *
 * The fixtures are really signed, because the admission gate under test is a
 * signature check — an object that merely looks like an event would pass a
 * hand-rolled stub and prove nothing about the boundary.
 */
import assert from "node:assert/strict";
import test from "node:test";

import {
  finalizeEvent,
  generateSecretKey,
  getPublicKey,
} from "nostr-tools/pure";

import {
  KIND_CODING_SESSION_GENESIS,
  KIND_CODING_SESSION_LIFECYCLE_COMMAND,
  KIND_CODING_SESSION_LIFECYCLE_RECEIPT,
  KIND_CODING_SESSION_METADATA,
} from "@/shared/constants/kinds.ts";

import {
  fetchRolePackProvenanceEvents,
  rolePackProvenanceQueryKey,
  ROLE_PACK_PROVENANCE_CHANNELS_PER_QUERY,
  ROLE_PACK_PROVENANCE_QUERY_LIMIT,
} from "./rolePackProvenanceQuery.ts";

const CHANNEL = "channel-provenance-1";
const SECRET = generateSecretKey();
const PUBKEY = getPublicKey(SECRET);

function signedNote(kind, createdAt = 1_800_000_000) {
  return finalizeEvent(
    {
      kind,
      created_at: createdAt,
      tags: [["h", CHANNEL]],
      content: JSON.stringify({ note: kind }),
    },
    SECRET,
  );
}

test("each proof kind gets its own read, its own budget and the h scope", async () => {
  const filters = [];
  await fetchRolePackProvenanceEvents([CHANNEL], {
    fetchEvents: async (filter) => {
      filters.push(filter);
      return [];
    },
  });

  assert.deepEqual(
    filters.map((filter) => filter.kinds),
    [
      [KIND_CODING_SESSION_LIFECYCLE_COMMAND],
      [KIND_CODING_SESSION_LIFECYCLE_RECEIPT],
      [KIND_CODING_SESSION_METADATA],
      [KIND_CODING_SESSION_GENESIS],
    ],
  );
  for (const filter of filters) {
    assert.deepEqual(filter["#h"], [CHANNEL]);
    assert.equal(filter.limit, ROLE_PACK_PROVENANCE_QUERY_LIMIT);
  }
});

test("channels are chunked at the relay's cap on explicit h values", async () => {
  const channelIds = Array.from(
    { length: ROLE_PACK_PROVENANCE_CHANNELS_PER_QUERY + 1 },
    (_unused, index) => `channel-${String(index).padStart(4, "0")}`,
  );
  const filters = [];
  await fetchRolePackProvenanceEvents(channelIds, {
    fetchEvents: async (filter) => {
      filters.push(filter);
      return [];
    },
  });

  assert.equal(filters.length, 8);
  assert.equal(
    filters[0]["#h"].length,
    ROLE_PACK_PROVENANCE_CHANNELS_PER_QUERY,
  );
  assert.equal(filters[4]["#h"].length, 1);
});

test("a full page is disclosed as truncated proof, not as the whole story", async () => {
  const page = signedNote(KIND_CODING_SESSION_LIFECYCLE_COMMAND);
  const { events, sourceErrors } = await fetchRolePackProvenanceEvents(
    [CHANNEL],
    {
      fetchEvents: async (filter) =>
        filter.kinds[0] === KIND_CODING_SESSION_LIFECYCLE_COMMAND
          ? Array.from({ length: ROLE_PACK_PROVENANCE_QUERY_LIMIT }, () => page)
          : [],
    },
  );

  assert.deepEqual(sourceErrors, [
    {
      scope: "lifecycle-commands",
      message:
        "Lifecycle commands were truncated at 1000 events; older proof may be missing.",
      channelIds: [CHANNEL],
    },
  ]);
  assert.equal(events.length, 1, "the repeated page is deduped by event id");
});

test("a failed read names what could not be read and why", async () => {
  const { sourceErrors } = await fetchRolePackProvenanceEvents([CHANNEL], {
    fetchEvents: async (filter) => {
      if (filter.kinds[0] === KIND_CODING_SESSION_LIFECYCLE_RECEIPT) {
        throw new Error("relay request timed out.");
      }
      return [];
    },
  });

  assert.deepEqual(sourceErrors, [
    {
      scope: "lifecycle-receipts",
      message: "Lifecycle receipts could not be read: relay request timed out.",
      channelIds: [CHANNEL],
    },
  ]);
});

test("an event whose signature does not verify is excluded with a readable note", async () => {
  const genuine = signedNote(KIND_CODING_SESSION_GENESIS);
  const forged = {
    ...genuine,
    id: `${genuine.id[0] === "f" ? "0" : "f"}${genuine.id.slice(1)}`,
  };
  const { events, sourceErrors } = await fetchRolePackProvenanceEvents(
    [CHANNEL],
    {
      fetchEvents: async (filter) =>
        filter.kinds[0] === KIND_CODING_SESSION_GENESIS
          ? [genuine, forged]
          : [],
    },
  );

  assert.deepEqual(
    events.map((event) => event.id),
    [genuine.id],
  );
  assert.deepEqual(sourceErrors, [
    {
      scope: "invalid-event",
      message: `A lifecycle event (${forged.id.slice(0, 8)}…, kind ${KIND_CODING_SESSION_GENESIS}) failed signature validation and was not used as proof.`,
    },
  ]);
});

test("admitted events keep their signature for the classifiers to re-check", async () => {
  const genuine = signedNote(KIND_CODING_SESSION_LIFECYCLE_COMMAND);
  const { events } = await fetchRolePackProvenanceEvents([CHANNEL], {
    fetchEvents: async (filter) =>
      filter.kinds[0] === KIND_CODING_SESSION_LIFECYCLE_COMMAND
        ? [genuine]
        : [],
  });

  assert.equal(events[0].sig, genuine.sig);
  assert.equal(events[0].pubkey, PUBKEY);
});

test("a failed read names only the channels its own chunk covered", async () => {
  const channelIds = Array.from(
    { length: ROLE_PACK_PROVENANCE_CHANNELS_PER_QUERY + 1 },
    (_unused, index) => `channel-${String(index).padStart(4, "0")}`,
  );
  const { sourceErrors } = await fetchRolePackProvenanceEvents(channelIds, {
    fetchEvents: async (filter) => {
      if (
        filter.kinds[0] === KIND_CODING_SESSION_GENESIS &&
        filter["#h"].length === 1
      ) {
        throw new Error("relay request timed out");
      }
      return [];
    },
  });

  assert.equal(sourceErrors.length, 1);
  assert.deepEqual(sourceErrors[0].channelIds, ["channel-0128"]);
});

test("the query key leads with the relay a proof was read from", () => {
  const before = rolePackProvenanceQueryKey(
    "wss://a",
    "project",
    [CHANNEL],
    ["aa"],
  );
  const after = rolePackProvenanceQueryKey(
    "wss://b",
    "project",
    [CHANNEL],
    ["aa"],
  );
  assert.notDeepEqual(
    before,
    after,
    "the same project on two relays is two answers",
  );
  assert.equal(before[0], "role-pack-provenance");
  assert.equal(before[1], "wss://a");
});

test("the query key changes when a new report arrives", () => {
  const before = rolePackProvenanceQueryKey(
    "wss://a",
    "project",
    [CHANNEL],
    ["aa"],
  );
  const after = rolePackProvenanceQueryKey(
    "wss://a",
    "project",
    [CHANNEL],
    ["aa", "bb"],
  );
  assert.notDeepEqual(before, after);
  assert.deepEqual(
    rolePackProvenanceQueryKey("wss://a", "project", ["b", "a"], ["z", "y"]),
    rolePackProvenanceQueryKey("wss://a", "project", ["a", "b"], ["y", "z"]),
    "the key is order-insensitive, so a re-ordered channel list is one entry",
  );
});
