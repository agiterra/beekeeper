import assert from "node:assert/strict";
import test from "node:test";

import { armCodingSessionDiscoveryOnConnect } from "./codingSessionDiscoveryArming.ts";

test("the first connect re-arms a read the reconnect signal would never reach", () => {
  const rearms = [];
  let emit = () => {};
  const client = {
    // The relay client stays silent here until it has connected once.
    subscribeToReconnects: () => () => {},
    subscribeToConnectionState: (listener) => {
      emit = listener;
      listener("connecting");
      return () => {};
    },
  };

  const disarm = armCodingSessionDiscoveryOnConnect(client, () =>
    rearms.push("rearm"),
  );
  assert.equal(rearms.length, 0);

  emit("connected");
  assert.equal(rearms.length, 1);

  emit("reconnecting");
  emit("connected");
  assert.equal(rearms.length, 2);

  disarm();
});

test("disarming releases both subscriptions", () => {
  const released = [];
  const client = {
    subscribeToReconnects: () => () => released.push("reconnects"),
    subscribeToConnectionState: (listener) => {
      listener("idle");
      return () => released.push("connection-state");
    },
  };

  armCodingSessionDiscoveryOnConnect(client, () => {})();
  assert.deepEqual(released.sort(), ["connection-state", "reconnects"]);
});

test("a client without connection-state reporting still arms on reconnect", () => {
  const rearms = [];
  let emitReconnect = () => {};
  const client = {
    subscribeToReconnects: (listener) => {
      emitReconnect = listener;
      return () => {};
    },
  };

  armCodingSessionDiscoveryOnConnect(client, () => rearms.push("rearm"));
  emitReconnect();
  assert.equal(rearms.length, 1);
});
