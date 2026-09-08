// fetchEventsBatch on the inactive-community observer: several filters ride
// as few REQ frames as the relay's 10-filter cap allows, each frame one
// admission unit, and the caller gets one deduplicated oldest-first union.
import assert from "node:assert/strict";
import test from "node:test";

const pendingTimers = new Map();
let nextTimerId = 1;
const sends = [];

globalThis.window = {
  setTimeout: (fn, ms) => {
    const id = nextTimerId++;
    pendingTimers.set(id, { fn, fireAt: ms });
    return id;
  },
  clearTimeout: (id) => pendingTimers.delete(id),
  __TAURI_INTERNALS__: {
    invoke: async (command, args) => {
      if (command === "plugin:websocket|send") {
        sends.push(JSON.parse(args.message.data));
      }
    },
  },
};

const { ReadOnlyRelayClient } = await import("./readOnlyRelayClient.ts");

function event(id, createdAt) {
  return {
    id,
    pubkey: "p",
    created_at: createdAt,
    kind: 9,
    tags: [],
    content: "",
    sig: "s",
  };
}

function connectedClient() {
  const client = new ReadOnlyRelayClient("wss://inactive.example");
  client.wsId = 7;
  client.connect = async () => {};
  return client;
}

function deliver(client, frame) {
  return client.handleWsMessage(
    { type: "Text", data: JSON.stringify(frame) },
    client.generation,
  );
}

test("23 filters go out as REQ frames of 10, 10 and 3; the union is deduped and sorted", async () => {
  sends.length = 0;
  const client = connectedClient();
  const filters = Array.from({ length: 23 }, (_, i) => ({
    kinds: [9],
    "#h": [`ch-${i}`],
    limit: 5,
  }));

  const pending = client.fetchEventsBatch(filters);
  await new Promise((resolve) => setImmediate(resolve));
  const reqs = sends.filter((frame) => frame[0] === "REQ");
  assert.deepEqual(
    reqs.map((frame) => frame.length - 2),
    [10, 10, 3],
    "filters per REQ frame",
  );
  assert.deepEqual(reqs[0].slice(2), filters.slice(0, 10));

  await deliver(client, ["EVENT", reqs[0][1], event("b", 20)]);
  await deliver(client, ["EVENT", reqs[1][1], event("a", 10)]);
  await deliver(client, ["EVENT", reqs[2][1], event("b", 20)]);
  for (const frame of reqs) await deliver(client, ["EOSE", frame[1]]);

  assert.deepEqual(
    (await pending).map((e) => e.id),
    ["a", "b"],
  );
  assert.equal(
    sends.filter((frame) => frame[0] === "CLOSE").length,
    3,
    "each history REQ is closed after EOSE",
  );
});

test("an empty batch sends nothing", async () => {
  sends.length = 0;
  const client = connectedClient();
  assert.deepEqual(await client.fetchEventsBatch([]), []);
  assert.equal(sends.length, 0);
});
