import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import test from "node:test";

import {
  buildCodingSessionObservationFilters,
  buildCodingSessionObservationLiveFilter,
  CODING_SESSION_OBSERVATION_HISTORY_LIMIT,
  CODING_SESSION_OBSERVATION_LIVE_DEBOUNCE_MS,
  createTrailingDebounce,
  openCodingSessionObservationLiveSubscription,
  readCodingSessionObservations,
  watchCodingSessionObservationConnection,
} from "./useCodingSessionObservations.ts";
import { codingSessionObservationNotLive } from "../lib/codingSessionObservationLiveness.ts";
import {
  hasLiveGateStart,
  nextGateStartStaleFlipDelayMs,
} from "./useCodingSessionGateStartClock.ts";

const FIXTURE = JSON.parse(
  readFileSync(
    fileURLToPath(
      new URL(
        "../lib/codingSessionObservationFoldAdapterResponse.fixture.json",
        import.meta.url,
      ),
    ),
    "utf8",
  ),
);

const SCOPE = {
  channelRef: "d3e440ea-89f8-4aee-8a02-17edc3e7272e",
  sessionRef: FIXTURE.sessionRef,
  genesisRef: FIXTURE.genesisRef,
};

test("the filter names its kinds, and scopes by channel, umbrella and genesis", () => {
  const filters = buildCodingSessionObservationFilters(
    SCOPE,
    CODING_SESSION_OBSERVATION_HISTORY_LIMIT,
  );
  assert.equal(filters.length, 1);
  // Omitting `kinds` trips the relay's p-gate (403), so it is never omitted.
  assert.deepEqual(filters[0].kinds, [44246]);
  assert.deepEqual(filters[0]["#h"], [SCOPE.channelRef]);
  assert.deepEqual(filters[0]["#d"], [SCOPE.sessionRef]);
  assert.deepEqual(filters[0]["#csob-genesis"], [SCOPE.genesisRef]);
  assert.equal(filters[0].limit, CODING_SESSION_OBSERVATION_HISTORY_LIMIT);
});

test("the read hands the relay's events to the native fold, unfiltered", async () => {
  const fetched = FIXTURE.inputEventIds.map((id, index) => ({
    id,
    pubkey: "11".repeat(32),
    created_at: 1_756_800_000 + index,
    kind: 44246,
    tags: [],
    content: "{}",
    sig: "22".repeat(64),
  }));
  const calls = [];
  const result = await readCodingSessionObservations({
    scope: SCOPE,
    knownAssignmentRefs: ["cd".repeat(32)],
    providerPubkeys: ["66".repeat(32)],
    client: { fetchEventsCoalesced: async () => fetched },
    invoke: async (command, args) => {
      calls.push({ command, args });
      return structuredClone(FIXTURE);
    },
  });

  assert.equal(calls.length, 1);
  // Nothing is pre-filtered: the fold verifies each signature itself and lists
  // what it cannot read, and an event dropped here would vanish instead of
  // being disclosed.
  assert.equal(calls[0].args.request.events.length, fetched.length);
  // REVIEW-L5 F2: the provider set rides with the request, so Rust decides
  // whether an `observed` claim was earned.
  assert.deepEqual(calls[0].args.request.providerPubkeys, ["66".repeat(32)]);
  assert.equal(result.fold.gates.length, 2);
  assert.equal(result.signedAt.get(fetched[0].id), 1_756_800_000);
});

test("a relay that returns nothing folds to an empty answer, not to an error", async () => {
  const empty = {
    ...structuredClone(FIXTURE),
    inputEventIds: [],
    checkpoints: [],
    gates: [],
    findings: [],
    phases: [],
    unresolved: [],
    ignored: [],
  };
  const result = await readCodingSessionObservations({
    scope: SCOPE,
    knownAssignmentRefs: [],
    providerPubkeys: null,
    client: { fetchEventsCoalesced: async () => [] },
    invoke: async () => structuredClone(empty),
  });
  assert.deepEqual(result.fold.gates, []);
  assert.equal(result.signedAt.size, 0);
});

test("the live filter wakes on new 44246 in this channel and umbrella only", () => {
  const filter = buildCodingSessionObservationLiveFilter(SCOPE, 1_800_000_000);
  assert.deepEqual(filter.kinds, [44246]);
  assert.deepEqual(filter["#h"], [SCOPE.channelRef]);
  assert.deepEqual(filter["#d"], [SCOPE.sessionRef]);
  // `since`, so an observation signed while the subscription was opening is
  // replayed and still wakes a re-read.
  assert.equal(filter.since, 1_800_000_000);
});

