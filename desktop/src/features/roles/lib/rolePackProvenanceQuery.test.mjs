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
  ROLE_PACK_PROVENANCE_MAX_REQUESTS,
  ROLE_PACK_PROVENANCE_QUERY_LIMIT,
} from "./rolePackProvenanceQuery.ts";

const CHANNEL = "channel-provenance-1";
const SECRET = generateSecretKey();
const PUBKEY = getPublicKey(SECRET);

function signedNote(
  kind,
  createdAt = 1_800_000_000,
  nonce = "",
  channel = CHANNEL,
) {
  return finalizeEvent(
    {
      kind,
      created_at: createdAt,
      tags: [["h", channel]],
      content: JSON.stringify({ note: kind, nonce }),
    },
    SECRET,
  );
}

function makeRelayModel(events) {
  const calls = [];
  const fetchEvents = async (filter) => {
    calls.push(filter);
    return events
      .filter((event) => {
        if (!filter.kinds.includes(event.kind)) return false;
        if (
          !event.tags.some(
            (tag) => tag[0] === "h" && filter["#h"].includes(tag[1]),
          )
        ) {
          return false;
        }
        return filter.until === undefined || event.created_at <= filter.until;
      })
      .sort((left, right) =>
        right.created_at !== left.created_at
          ? right.created_at - left.created_at
          : left.id < right.id
            ? -1
            : 1,
      )
      .slice(0, filter.limit);
  };
  return { calls, fetchEvents };
}

const LONG_COMMAND_HISTORY = Array.from(
  { length: ROLE_PACK_PROVENANCE_QUERY_LIMIT + 2 },
  (_unused, index) =>
    signedNote(
      KIND_CODING_SESSION_LIFECYCLE_COMMAND,
      10_000 + index,
      `command-${index}`,
    ),
);

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
      [44228],
      [40099],
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

  assert.equal(filters.length, 12);
  assert.equal(
    filters[0]["#h"].length,
    ROLE_PACK_PROVENANCE_CHANNELS_PER_QUERY,
  );
  assert.equal(filters[6]["#h"].length, 1);
});

test("recovers more than 1000 events with inclusive overlap deduped", async () => {
  const { calls, fetchEvents } = makeRelayModel(LONG_COMMAND_HISTORY);
  const { events, sourceErrors } = await fetchRolePackProvenanceEvents(
    [CHANNEL],
    { fetchEvents },
  );

  assert.deepEqual(sourceErrors, []);
  assert.equal(events.length, LONG_COMMAND_HISTORY.length);
  const commandCalls = calls.filter(
    (filter) => filter.kinds[0] === KIND_CODING_SESSION_LIFECYCLE_COMMAND,
  );
  assert.equal(commandCalls.length, 2);
  assert.equal(
    commandCalls[1].until,
    10_002,
    "the second request reuses the oldest timestamp inclusively",
  );
  assert.equal(
    new Set(events.map((event) => event.id)).size,
    events.length,
    "the inclusive boundary event is returned once",
  );
});

test("an older competing lifecycle command remains in the recovered history", async () => {
  const olderConflict = LONG_COMMAND_HISTORY[0];
  const { fetchEvents } = makeRelayModel(LONG_COMMAND_HISTORY);
  const { events, sourceErrors } = await fetchRolePackProvenanceEvents(
    [CHANNEL],
    { fetchEvents },
  );

  assert.deepEqual(sourceErrors, []);
  assert.ok(
    events.some((event) => event.id === olderConflict.id),
    "the command beyond the first 1,000 rows must remain available to dispute newer proof",
  );
});

