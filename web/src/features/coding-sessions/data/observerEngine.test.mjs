/**
 * The observer engine, against a fake transport.
 *
 * The transport is injected whole, so these tests are about what the engine
 * asks the relay for, what it does with what comes back, and what it refuses
 * to conclude — never about sockets.
 */
import assert from "node:assert/strict";
import test from "node:test";
import {
  CHANNEL_ID,
  SESSION_REF,
  createEvent,
  leaseEvent,
  metadataEvent,
  newSigner,
  receiptEvent,
  target,
  transcriptEvent,
} from "../domain/testFixtures.mjs";
import {
  codingSessionHistoryFilters,
  codingSessionLeasesFilter,
} from "../domain/filters.ts";
import { buildCodingSessionTargetKey } from "../domain/keys.ts";
import {
  CodingSessionObserverEngine,
  codingSessionLiveFilters,
} from "./observerEngine.ts";

const RELAY_URL = "wss://relay.example/";
const NOW_MS = 1_700_000_010_000;

/** A transport that records every subscription instead of opening one. */
function fakeTransport() {
  const subscriptions = [];
  const subscribe = (wsUrl, filters, onEvent, options = {}) => {
    const subscription = {
      wsUrl,
      filters,
      onEvent,
      options,
      stopped: false,
      /** Route an event to the first filter that would have matched it. */
      emit(event) {
        const index = filters.findIndex((filter) =>
          filter.kinds.includes(event.kind),
        );
        assert.notEqual(index, -1, `no filter asked for kind ${event.kind}`);
        onEvent(event, index);
      },
      eoseAll() {
        filters.forEach((_filter, index) => {
          options.onEose?.(index);
        });
      },
    };
    subscription.stop = () => {
      subscription.stopped = true;
    };
    subscriptions.push(subscription);
    return subscription.stop;
  };
  return {
    subscribe,
    subscriptions,
    live: () => subscriptions[0],
    history: () => subscriptions[1],
    last: () => subscriptions[subscriptions.length - 1],
  };
}

function startEngine(options = {}) {
  const transport = fakeTransport();
  const leaseSchedule = {};
  const engine = new CodingSessionObserverEngine({
    wsUrl: RELAY_URL,
    channelId: CHANNEL_ID,
    subscribe: transport.subscribe,
    now: () => options.nowMs ?? NOW_MS,
    scheduleRepeating: (run, everyMs) => {
      leaseSchedule.run = run;
      leaseSchedule.everyMs = everyMs;
      leaseSchedule.cancelled = false;
      return () => {
        leaseSchedule.cancelled = true;
      };
    },
    ...options,
  });
  engine.start();
  return { engine, transport, leaseSchedule };
}

/** One governed session: a create, its receipt, and provider metadata. */
function governedSession({ status = "idle", createdAt = 1_700_000_000 } = {}) {
  const operator = newSigner();
  const provider = newSigner();
  return {
    operator,
    provider,
    events: [
      createEvent(operator, {
        sessionRef: SESSION_REF,
        providerAuthorityPubkey: provider.pubkey,
      }),
      receiptEvent(provider, { status: "created" }),
      metadataEvent(provider, {
        status,
        sessionRef: SESSION_REF,
        created_at: createdAt,
      }),
    ],
  };
}

test("history asks for exactly the D2 filters, in wire order", () => {
  const { transport } = startEngine();
  assert.deepEqual(
    transport.history().filters,
    codingSessionHistoryFilters(CHANNEL_ID),
  );
  assert.equal(transport.history().options.closeOnEose, true);
});

test("the live subscription is limit 0, h-scoped, and replays a real page", () => {
  const { transport } = startEngine();
  const live = transport.live();
  assert.deepEqual(live.filters, codingSessionLiveFilters(CHANNEL_ID));
  for (const filter of live.filters) {
    assert.equal(filter.limit, 0, "a live filter never re-pages history");
    assert.ok(filter.kinds.length > 0, "a filter without kinds is a 403");
    assert.deepEqual(filter["#h"], [CHANNEL_ID]);
  }
  assert.equal(live.options.replayLimit, 1000);
  assert.notEqual(live.options.closeOnEose, true);
});

