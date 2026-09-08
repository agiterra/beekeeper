import assert from "node:assert/strict";
import { test } from "node:test";

import {
  fetchProjectPulseDigest,
  projectPulseChannelSetUnresolved,
  pulseLeaseExpiryDelayMs,
} from "@/features/project-pulse/lib/pulseQueries";

const OWNER = "ab".repeat(32);
const PROJECT = `30621:${OWNER}:pulse-demo`;

const noEvents = async () => [];

/** A batch fake that records each bundle and answers per filter. */
function recordingBatch(batches, answer = async () => []) {
  return async (filters) => {
    batches.push(filters);
    const rows = [];
    for (const filter of filters) rows.push(...(await answer(filter)));
    return rows;
  };
}

/**
 * An unresolved channel set is a read that did not happen, not a project with
 * no channels. Without this the digest comes back `complete: true` with
 * `sessions: []` and the screen paints "the project is quiet" while a live
 * session runs in a channel the client never learned about. The Rust twin
 * (`scan_project_sessions`) pushes the same `{scope:"channels"}` row.
 */
test("an unresolved channel set makes the read partial, never quiet", async () => {
  const digest = await fetchProjectPulseDigest(PROJECT, [], {
    fetchEventsBatch: noEvents,
    channelsUnresolved: true,
  });
  assert.equal(digest.complete, false);
  assert.deepEqual(
    digest.errors.map((error) => error.scope),
    ["channels"],
  );
});

test("a stale persisted channel floor remains unresolved during revalidation", () => {
  assert.equal(
    projectPulseChannelSetUnresolved({
      isPending: false,
      isError: false,
      isFetching: true,
    }),
    true,
  );
  assert.equal(
    projectPulseChannelSetUnresolved({
      isPending: false,
      isError: false,
      isFetching: false,
    }),
    false,
  );
});

test("a resolved but empty channel set is a complete, confirmed-empty read", async () => {
  const digest = await fetchProjectPulseDigest(PROJECT, [], {
    fetchEventsBatch: noEvents,
    channelsUnresolved: false,
  });
  assert.equal(digest.complete, true);
  assert.deepEqual(digest.errors, []);
});

test("cold reads split durable history from the one-shot lease snapshot", async () => {
  const batches = [];
  await fetchProjectPulseDigest(PROJECT, ["channel-1"], {
    fetchEventsBatch: recordingBatch(batches),
  });
  // One bundled `POST /query` for the whole project: four filters, four
  // budgets, one call against the relay instead of four REQs.
  assert.equal(batches.length, 1);
  const filters = batches[0];
  assert.equal(filters.length, 4);
  assert.deepEqual(filters[0], {
    kinds: [44240],
    "#a": [PROJECT],
    limit: 500,
  });
  // Per-generation facts and per-turn receipts read separately, each with its
  // own budget — see the eviction test at the end of this file.
  assert.deepEqual(filters[1], {
    kinds: [44221, 44223, 44227, 44229, 44230],
    "#h": ["channel-1"],
    limit: 1000,
  });
  assert.deepEqual(filters[2], {
    kinds: [44224],
    "#h": ["channel-1"],
    limit: 1000,
  });
  assert.deepEqual(filters[3], {
    kinds: [24223],
    "#h": ["channel-1"],
    limit: 1000,
  });
});

test("cold reads sort, dedupe, and chunk explicit channels at the relay cap", async () => {
  const filters = [];
  const channels = [
    ...Array.from(
      { length: 129 },
      (_, index) => `channel-${String(index).padStart(3, "0")}`,
    ),
    "channel-000",
  ].reverse();
  const batches = [];
  await fetchProjectPulseDigest(PROJECT, channels, {
    fetchEventsBatch: recordingBatch(batches),
  });
  // One bundle per chunk: (generation facts, receipts, lease snapshot) each
  // as its own filter with its own budget; the entry read rides the first.
  assert.equal(batches.length, 2);
  assert.deepEqual(
    batches.map((bundle) => bundle.map((filter) => filter["#h"]?.length)),
    [
      [undefined, 128, 128, 128],
      [1, 1, 1],
    ],
  );
  assert.deepEqual(
    batches.map((bundle) => bundle.map((filter) => filter.kinds[0])),
    [
      [44240, 44221, 44224, 24223],
      [44221, 44224, 24223],
    ],
  );
  const channelFilters = batches.flat().filter((filter) => filter["#h"]);
  filters.push(...channelFilters);
  assert.ok(channelFilters.every((filter) => filter["#h"].length <= 128));
  assert.deepEqual(channelFilters[0]["#h"].slice(0, 2), [
    "channel-000",
    "channel-001",
  ]);
});