test("a saturated same-timestamp boundary is incomplete and never skips back one second", async () => {
  const timestamp = 50_000;
  const boundary = Array.from(
    { length: ROLE_PACK_PROVENANCE_QUERY_LIMIT },
    (_unused, index) =>
      signedNote(
        KIND_CODING_SESSION_LIFECYCLE_COMMAND,
        timestamp,
        `boundary-${index}`,
      ),
  );
  const { calls, fetchEvents } = makeRelayModel(boundary);
  const { events, sourceErrors } = await fetchRolePackProvenanceEvents(
    [CHANNEL],
    { fetchEvents },
  );

  assert.equal(events.length, ROLE_PACK_PROVENANCE_QUERY_LIMIT);
  const commandCalls = calls.filter(
    (filter) => filter.kinds[0] === KIND_CODING_SESSION_LIFECYCLE_COMMAND,
  );
  assert.equal(commandCalls.length, 2);
  assert.equal(commandCalls[1].until, timestamp);
  assert.ok(
    commandCalls.every((filter) => filter.until !== timestamp - 1),
    "the reader must not skip unresolved events at the boundary second",
  );
  assert.equal(sourceErrors[0].scope, "lifecycle-commands");
  assert.deepEqual(sourceErrors[0].channelIds, [CHANNEL]);
  assert.match(sourceErrors[0].message, /full relay page at timestamp 50000/);
});

test("a later-page failure preserves the already recovered page", async () => {
  const { fetchEvents: relayFetch } = makeRelayModel(LONG_COMMAND_HISTORY);
  let commandRequests = 0;
  const { events, sourceErrors } = await fetchRolePackProvenanceEvents(
    [CHANNEL],
    {
      fetchEvents: async (filter) => {
        if (
          filter.kinds[0] === KIND_CODING_SESSION_LIFECYCLE_COMMAND &&
          ++commandRequests === 2
        ) {
          throw new Error("page two timed out");
        }
        return relayFetch(filter);
      },
    },
  );

  assert.equal(events.length, ROLE_PACK_PROVENANCE_QUERY_LIMIT);
  assert.equal(sourceErrors.length, 1);
  assert.deepEqual(sourceErrors[0].channelIds, [CHANNEL]);
  assert.match(sourceErrors[0].message, /page two timed out/);
});

test("the per-kind request budget returns recovered events as incomplete", async () => {
  const steps = Array.from(
    { length: ROLE_PACK_PROVENANCE_MAX_REQUESTS },
    (_unused, index) =>
      signedNote(
        KIND_CODING_SESSION_LIFECYCLE_COMMAND,
        70_000 - index,
        `budget-${index}`,
      ),
  );
  let commandRequests = 0;
  const { events, sourceErrors } = await fetchRolePackProvenanceEvents(
    [CHANNEL],
    {
      fetchEvents: async (filter) => {
        if (filter.kinds[0] !== KIND_CODING_SESSION_LIFECYCLE_COMMAND) {
          return [];
        }
        const step = steps[commandRequests++];
        return Array.from(
          { length: ROLE_PACK_PROVENANCE_QUERY_LIMIT },
          () => step,
        );
      },
    },
  );

  assert.equal(commandRequests, ROLE_PACK_PROVENANCE_MAX_REQUESTS);
  assert.equal(events.length, ROLE_PACK_PROVENANCE_MAX_REQUESTS);
  assert.equal(sourceErrors.length, 1);
  assert.equal(sourceErrors[0].scope, "lifecycle-commands");
  assert.match(
    sourceErrors[0].message,
    /bounded recovery budget of 8 requests/,
  );
});

test("malformed and out-of-scope pagination responses are rejected per read", async (t) => {
  const cases = [
    {
      name: "malformed event",
      event: { id: "incomplete" },
      message: /malformed event/,
    },
    {
      name: "wrong kind",
      event: signedNote(KIND_CODING_SESSION_GENESIS),
      message: /outside the requested kind/,
    },
    {
      name: "wrong channel",
      event: signedNote(
        KIND_CODING_SESSION_LIFECYCLE_COMMAND,
        10,
        "other-channel",
        "other-channel",
      ),
      message: /outside the requested channel chunk/,
    },
  ];

  for (const scenario of cases) {
    await t.test(scenario.name, async () => {
      const { events, sourceErrors } = await fetchRolePackProvenanceEvents(
        [CHANNEL],
        {
          fetchEvents: async (filter) =>
            filter.kinds[0] === KIND_CODING_SESSION_LIFECYCLE_COMMAND
              ? [scenario.event]
              : [],
        },
      );
      assert.deepEqual(events, []);
      assert.equal(sourceErrors[0].scope, "lifecycle-commands");
      assert.deepEqual(sourceErrors[0].channelIds, [CHANNEL]);
      assert.match(sourceErrors[0].message, scenario.message);
    });
  }
});

