/**
 * The live REQ helper, against a mock socket.
 *
 * `subscribeEvents` is the only thing between the read rules and the relay, so
 * what it puts on the wire is tested literally: the D2 filters, byte for byte,
 * and the replay a reconnect has to send so an outage does not silently eat a
 * transcript.
 */
import assert from "node:assert/strict";
import test from "node:test";
import { codingSessionHistoryFilters } from "../domain/filters.ts";
import { subscribeEvents } from "../../../shared/lib/nostr-client.ts";

const CHANNEL_ID = "11111111-1111-4111-8111-111111111111";
const RELAY_URL = "wss://relay.example/";

class MockWebSocket {
  static instances = [];

  constructor(url) {
    this.url = url;
    this.readyState = 0;
    this.sent = [];
    this.listeners = new Map();
    MockWebSocket.instances.push(this);
  }

  addEventListener(type, handler) {
    const bucket = this.listeners.get(type) ?? [];
    bucket.push(handler);
    this.listeners.set(type, bucket);
  }

  send(raw) {
    this.sent.push(JSON.parse(raw));
  }

  close() {
    if (this.readyState === 3) return;
    this.readyState = 3;
    this.emit("close", {});
  }

  emit(type, event) {
    for (const handler of this.listeners.get(type) ?? []) handler(event);
  }

  accept() {
    this.readyState = 1;
    this.emit("open", {});
  }

  deliver(message) {
    this.emit("message", { data: JSON.stringify(message) });
  }

  frames(type) {
    return this.sent.filter((frame) => frame[0] === type);
  }
}

function installMockSocket() {
  MockWebSocket.instances = [];
  globalThis.WebSocket = MockWebSocket;
  return MockWebSocket;
}

const delay = (ms) => new Promise((resolve) => setTimeout(resolve, ms));
/** Long enough for the 100ms unauthenticated-REQ grace to elapse. */
const afterAuthGrace = () => delay(160);

test("one REQ per D2 filter, in wire order, exactly as the domain built it", async () => {
  const sockets = installMockSocket();
  const filters = codingSessionHistoryFilters(CHANNEL_ID);
  const stop = subscribeEvents(RELAY_URL, filters, () => {});
  const socket = sockets.instances[0];
  assert.equal(socket.url, RELAY_URL);
  socket.accept();
  await afterAuthGrace();

  const reqs = socket.frames("REQ");
  assert.equal(reqs.length, filters.length);
  assert.deepEqual(
    reqs.map((frame) => frame[2]),
    filters,
  );
  assert.equal(new Set(reqs.map((frame) => frame[1])).size, filters.length);
  stop();
});

test("the NIP-42 challenge is answered before any REQ goes out", async () => {
  const sockets = installMockSocket();
  const filters = [{ kinds: [44223], "#h": [CHANNEL_ID], limit: 1000 }];
  const stop = subscribeEvents(RELAY_URL, filters, () => {});
  const socket = sockets.instances[0];
  socket.accept();
  socket.deliver(["AUTH", "challenge-1"]);
  await delay(20);

  const auth = socket.frames("AUTH")[0];
  assert.ok(auth, "the client signs the challenge");
  assert.equal(socket.frames("REQ").length, 0);

  socket.deliver(["OK", auth[1].id, true]);
  assert.equal(socket.frames("REQ").length, 1);
  stop();
});

test("the subscription survives EOSE and keeps delivering", async () => {
  const sockets = installMockSocket();
  const seen = [];
  const stop = subscribeEvents(
    RELAY_URL,
    [{ kinds: [44225], "#h": [CHANNEL_ID], limit: 0 }],
    (event, index) => seen.push([event.id, index]),
  );
  const socket = sockets.instances[0];
  socket.accept();
  await afterAuthGrace();
  const subId = socket.frames("REQ")[0][1];

  socket.deliver(["EOSE", subId]);
  socket.deliver(["EVENT", subId, { id: "after-eose", created_at: 10 }]);

  assert.deepEqual(seen, [["after-eose", 0]]);
  assert.equal(socket.readyState, 1, "the socket stays open past EOSE");
  stop();
});

test("closeOnEose closes the socket once every filter has replayed", async () => {
  const sockets = installMockSocket();
  const states = [];
  const stop = subscribeEvents(
    RELAY_URL,
    [
      { kinds: [44223], "#h": [CHANNEL_ID], limit: 1000 },
      { kinds: [44225], "#h": [CHANNEL_ID], limit: 1000 },
    ],
    () => {},
    { closeOnEose: true, onStateChange: (state) => states.push(state) },
  );
  const socket = sockets.instances[0];
  socket.accept();
  await afterAuthGrace();
  const [first, second] = socket.frames("REQ").map((frame) => frame[1]);

  socket.deliver(["EOSE", first]);
  assert.equal(socket.readyState, 1, "one page done is not the whole read");
  socket.deliver(["EOSE", second]);

  assert.equal(socket.readyState, 3);
  assert.equal(states.at(-1), "closed");
  await delay(30);
  assert.equal(sockets.instances.length, 1, "a finished read never reconnects");
  stop();
});