test("a history page folds into one session with its execution", () => {
  const { engine, transport } = startEngine();
  const { events } = governedSession({ status: "running" });
  for (const event of events) transport.history().emit(event);
  transport.history().eoseAll();
  engine.flush();

  const snapshot = engine.getSnapshot();
  assert.equal(snapshot.sessions.length, 1);
  assert.equal(snapshot.sessions[0].sessionRef, SESSION_REF);
  assert.equal(snapshot.sessions[0].status, "running");
  assert.equal(snapshot.executions.length, 1);
  assert.equal(snapshot.counts.sessions, 1);
  assert.equal(snapshot.historyRead, true);
  assert.equal(snapshot.signaturesVerified, true);
});

test("a live event appends to the transcript and re-folds the status", () => {
  const { engine, transport } = startEngine();
  const { provider, events } = governedSession({ status: "running" });
  for (const event of events) transport.history().emit(event);
  transport.history().eoseAll();
  engine.flush();
  assert.equal(engine.getSnapshot().sessions[0].status, "running");

  transport.live().emit(
    transcriptEvent(provider, {
      eventSeq: 1,
      item: { kind: "assistant_text", text: "live row" },
    }),
  );
  transport.live().emit(
    metadataEvent(provider, {
      status: "stopped",
      sessionRef: SESSION_REF,
      created_at: 1_700_000_005,
    }),
  );
  engine.flush();

  const snapshot = engine.getSnapshot();
  assert.equal(snapshot.sessions[0].status, "stopped");
  const block = snapshot.transcriptBlocksByExecution.get(
    snapshot.executions[0].executionKey,
  );
  assert.equal(block.items.length, 1);
  assert.equal(block.items[0].text, "live row");
});

test("a turn receipt never creates a generation", () => {
  const { engine, transport } = startEngine();
  const operator = newSigner();
  const provider = newSigner();
  transport.history().emit(
    createEvent(operator, {
      sessionRef: SESSION_REF,
      providerAuthorityPubkey: provider.pubkey,
    }),
  );
  transport
    .history()
    .emit(receiptEvent(provider, { status: "turn_started", turnId: "turn-1" }));
  transport.history().emit(metadataEvent(provider, { status: "running" }));
  transport.history().eoseAll();
  engine.flush();

  assert.deepEqual(engine.getSnapshot().sessions, []);
});

test("a page that came back full is disclosed as truncated", () => {
  const { engine, transport } = startEngine();
  const history = transport.history();
  for (let index = 0; index < 1000; index += 1) {
    history.onEvent({ id: `pad-${index}`, kind: 44225 }, 0);
  }
  history.eoseAll();
  engine.flush();

  assert.equal(engine.getSnapshot().truncatedAt1000, true);
});

test("a short page is not disclosed as truncated", () => {
  const { engine, transport } = startEngine();
  transport.history().eoseAll();
  engine.flush();

  assert.equal(engine.getSnapshot().truncatedAt1000, false);
});

test("an unread lease never renders as nobody answering", () => {
  const transport = fakeTransport();
  const engine = new CodingSessionObserverEngine({
    wsUrl: RELAY_URL,
    channelId: CHANNEL_ID,
    subscribe: transport.subscribe,
    now: () => NOW_MS,
    scheduleRepeating: () => () => {},
  });
  engine.start();
  const { events } = governedSession({ status: "running" });
  for (const event of events) transport.history().emit(event);
  // Every filter EOSEs except the leases, so the lease read is still open.
  const filters = transport.history().filters;
  filters.forEach((filter, index) => {
    if (filter.kinds.includes(24223)) return;
    transport.history().options.onEose(index);
  });
  engine.flush();

  const snapshot = engine.getSnapshot();
  const generationId = snapshot.executions[0].activeGeneration.generationId;
  assert.equal(
    snapshot.reachabilityByGenerationId.get(generationId).reachability,
    "unknown",
  );
  assert.equal(snapshot.leasesRead, false);
});