test("an event newer than an inclusive cursor is rejected without losing page one", async () => {
  const first = signedNote(KIND_CODING_SESSION_LIFECYCLE_COMMAND, 100, "first");
  const outOfRange = signedNote(
    KIND_CODING_SESSION_LIFECYCLE_COMMAND,
    101,
    "out-of-range",
  );
  let commandRequests = 0;
  const { events, sourceErrors } = await fetchRolePackProvenanceEvents(
    [CHANNEL],
    {
      fetchEvents: async (filter) => {
        if (filter.kinds[0] !== KIND_CODING_SESSION_LIFECYCLE_COMMAND) {
          return [];
        }
        commandRequests += 1;
        return commandRequests === 1
          ? Array.from(
              { length: ROLE_PACK_PROVENANCE_QUERY_LIMIT },
              () => first,
            )
          : [outOfRange];
      },
    },
  );

  assert.deepEqual(
    events.map((event) => event.id),
    [first.id],
  );
  assert.match(
    sourceErrors[0].message,
    /newer than the inclusive until cursor/,
  );
});

test("a non-progressing full response stops inside the request bound", async () => {
  const first = signedNote(
    KIND_CODING_SESSION_LIFECYCLE_COMMAND,
    100,
    "stalled",
  );
  let commandRequests = 0;
  const { events, sourceErrors } = await fetchRolePackProvenanceEvents(
    [CHANNEL],
    {
      fetchEvents: async (filter) => {
        if (filter.kinds[0] !== KIND_CODING_SESSION_LIFECYCLE_COMMAND) {
          return [];
        }
        commandRequests += 1;
        if (commandRequests === 1) {
          return Array.from(
            { length: ROLE_PACK_PROVENANCE_QUERY_LIMIT },
            () => first,
          );
        }
        return [
          ...Array.from(
            { length: ROLE_PACK_PROVENANCE_QUERY_LIMIT - 1 },
            () => first,
          ),
          { ...first, created_at: 99 },
        ];
      },
    },
  );

  assert.equal(commandRequests, 2);
  assert.deepEqual(
    events.map((event) => event.id),
    [first.id],
  );
  assert.match(sourceErrors[0].message, /made no unique-event progress/);
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
  const unaffected = signedNote(
    KIND_CODING_SESSION_GENESIS,
    123,
    "unaffected-chunk",
    "channel-0000",
  );
  const { events, sourceErrors } = await fetchRolePackProvenanceEvents(
    channelIds,
    {
      fetchEvents: async (filter) => {
        if (filter.kinds[0] !== KIND_CODING_SESSION_GENESIS) return [];
        if (filter["#h"].length === 1) {
          throw new Error("relay request timed out");
        }
        return [unaffected];
      },
    },
  );

  assert.equal(sourceErrors.length, 1);
  assert.deepEqual(sourceErrors[0].channelIds, ["channel-0128"]);
  assert.deepEqual(
    events.map((event) => event.id),
    [unaffected.id],
    "a failed chunk does not discard evidence recovered from another chunk",
  );
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

test("cold history verification yields to queued UI work before completing", async () => {
  const events = Array.from({ length: 32 }, (_, i) =>
    signedNote(KIND_CODING_SESSION_METADATA, 1800000000 + i),
  );
  let inputHandled = false;
  const input = setTimeout(() => {
    inputHandled = true;
  }, 0);
  const result = await fetchRolePackProvenanceEvents([CHANNEL], {
    fetchEvents: async (filter) =>
      filter.kinds[0] === KIND_CODING_SESSION_METADATA ? events : [],
  });
  clearTimeout(input);
  assert.equal(inputHandled, true);
  assert.equal(result.events.length, 32);
});