test("a reconnect replays from lastSeen - 5s with a real page size", async () => {
  const sockets = installMockSocket();
  const base = { kinds: [44225], "#h": [CHANNEL_ID], limit: 0 };
  const stop = subscribeEvents(RELAY_URL, [base], () => {}, {
    replayLimit: 1000,
    reconnectDelayMs: 1,
  });
  const first = sockets.instances[0];
  first.accept();
  await afterAuthGrace();
  const subId = first.frames("REQ")[0][1];
  first.deliver(["EVENT", subId, { id: "a", created_at: 1_700_000_000 }]);
  first.deliver(["EVENT", subId, { id: "b", created_at: 1_700_000_040 }]);

  first.close();
  await delay(20);
  const second = sockets.instances[1];
  assert.ok(second, "a dropped socket is reopened");
  second.accept();
  await afterAuthGrace();

  assert.deepEqual(second.frames("REQ")[0][2], {
    kinds: [44225],
    "#h": [CHANNEL_ID],
    limit: 1000,
    since: 1_700_000_035,
  });
  stop();
});

test("a standing refusal is reported and never re-sent", async () => {
  const sockets = installMockSocket();
  const closed = [];
  const stop = subscribeEvents(
    RELAY_URL,
    [{ kinds: [44223], "#h": [CHANNEL_ID], limit: 1000 }],
    () => {},
    {
      reconnectDelayMs: 1,
      onClosed: (reason, index) => closed.push([reason, index]),
    },
  );
  const socket = sockets.instances[0];
  socket.accept();
  await afterAuthGrace();
  socket.deliver([
    "CLOSED",
    socket.frames("REQ")[0][1],
    "restricted: not a member of this group",
  ]);

  assert.deepEqual(closed, [["restricted: not a member of this group", 0]]);
  await delay(30);
  assert.equal(
    sockets.instances.length,
    1,
    "a standing refusal is not retried in a loop",
  );
  stop();
});

test("a transient refusal is retried on the next connection", async () => {
  const sockets = installMockSocket();
  const states = [];
  const base = { kinds: [44225], "#h": [CHANNEL_ID], limit: 0 };
  const stop = subscribeEvents(RELAY_URL, [base], () => {}, {
    reconnectDelayMs: 1,
    replayLimit: 1000,
    onStateChange: (state) => states.push(state),
  });
  const first = sockets.instances[0];
  first.accept();
  await afterAuthGrace();
  assert.equal(states.at(-1), "open");
  // Both of these are real relay replies, and the 100ms unauthenticated-REQ
  // grace can provoke the second one on its own.
  first.deliver([
    "CLOSED",
    first.frames("REQ")[0][1],
    "auth-required: authentication required",
  ]);

  assert.notEqual(
    states.at(-1),
    "open",
    "a refused live filter must not leave the subscription calling itself open",
  );
  await delay(30);
  const second = sockets.instances[1];
  assert.ok(second, "the subscription reconnects rather than dying silently");
  second.accept();
  await afterAuthGrace();
  assert.equal(
    second.frames("REQ").length,
    1,
    "and re-sends the filter it was refused",
  );
  stop();
});

test("a live filter reconnects with a since even before its first event", async () => {
  const sockets = installMockSocket();
  const base = { kinds: [44225], "#h": [CHANNEL_ID], limit: 0 };
  const openedAt = Math.floor(Date.now() / 1000);
  const stop = subscribeEvents(RELAY_URL, [base], () => {}, {
    replayLimit: 1000,
    reconnectDelayMs: 1,
  });
  const first = sockets.instances[0];
  first.accept();
  await afterAuthGrace();
  assert.deepEqual(
    first.frames("REQ")[0][2],
    base,
    "the first REQ is verbatim",
  );

  // Nothing arrived before the socket dropped: without a floor there is no
  // `since` to replay from and everything published in the gap is lost.
  first.close();
  await delay(20);
  const second = sockets.instances[1];
  second.accept();
  await afterAuthGrace();

  const replayed = second.frames("REQ")[0][2];
  assert.equal(replayed.limit, 1000, "and it borrows a real page size");
  assert.ok(
    replayed.since >= openedAt - 6 && replayed.since <= openedAt + 2,
    `since ${replayed.since} must be about the time the REQ went out`,
  );
  stop();
});

test("a page read is re-sent whole after a drop, never narrowed to now", async () => {
  const sockets = installMockSocket();
  const base = { kinds: [44223], "#h": [CHANNEL_ID], limit: 1000 };
  const stop = subscribeEvents(RELAY_URL, [base], () => {}, {
    closeOnEose: true,
    reconnectDelayMs: 1,
  });
  const first = sockets.instances[0];
  first.accept();
  await afterAuthGrace();
  // A relay streams a page newest-first, so the read has almost always seen an
  // event by the time the socket drops. That seen point must not narrow the
  // re-read: everything older than it is precisely what the page exists to
  // fetch, and losing it silently shrinks the count under the page size, so
  // the truncation notice never fires and a partial read reads as complete.
  first.deliver([
    "EVENT",
    first.frames("REQ")[0][1],
    { id: "newest", created_at: 1_700_000_900 },
  ]);
  first.close();
  await delay(20);
  const second = sockets.instances[1];
  second.accept();
  await afterAuthGrace();

  assert.deepEqual(
    second.frames("REQ")[0][2],
    base,
    "a filter that already asks for a page needs no gap-filling since",
  );
  stop();
});

test("unsubscribing sends CLOSE and stops reconnecting", async () => {
  const sockets = installMockSocket();
  const stop = subscribeEvents(
    RELAY_URL,
    [{ kinds: [44223], "#h": [CHANNEL_ID], limit: 1000 }],
    () => {},
    { reconnectDelayMs: 1 },
  );
  const socket = sockets.instances[0];
  socket.accept();
  await afterAuthGrace();
  const subId = socket.frames("REQ")[0][1];

  stop();

  assert.deepEqual(socket.frames("CLOSE"), [["CLOSE", subId]]);
  assert.equal(socket.readyState, 3);
  await delay(30);
  assert.equal(sockets.instances.length, 1);
});