test("reachable lease schedules invalidation at its exact folded expiry", () => {
  const digest = {
    sessions: [
      {
        coordinationState: "provider_reachable",
        generations: [
          {
            current: true,
            reachability: "provider_reachable",
            leaseExpiresAt: 1_000,
          },
        ],
      },
    ],
  };
  assert.equal(pulseLeaseExpiryDelayMs(digest, 999_250), 750);
  assert.equal(pulseLeaseExpiryDelayMs(digest, 1_000_000), 0);
  assert.equal(pulseLeaseExpiryDelayMs({ sessions: [] }, 999_250), null);
});

test("a rejected bundle makes the digest partial for every read in it, never quiet", async () => {
  const digest = await fetchProjectPulseDigest(PROJECT, ["channel-1"], {
    fetchEventsBatch: async (filters) => {
      if (filters.some((filter) => filter.kinds.includes(24223)))
        throw new Error("lease snapshot timed out");
      return [];
    },
  });
  assert.equal(digest.complete, false);
  assert.deepEqual(digest.providerReachableSessions, []);
  // The bundle carried the entries, the session facts and the lease snapshot;
  // a failed bundle is every one of those reads not happening.
  assert.deepEqual(
    digest.errors
      .filter((error) => error.message === "lease snapshot timed out")
      .map((error) => error.scope)
      .sort(),
    ["entries", "leases", "sessions"],
  );
});

test("truncation is attributed to the read whose budget filled, not the bundle", async () => {
  const receipt = (index) => ({
    id: `receipt-${index}`,
    pubkey: OWNER,
    created_at: 2_000 + index,
    kind: 44224,
    tags: [],
    content: "{}",
    sig: "0".repeat(128),
  });
  const digest = await fetchProjectPulseDigest(PROJECT, ["channel-1"], {
    fetchEventsBatch: async () =>
      Array.from({ length: 1_000 }, (_, index) => receipt(index)),
  });
  assert.deepEqual(
    digest.errors.filter((error) => error.message.includes("truncated")),
    [{ scope: "sessions", message: "session read truncated at 1000 events" }],
  );
});

/**
 * An event dropped by client-side validation is an observation the surface
 * does not have. Dropping it without a trace turns one undecodable live
 * session into a completed negative verdict.
 */
test("events excluded by client validation are recorded, not silently dropped", async () => {
  const digest = await fetchProjectPulseDigest(PROJECT, ["channel-1"], {
    fetchEventsBatch: async () => [
      {
        id: "f".repeat(64),
        pubkey: OWNER,
        created_at: 1_785_512_437,
        kind: 44223,
        tags: [],
        content: "not json at all",
        sig: "0".repeat(128),
      },
    ],
  });
  assert.equal(digest.sessions.length, 0);
  assert.ok(
    digest.errors.some(
      (error) =>
        error.scope === "invalid-event" && error.message.includes("44223"),
    ),
    `expected an invalid-event row, got ${JSON.stringify(digest.errors)}`,
  );
});

/**
 * Kind 44224 stopped being one receipt per generation: a turn publishes at
 * least `turn_queued` and `turn_started`. A relay filter returns its newest
 * `limit` rows across *every* kind it names, so a receipt kind sharing the
 * session budget with the 44221 commands and 44223 metadata means a busy
 * project's proven generations fall off the end of the page and vanish from
 * the digest behind nothing but a generic truncation note.
 */
test("per-turn receipt volume cannot evict the facts a session is proven from", async () => {
  const create = { id: "create-1", kind: 44221, created_at: 1_000 };
  const metadata = { id: "metadata-1", kind: 44223, created_at: 1_001 };
  const pool = [
    create,
    metadata,
    ...Array.from({ length: 1_200 }, (_, index) => ({
      id: `receipt-${index}`,
      kind: 44224,
      created_at: 2_000 + index,
    })),
  ];
  const read = new Set();
  // A relay returns the newest `limit` rows matching the filter, exactly as
  // `ORDER BY created_at DESC ... LIMIT` does. Nothing is folded here: this
  // test is about what the read can still *reach*.
  const fetchEventsBatch = async (filters) => {
    for (const filter of filters) {
      const kinds = new Set(filter.kinds);
      for (const event of pool
        .filter((event) => kinds.has(event.kind))
        .sort((left, right) => right.created_at - left.created_at)
        .slice(0, filter.limit ?? 0)) {
        read.add(event.id);
      }
    }
    return [];
  };

  await fetchProjectPulseDigest(PROJECT, ["channel-1"], { fetchEventsBatch });
  assert.equal(
    read.has("create-1"),
    true,
    "the 44221 create must survive a channel full of turn receipts",
  );
  assert.equal(
    read.has("metadata-1"),
    true,
    "the 44223 metadata must survive a channel full of turn receipts",
  );
});