test("a burst of live observations triggers one re-read, 750 ms after the last", () => {
  assert.equal(CODING_SESSION_OBSERVATION_LIVE_DEBOUNCE_MS, 750);
  const pending = new Map();
  let next = 1;
  const timers = {
    set: (callback, ms) => {
      const id = next++;
      pending.set(id, { callback, ms });
      return id;
    },
    clear: (id) => pending.delete(id),
  };
  let reads = 0;
  const debounce = createTrailingDebounce(() => reads++, 750, timers);
  debounce.wake();
  debounce.wake();
  debounce.wake();
  assert.equal(pending.size, 1, "only the last wake is still pending");
  const [[id, timer]] = [...pending];
  assert.equal(timer.ms, 750);
  pending.delete(id);
  timer.callback();
  assert.equal(reads, 1);
  debounce.wake();
  debounce.cancel();
  assert.equal(pending.size, 0, "cancel drops a pending re-read");
  assert.equal(reads, 1);
});

test("the gate-start clock ticks only while an open start is not yet stale", () => {
  const start = {
    eventId: "aa".repeat(32),
    authorPubkey: "bb".repeat(32),
    gate: "cargo test",
    startedAtMs: 1_000,
    assignmentRef: null,
    closeEventId: null,
    endedAtMs: null,
    durationMs: null,
  };
  assert.equal(hasLiveGateStart([start], 1_800_000, 1_000), true);
  assert.equal(hasLiveGateStart([start], 1_800_000, 1_801_000), false);
  assert.equal(
    hasLiveGateStart(
      [{ ...start, closeEventId: "cc".repeat(32) }],
      1_800_000,
      1_000,
    ),
    false,
  );
  assert.equal(hasLiveGateStart([start], null, 1_000), false);
  assert.equal(hasLiveGateStart([], 1_800_000, 1_000), false);
});

async function liveStates(client) {
  const states = [];
  const dispose = openCodingSessionObservationLiveSubscription({
    client,
    scope: SCOPE,
    sinceSeconds: 1,
    onWake: () => {},
    onLive: (live) => states.push(live),
  });
  await new Promise((resolve) => setImmediate(resolve));
  return { states, dispose };
}

test("a rejected live subscription reads as unavailable, never as live", async () => {
  const { states } = await liveStates({
    fetchEventsCoalesced: async () => [],
    subscribeLive: () => Promise.reject(new Error("relay closed")),
  });
  assert.deepEqual(states, ["connecting", "unavailable"]);
});

test("a client with no live subscription, or one that hands back none, is unavailable", async () => {
  const absent = await liveStates({ fetchEventsCoalesced: async () => [] });
  assert.deepEqual(absent.states, ["unavailable"]);
  const empty = await liveStates({
    fetchEventsCoalesced: async () => [],
    subscribeLive: async () => null,
  });
  assert.deepEqual(empty.states, ["connecting", "unavailable"]);
});

test("a subscription the relay answers with EOSE is subscribed, and disposing closes it", async () => {
  let closed = 0;
  const { states, dispose } = await liveStates({
    fetchEventsCoalesced: async () => [],
    subscribeLive: async (_filter, _onEvent, onReady) => {
      onReady?.("eose");
      return async () => {
        closed += 1;
      };
    },
  });
  assert.deepEqual(states, ["connecting", "subscribed"]);
  dispose();
  assert.equal(closed, 1);
});

test("a handle after a CLOSED refusal is unavailable, never live", async () => {
  const { states } = await liveStates({
    fetchEventsCoalesced: async () => [],
    subscribeLive: async (_filter, _onEvent, onReady) => {
      onReady?.("closed");
      return async () => {};
    },
  });
  assert.deepEqual(states, ["connecting", "unavailable"]);
});

test("a handle after a readiness timeout, or with none reported, stays connecting", async () => {
  const timedOut = await liveStates({
    fetchEventsCoalesced: async () => [],
    subscribeLive: async (_filter, _onEvent, onReady) => {
      onReady?.("timeout");
      return async () => {};
    },
  });
  assert.deepEqual(timedOut.states, ["connecting"]);
  const silent = await liveStates({
    fetchEventsCoalesced: async () => [],
    subscribeLive: async () => async () => {},
  });
  assert.deepEqual(silent.states, ["connecting"]);
});