test("a live lease for the accepted command proves the provider is answering", () => {
  const { engine, transport } = startEngine();
  const { provider, events } = governedSession({ status: "running" });
  for (const event of events) transport.history().emit(event);
  transport.history().eoseAll();
  transport
    .live()
    .emit(leaseEvent(provider, { state: "live", leaseSequence: 7 }));
  engine.flush();

  const snapshot = engine.getSnapshot();
  const generationId = snapshot.executions[0].activeGeneration.generationId;
  assert.equal(
    snapshot.reachabilityByGenerationId.get(generationId).reachability,
    "provider_reachable",
  );
});

test("leases are re-read every 60s while mounted", () => {
  const { engine, transport, leaseSchedule } = startEngine();
  transport.history().eoseAll();
  engine.flush();
  const before = transport.subscriptions.length;

  assert.equal(leaseSchedule.everyMs, 60_000);
  leaseSchedule.run();

  const reread = transport.last();
  assert.equal(transport.subscriptions.length, before + 1);
  assert.deepEqual(reread.filters, [codingSessionLeasesFilter(CHANNEL_ID)]);
  assert.equal(reread.options.closeOnEose, true);

  engine.stop();
  assert.equal(leaseSchedule.cancelled, true);
});

test("stop closes every read and the schedule", () => {
  const { engine, transport, leaseSchedule } = startEngine();
  engine.stop();
  assert.equal(transport.live().stopped, true);
  assert.equal(transport.history().stopped, true);
  assert.equal(leaseSchedule.cancelled, true);
  assert.equal(engine.getSnapshot().connection, "idle");
});

test("at most N raw events are retained per generation", () => {
  const transport = fakeTransport();
  const engine = new CodingSessionObserverEngine({
    wsUrl: RELAY_URL,
    channelId: CHANNEL_ID,
    subscribe: transport.subscribe,
    now: () => NOW_MS,
    maxRetainedRawEventsPerGeneration: 3,
    scheduleRepeating: () => () => {},
  });
  engine.start();
  const provider = newSigner();
  for (let seq = 1; seq <= 6; seq += 1) {
    transport.history().emit(
      transcriptEvent(provider, {
        eventSeq: seq,
        created_at: 1_700_000_000 + seq,
        item: { kind: "assistant_text", text: `row ${seq}` },
      }),
    );
  }
  transport.history().eoseAll();
  engine.flush();

  const retained = engine.retainedRawEvents(
    buildCodingSessionTargetKey(target()),
    provider.pubkey,
  );
  assert.equal(retained.length, 3);
  assert.deepEqual(
    retained.map((event) => event.created_at),
    [1_700_000_004, 1_700_000_005, 1_700_000_006],
  );
});

test("the connection state and the last relay error are both reported", () => {
  const { engine, transport } = startEngine();
  transport.live().options.onStateChange("open");
  assert.equal(engine.getSnapshot().connection, "open");

  transport.live().options.onStateChange("error");
  transport.live().options.onClosed("WebSocket connection failed", null);
  const snapshot = engine.getSnapshot();
  assert.equal(snapshot.connection, "error");
  assert.equal(snapshot.lastError, "WebSocket connection failed");
});

test("refresh re-reads history and resolves when the page completes", async () => {
  const { engine, transport } = startEngine();
  transport.history().eoseAll();
  engine.flush();

  const settled = engine.refresh();
  const reread = transport.subscriptions.at(-2);
  assert.deepEqual(reread.filters, codingSessionHistoryFilters(CHANNEL_ID));
  assert.equal(engine.getSnapshot().historyRead, false);
  reread.eoseAll();
  await settled;
  assert.equal(engine.getSnapshot().historyRead, true);
});

test("a socket-level failure during history rejects the waiting caller", async () => {
  const { engine, transport } = startEngine();
  const settled = engine.whenHistoryRead();
  transport.history().options.onClosed("WebSocket connection failed", null);

  await assert.rejects(settled, /WebSocket connection failed/);
  assert.equal(engine.getSnapshot().connection, "error");
  assert.equal(transport.history().stopped, true);
});
