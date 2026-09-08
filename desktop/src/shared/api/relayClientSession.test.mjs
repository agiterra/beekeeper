// Session-level behaviour of the relay transport under the send budget and
// the rate-limit gate: multi-filter REQ shape, subscription dedupe, typing
// frames dropped under pressure, and the one rate-limit retry on publish.
import assert from "node:assert/strict";
import test from "node:test";

let fakeNow = 0;
const pendingTimers = new Map();
let nextTimerId = 1;
const frames = [];
let signCounter = 0;

globalThis.window = {
  setTimeout: (fn, ms) => {
    const id = nextTimerId++;
    pendingTimers.set(id, { fn, fireAt: fakeNow + ms });
    return id;
  },
  clearTimeout: (id) => pendingTimers.delete(id),
  __TAURI_INTERNALS__: {
    invoke: async (command, args) => {
      if (command === "plugin:websocket|send") {
        frames.push(JSON.parse(args.message.data));
        return;
      }
      if (command === "sign_event") {
        signCounter++;
        return JSON.stringify({
          id: `${signCounter}`.padStart(64, "0"),
          pubkey: "me",
          created_at: Math.floor(fakeNow / 1000),
          kind: args.kind,
          tags: args.tags,
          content: args.content,
          sig: "sig",
        });
      }
      throw new Error(`unexpected invoke ${command}`);
    },
  },
};
Date.now = () => fakeNow;

const { RelayClient } = await import("./relayClientFeatureApi.ts");
const { activateRateLimit, isRateLimited, resetRateLimitGate } = await import(
  "./relayRateLimitGate.ts"
);
const { relaySendBudget, resetRelaySendBudget } = await import(
  "./relaySendBudget.ts"
);

function tickTo(ms) {
  fakeNow = ms;
  for (const [id, { fn, fireAt }] of Array.from(pendingTimers.entries())) {
    if (fireAt <= fakeNow) {
      pendingTimers.delete(id);
      fn();
    }
  }
}

function reset() {
  resetRateLimitGate();
  resetRelaySendBudget();
  fakeNow = 0;
  pendingTimers.clear();
  nextTimerId = 1;
  frames.length = 0;
}

/** A client whose socket is "up" without going through connect(). */
function connectedClient() {
  const client = new RelayClient();
  client.wsId = 7;
  client.ensureConnected = async () => client.connectionGeneration;
  return client;
}

function deliver(client, frame) {
  return client.handleWsMessage(
    { type: "Text", data: JSON.stringify(frame) },
    client.connectionGeneration,
  );
}

async function flushUntil(predicate, attempts = 50) {
  for (let attempt = 0; attempt < attempts; attempt++) {
    if (predicate()) return;
    await new Promise((resolve) => setImmediate(resolve));
  }
  assert.fail("condition did not become true before the flush limit");
}

const reqFrames = () => frames.filter((frame) => frame[0] === "REQ");
const eventFrames = () => frames.filter((frame) => frame[0] === "EVENT");

test("subscribeLiveMany sends one REQ carrying every filter", async () => {
  reset();
  const client = connectedClient();
  const filters = [
    { kinds: [9], "#h": ["ch-1", "ch-2"], limit: 0 },
    { kinds: [40002], "#p": ["me"], limit: 0 },
    { kinds: [7], "#h": ["ch-3"], limit: 5 },
  ];
  const subscribed = client.subscribeLiveMany(filters, () => {});
  await flushUntil(() => reqFrames().length === 1);
  const [, subId, ...sent] = reqFrames()[0];
  assert.deepEqual(sent, filters, "exact 3-filter REQ shape");
  await deliver(client, ["EOSE", subId]);
  const leave = await subscribed;
  await leave();
  assert.deepEqual(
    frames.at(-1),
    ["CLOSE", subId],
    "leaving the only subscription sends CLOSE",
  );
});

test("subscribeLiveMany refuses 0 or more than 10 filters", async () => {
  reset();
  const client = connectedClient();
  await assert.rejects(
    client.subscribeLiveMany([], () => {}),
    /1 to 10/,
  );
  await assert.rejects(
    client.subscribeLiveMany(
      Array.from({ length: 11 }, () => ({ kinds: [9], limit: 0 })),
      () => {},
    ),
    /1 to 10/,
  );
  assert.equal(frames.length, 0);
});