test("the not-live note names the read time, and is absent while subscribed", () => {
  const format = () => "14:02";
  assert.equal(codingSessionObservationNotLive("subscribed", 1, format), null);
  const note = codingSessionObservationNotLive("unavailable", 1, format);
  assert.equal(note?.short, "not live — read at 14:02");
  assert.match(note?.sentence ?? "", /not being kept current/);
  assert.equal(
    codingSessionObservationNotLive("connecting", null, format)?.short,
    "not live",
  );
});

test("the stale flip is scheduled for the earliest open start, not the next minute tick", () => {
  const open = (eventId, startedAtMs) => ({
    eventId,
    authorPubkey: "bb".repeat(32),
    gate: "cargo test",
    startedAtMs,
    assignmentRef: null,
    closeEventId: null,
    endedAtMs: null,
    durationMs: null,
  });
  const stale = 1_800_000;
  // A read landing 29m30s after the start flips 30s later, not at 30m30s.
  assert.equal(
    nextGateStartStaleFlipDelayMs([open("a", 0)], stale, 1_770_000),
    30_000,
  );
  // The earliest of several open starts wins; closed and stale ones do not.
  assert.equal(
    nextGateStartStaleFlipDelayMs(
      [
        open("a", 600_000),
        open("b", 300_000),
        { ...open("c", 100_000), closeEventId: "cc".repeat(32) },
        open("d", -2_000_000),
      ],
      stale,
      1_000_000,
    ),
    1_100_000,
  );
  assert.equal(
    nextGateStartStaleFlipDelayMs([open("a", 0)], stale, stale),
    null,
    "already stale: nothing left to flip",
  );
  assert.equal(nextGateStartStaleFlipDelayMs([open("a", 0)], null, 0), null);
  assert.equal(nextGateStartStaleFlipDelayMs([], stale, 0), null);
});

function connectionClient({ reconnects = true } = {}) {
  const stateListeners = new Set();
  const reconnectListeners = new Set();
  let state = "connected";
  const client = {
    fetchEventsCoalesced: async () => [],
    subscribeToConnectionState(listener) {
      stateListeners.add(listener);
      listener(state);
      return () => stateListeners.delete(listener);
    },
  };
  if (reconnects) {
    client.subscribeToReconnects = (listener) => {
      reconnectListeners.add(listener);
      return () => reconnectListeners.delete(listener);
    };
  }
  return {
    client,
    setState(next) {
      state = next;
      for (const listener of stateListeners) listener(next);
    },
    reconnect() {
      for (const listener of reconnectListeners) listener();
    },
    listenerCount: () => stateListeners.size + reconnectListeners.size,
  };
}

test("a dropped socket stops the read claiming live until the replayed subscription is back", () => {
  const relay = connectionClient();
  const downs = [];
  let restored = 0;
  const dispose = watchCodingSessionObservationConnection({
    client: relay.client,
    onDown: (live) => downs.push(live),
    onRestored: () => {
      restored += 1;
    },
  });
  assert.deepEqual(downs, [], "connected at subscribe time is not a drop");
  relay.setState("reconnecting");
  relay.setState("stalled");
  assert.deepEqual(downs, ["connecting", "connecting"]);
  // `connected` alone is not restored: the replay has not finished yet.
  relay.setState("connected");
  assert.equal(restored, 0);
  relay.reconnect();
  assert.equal(restored, 1, "re-open and re-read once, on the reconnect");
  relay.setState("disconnected");
  assert.deepEqual(downs.at(-1), "unavailable");
  dispose();
  assert.equal(relay.listenerCount(), 0, "disposing removes both listeners");
});

test("a client with no reconnect signal is restored on connected after a drop", () => {
  const relay = connectionClient({ reconnects: false });
  const downs = [];
  let restored = 0;
  const dispose = watchCodingSessionObservationConnection({
    client: relay.client,
    onDown: (live) => downs.push(live),
    onRestored: () => {
      restored += 1;
    },
  });
  relay.setState("connected");
  assert.equal(restored, 0, "no drop, nothing to restore");
  relay.setState("reconnecting");
  relay.setState("connected");
  assert.deepEqual(downs, ["connecting"]);
  assert.equal(restored, 1);
  dispose();
});

test("a client that reports no connection state is left alone", () => {
  let calls = 0;
  const dispose = watchCodingSessionObservationConnection({
    client: { fetchEventsCoalesced: async () => [] },
    onDown: () => {
      calls += 1;
    },
    onRestored: () => {
      calls += 1;
    },
  });
  dispose();
  assert.equal(calls, 0);
});