test("a mission read is keyed by coordinate and channel set, not by order", async () => {
  const { pulseMissionRowsQueryKey } = await import(
    "@/features/project-pulse/lib/pulseQueries"
  );
  assert.deepEqual(
    pulseMissionRowsQueryKey(PROJECT, ["b", "a"]),
    pulseMissionRowsQueryKey(PROJECT, ["a", "b"]),
  );
  assert.notDeepEqual(
    pulseMissionRowsQueryKey(PROJECT, ["a"]),
    pulseMissionRowsQueryKey(PROJECT, ["a", "b"]),
  );
});

test("a mission read that failed is unreadable, never an empty mission list", async () => {
  const { pulseMissionRowsState } = await import(
    "@/features/project-pulse/lib/pulseQueries"
  );
  const state = pulseMissionRowsState(
    {
      data: undefined,
      error: new Error("pulse mission rows: response is missing overlaps"),
      isPending: false,
      isFetching: false,
    },
    null,
  );
  assert.equal(state.kind, "unreadable");
  assert.equal(
    state.message,
    "pulse mission rows: response is missing overlaps",
  );
  assert.equal(state.rows, null);
});

test("a pending mission read paints its last complete answer, marked as such", async () => {
  const { pulseMissionRowsState } = await import(
    "@/features/project-pulse/lib/pulseQueries"
  );
  const cached = { missions: [], missionErrors: [] };
  const pending = pulseMissionRowsState(
    { data: undefined, error: null, isPending: true, isFetching: true },
    cached,
  );
  assert.equal(pending.kind, "ready");
  assert.equal(pending.refreshing, true);
  assert.equal(pending.rows, cached);

  const cold = pulseMissionRowsState(
    { data: undefined, error: null, isPending: true, isFetching: true },
    null,
  );
  assert.equal(cold.kind, "loading");
});

test("a settled mission read is ready and not marked stale", async () => {
  const { pulseMissionRowsState } = await import(
    "@/features/project-pulse/lib/pulseQueries"
  );
  const rows = { missions: [], missionErrors: [] };
  const state = pulseMissionRowsState(
    { data: rows, error: null, isPending: false, isFetching: false },
    null,
  );
  assert.equal(state.kind, "ready");
  assert.equal(state.refreshing, false);
  assert.equal(state.rows, rows);
});

// ── The mission read's own inputs ────────────────────────────────────────────
//
// REVIEW-L9 F1: the mission request used to be built from nothing — no
// sessions, and an `openSessionCount` no caller ever passed — so the native
// command answered with zero rows, zero errors, and one sentence promising
// "the newest 8 open sessions by observation time" over a read that opened no
// session at all. These pin the two halves of that: the count is the digest's
// real count, and every session that could not be read is disclosed by name.

const CHANNEL_ONE = "11111111-2222-3333-4444-555555555555";
const SESSION_ONE = "aaaaaaaa-1111-2222-3333-444444444444";
const FOUNDER = "cd".repeat(32);
const RELAY_SELF = "ef".repeat(32);

function missionFixture() {
  return {
    missionsSchema: "buzz-pulse-mission-rows/v1",
    missionScope:
      "project channels · the newest 8 open sessions by observation time",
    missions: [],
    missionErrors: [],
    openRulings: [],
    rulingsWaitingOnViewer: [],
    overlaps: [],
    viewerPubkey: null,
  };
}

function openSession(sessionKey, sessionRef, latestObservationAt) {
  return {
    sessionKey,
    sessionRef,
    name: null,
    goal: null,
    lifecycle: "open",
    coordinationState: "open_unverified",
    latestObservationAt,
    observedAgeSeconds: null,
    generations: [],
    sourceEventIds: [],
  };
}