test("two identical live subscriptions share one REQ and both receive events", async () => {
  reset();
  const client = connectedClient();
  const filter = { kinds: [9], "#h": ["ch-1"], limit: 0, since: 100 };
  const seenA = [];
  const seenB = [];
  const a = client.subscribeLive({ ...filter }, (e) => seenA.push(e.id));
  const b = client.subscribeLive({ ...filter }, (e) => seenB.push(e.id));
  await flushUntil(() => reqFrames().length === 1);
  const subId = reqFrames()[0][1];
  await deliver(client, ["EOSE", subId]);
  const leaveA = await a;
  const leaveB = await b;
  assert.equal(reqFrames().length, 1, "the second mount reused the REQ");

  await deliver(client, [
    "EVENT",
    subId,
    {
      id: "e1",
      pubkey: "p",
      created_at: 200,
      kind: 9,
      tags: [["h", "ch-1"]],
      content: "",
      sig: "s",
    },
  ]);
  tickTo(20); // EVENT_BATCH_MS flush
  assert.deepEqual(seenA, ["e1"]);
  assert.deepEqual(seenB, ["e1"]);

  await leaveA();
  assert.equal(frames.filter((f) => f[0] === "CLOSE").length, 0);
  await leaveB();
  assert.equal(frames.filter((f) => f[0] === "CLOSE").length, 1);
});

test("typing indicators are dropped while the gate is armed", async () => {
  reset();
  const client = connectedClient();
  activateRateLimit(4);
  await client.sendTypingIndicator("ch-1");
  await new Promise((resolve) => setImmediate(resolve));
  assert.equal(eventFrames().length, 0, "no frame under the gate");
  tickTo(4_001);
  assert.equal(isRateLimited(), false);
  await client.sendTypingIndicator("ch-1");
  await flushUntil(() => eventFrames().length === 1);
  assert.equal(eventFrames()[0][1].kind, 20002);
});

test("typing indicators are dropped when the ephemeral lane has no slot", async () => {
  reset();
  const client = connectedClient();
  const budget = relaySendBudget();
  while (budget.tryAcquire("ephemeral")) {
    // exhaust the lane
  }
  await client.sendTypingIndicator("ch-1");
  await new Promise((resolve) => setImmediate(resolve));
  assert.equal(eventFrames().length, 0, "dropped, not queued");
  assert.equal(
    budget.tryAcquire("write"),
    true,
    "the write reserve is untouched by the drops",
  );
});

test("a rate-limited OK is retried once after the gate expires", async () => {
  reset();
  const client = connectedClient();
  const event = { id: "a".repeat(64), kind: 40002 };
  const published = client.publishEvent(event, "timed out", "send failed");
  await flushUntil(() => eventFrames().length === 1);

  await deliver(client, [
    "OK",
    event.id,
    false,
    "rate-limited: message quota exceeded; retry in 3s",
  ]);
  assert.equal(isRateLimited(), true, "the refusal armed the gate");
  await new Promise((resolve) => setImmediate(resolve));
  assert.equal(eventFrames().length, 1, "no retry while the gate is armed");

  tickTo(3_001);
  await flushUntil(() => eventFrames().length === 2);
  assert.deepEqual(eventFrames()[1][1], event, "the same signed event again");
  await deliver(client, ["OK", event.id, true, ""]);
  assert.equal(await published, event);
});

test("a second rate-limited OK is final", async () => {
  reset();
  const client = connectedClient();
  const event = { id: "b".repeat(64), kind: 40002 };
  const published = client.publishEvent(event, "timed out", "send failed");
  await flushUntil(() => eventFrames().length === 1);
  await deliver(client, ["OK", event.id, false, "rate-limited: retry in 1s"]);
  tickTo(1_001);
  await flushUntil(() => eventFrames().length === 2);
  await deliver(client, ["OK", event.id, false, "rate-limited: retry in 1s"]);
  await assert.rejects(published, /rate-limited/);
  assert.equal(client.pendingEvents.size, 0);
});

test("a rate-limited OK for an untracked event still arms the gate", async () => {
  reset();
  const client = connectedClient();
  await deliver(client, [
    "OK",
    "c".repeat(64),
    false,
    "rate-limited: ephemeral quota exceeded; retry in 2s",
  ]);
  assert.equal(isRateLimited(), true);
});

test("a REQ waits for the send budget's read lane and is sent when a slot frees", async () => {
  reset();
  const client = connectedClient();
  const budget = relaySendBudget();
  while (budget.tryAcquire("read")) {
    // fill the read lane at t=0
  }
  const pending = client.fetchEvents({ kinds: [9], limit: 1 });
  await new Promise((resolve) => setImmediate(resolve));
  assert.equal(reqFrames().length, 0, "held by the budget");
  tickTo(5_001);
  await flushUntil(() => reqFrames().length === 1);
  await deliver(client, ["EOSE", reqFrames()[0][1]]);
  assert.deepEqual(await pending, []);
});