function digestOf(sessions) {
  return {
    schema: "buzz-project-pulse-digest/v2",
    source: "client-composed",
    project: PROJECT,
    asOf: 1_756_800_000,
    complete: true,
    sessionsScope: "project channels",
    sessions,
    providerReachableSessions: [],
    openUnverifiedSessions: sessions.map((session) => session.sessionKey),
    closedSessions: [],
    entries: [],
    errors: [],
  };
}

function genesisEvent(channelRef, sessionRef, id) {
  return {
    id,
    pubkey: FOUNDER,
    created_at: 1_756_700_000,
    kind: 44226,
    tags: [
      ["h", channelRef],
      ["csg-v", "csg1-1"],
      ["csg-session", sessionRef],
    ],
    content: JSON.stringify({ sessionRef, v: 1 }),
    sig: "0".repeat(128),
  };
}

test("the mission read sends the number of open sessions the digest proved", async () => {
  const { fetchPulseMissionRows } = await import(
    "@/features/project-pulse/lib/pulseQueries"
  );
  let sent = null;
  await fetchPulseMissionRows(
    PROJECT,
    [],
    {
      digest: digestOf([
        openSession("s-1", SESSION_ONE, 1_756_800_000),
        openSession(
          "s-2",
          "bbbbbbbb-1111-2222-3333-444444444444",
          1_756_700_000,
        ),
      ]),
    },
    {
      invoke: async (_command, args) => {
        sent = args;
        return missionFixture();
      },
      fetchEvents: async () => [],
      relaySelf: async () => RELAY_SELF,
    },
  );
  assert.equal(sent.request.openSessionCount, 2);
});

test("nine open sessions send eight, and the count still says nine", async () => {
  const { fetchPulseMissionRows } = await import(
    "@/features/project-pulse/lib/pulseQueries"
  );
  const sessions = [];
  for (let index = 0; index < 9; index += 1) {
    const sessionRef = `${String(index).repeat(8)}-1111-2222-3333-444444444444`;
    sessions.push(openSession(`s-${index}`, sessionRef, 1_756_800_000 - index));
  }
  let sent = null;
  await fetchPulseMissionRows(
    PROJECT,
    [CHANNEL_ONE],
    { digest: digestOf(sessions) },
    {
      invoke: async (_command, args) => {
        sent = args;
        return missionFixture();
      },
      fetchEvents: async (filter) =>
        filter.kinds[0] === 44226
          ? sessions.map((session, index) =>
              genesisEvent(
                CHANNEL_ONE,
                session.sessionRef,
                `${String(index).repeat(2)}${"ab".repeat(31)}`,
              ),
            )
          : [],
      relaySelf: async () => RELAY_SELF,
    },
  );
  assert.equal(sent.request.openSessionCount, 9);
  assert.equal(sent.request.sessions.length, 8);
  // Newest observation first, so the eight that are read are the eight the
  // scope sentence promises.
  assert.deepEqual(
    sent.request.sessions.map((session) => session.sessionKey),
    ["s-0", "s-1", "s-2", "s-3", "s-4", "s-5", "s-6", "s-7"],
  );
});

test("a session whose team records could not be read is disclosed, not dropped", async () => {
  const { fetchPulseMissionRows } = await import(
    "@/features/project-pulse/lib/pulseQueries"
  );
  let sent = null;
  await fetchPulseMissionRows(
    PROJECT,
    [CHANNEL_ONE],
    { digest: digestOf([openSession("s-1", SESSION_ONE, 1_756_800_000)]) },
    {
      invoke: async (_command, args) => {
        sent = args;
        return missionFixture();
      },
      fetchEvents: async (filter) => {
        if (filter.kinds[0] === 44226) {
          return [genesisEvent(CHANNEL_ONE, SESSION_ONE, "ab".repeat(32))];
        }
        if (filter.kinds[0] === 44244) throw new Error("relay unreachable");
        return [];
      },
      relaySelf: async () => RELAY_SELF,
    },
  );
  const disclosed = sent.request.readErrors.find((error) =>
    error.message.includes("relay unreachable"),
  );
  assert.notEqual(disclosed, undefined);
  assert.equal(disclosed.scope, `missions:${CHANNEL_ONE}`);
  // The count still says one session is in scope, so the native command's own
  // unread disclosure fires beside this one.
  assert.equal(sent.request.openSessionCount, 1);
  assert.deepEqual(sent.request.sessions, []);
});
