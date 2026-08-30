import assert from "node:assert/strict";
import test, { mock } from "node:test";

import { relayClient } from "@/shared/api/relayClient";
import {
  makeFakeWindow,
  installFakeWindow,
  installTauriMock,
} from "@/features/sidebar/lib/sidebarSyncTestHelpers.mjs";
import { ProjectOrderSyncManager } from "./projectOrderSync.ts";

function makeOrderStore(order = []) {
  return { version: 1, order };
}

const RELAY = "wss://r.test";
const RELAY_KEY = encodeURIComponent(RELAY);
const watermarkKey = (pubkey) =>
  `buzz-sync-watermark.v1:project-order:${pubkey}:${RELAY_KEY}`;

// ─── destroy() must cancel a pending publish, not flush it ───────────────────

// Community-switch cross-relay publish vector: reorder in relay A → destroy()
// → nothing may reach the shared relayClient, which now points at relay B.
test("destroy: cancels pending publish without flushing to the relay", () => {
  mock.method(relayClient, "fetchEvents", () => Promise.resolve([]));
  const publishCalls = [];
  mock.method(relayClient, "publishEvent", (...args) => {
    publishCalls.push(args);
    return Promise.resolve();
  });
  const fw = makeFakeWindow();
  const restore = installFakeWindow(fw);
  try {
    const manager = new ProjectOrderSyncManager("pk-test", RELAY);
    manager.publishOrder(makeOrderStore(["a", "b"]));
    assert.ok(fw._hasTimer(), "debounce timer should be set");
    manager.destroy();
    assert.ok(!fw._hasTimer(), "debounce timer should be cleared on destroy");
    assert.equal(publishCalls.length, 0);
    assert.equal(manager.getPendingStore(), null);
  } finally {
    restore();
    mock.reset();
  }
});

test("destroy: aborts in-flight doPublish after fetchOwnBlobBeforePublish resolves", async () => {
  let releaseFetch = null;
  const publishCalls = [];
  mock.method(
    relayClient,
    "fetchEvents",
    () =>
      new Promise((res) => {
        releaseFetch = () => res([]);
      }),
  );
  mock.method(relayClient, "publishEvent", (...args) => {
    publishCalls.push(args);
    return Promise.resolve();
  });
  const fw = makeFakeWindow();
  const restore = installFakeWindow(fw);
  try {
    const manager = new ProjectOrderSyncManager("pk-race", RELAY);
    manager.publishOrder(makeOrderStore(["a"]));
    fw._fireTimer(); // doPublish now awaits fetchOwnBlobBeforePublish
    manager.destroy();
    releaseFetch();
    await new Promise((r) => setTimeout(r, 0));
    assert.equal(
      publishCalls.length,
      0,
      "publishEvent must not fire after destroy",
    );
  } finally {
    restore();
    mock.reset();
  }
});

// ─── bootstrap seed/hold policy ──────────────────────────────────────────────

test("bootstrap: a failed fetch never seed-publishes", async () => {
  mock.method(relayClient, "fetchEvents", () =>
    Promise.reject(new Error("relay timeout")),
  );
  mock.method(relayClient, "publishEvent", () => Promise.resolve());
  const fw = makeFakeWindow();
  const restore = installFakeWindow(fw);
  try {
    const manager = new ProjectOrderSyncManager("pk-fail", RELAY);
    const result = await manager.bootstrap(makeOrderStore(["a"]));
    assert.equal(result.action, "hold");
    assert.equal(manager.getPendingStore(), null);
  } finally {
    restore();
    mock.reset();
  }
});

test("bootstrap: absent fetch with a prior watermark blocks seed-publish", async () => {
  mock.method(relayClient, "fetchEvents", () => Promise.resolve([]));
  mock.method(relayClient, "publishEvent", () => Promise.resolve());
  const fw = makeFakeWindow();
  fw.localStorage.setItem(watermarkKey("pk-stale"), "1700000000");
  const restore = installFakeWindow(fw);
  try {
    const manager = new ProjectOrderSyncManager("pk-stale", RELAY);
    const result = await manager.bootstrap(makeOrderStore(["a"]));
    assert.equal(result.action, "hold");
    assert.equal(manager.getPendingStore(), null);
  } finally {
    restore();
    mock.reset();
  }
});

test("bootstrap: absent fetch with zero watermark seeds a non-empty local order", async () => {
  mock.method(relayClient, "fetchEvents", () => Promise.resolve([]));
  mock.method(relayClient, "publishEvent", () => Promise.resolve());
  const fw = makeFakeWindow();
  const restore = installFakeWindow(fw);
  try {
    const manager = new ProjectOrderSyncManager("pk-fresh", RELAY);
    const result = await manager.bootstrap(makeOrderStore(["a", "b"]));
    assert.equal(result.action, "hold");
    assert.ok(manager.getPendingStore() !== null);
  } finally {
    restore();
    mock.reset();
  }
});

test("bootstrap: an empty local order never seeds", async () => {
  // A user who has never dragged anything must not publish a blob just by
  // opening the app — the alphabetical base order already reads correctly.
  mock.method(relayClient, "fetchEvents", () => Promise.resolve([]));
  mock.method(relayClient, "publishEvent", () => Promise.resolve());
  const fw = makeFakeWindow();
  const restore = installFakeWindow(fw);
  try {
    const manager = new ProjectOrderSyncManager("pk-empty", RELAY);
    const result = await manager.bootstrap(makeOrderStore([]));
    assert.equal(result.action, "hold");
    assert.equal(manager.getPendingStore(), null);
  } finally {
    restore();
    mock.reset();
  }
});

// ─── whole-blob LWW ──────────────────────────────────────────────────────────

// Guards the `headBeforeFetch` snapshot: comparing against the already-advanced
// watermark would make 200 > 200 false, silently publishing the local order
// over a newer remote one.
test("LWW: a newer remote order wins over the local one at publish time", async () => {
  let callCount = 0;
  mock.method(relayClient, "fetchEvents", () => {
    callCount++;
    return Promise.resolve([
      {
        pubkey: "pk-lww",
        content: callCount === 1 ? "bad-cipher" : "good-cipher",
        created_at: callCount === 1 ? 100 : 200,
        id: `evt-${callCount}`,
      },
    ]);
  });
  mock.method(relayClient, "publishEvent", () => Promise.resolve());
  const fw = makeFakeWindow();
  const restore = installFakeWindow(fw);
  const tauri = installTauriMock(
    JSON.stringify({ version: 1, order: ["remote-1", "remote-2"] }),
  );
  try {
    const manager = new ProjectOrderSyncManager("pk-lww", RELAY);
    await manager.fetchRemoteOrder();
    assert.ok(
      Number(fw.localStorage.getItem(watermarkKey("pk-lww")) ?? "0") >= 100,
    );
    manager.publishOrder(makeOrderStore(["local-1"]));
    fw._fireTimer();
    await new Promise((r) => setTimeout(r, 20));
    const plaintext = tauri.capturedPlaintext();
    assert.ok(plaintext !== null, "nip44EncryptToSelf must have been called");
    assert.deepEqual(
      JSON.parse(plaintext).order,
      ["remote-1", "remote-2"],
      `remote order must win the LWW merge — got: ${plaintext}`,
    );
  } finally {
    tauri.restore();
    restore();
    mock.reset();
  }
});

// ─── live subscription ───────────────────────────────────────────────────────

test("live: an undecryptable event advances the watermark before the decrypt attempt", async () => {
  let liveCallback = null;
  mock.method(relayClient, "subscribeLive", (_filter, onEvent) => {
    liveCallback = onEvent;
    return Promise.resolve(async () => {});
  });
  const fw = makeFakeWindow();
  const restore = installFakeWindow(fw);
  try {
    const manager = new ProjectOrderSyncManager("pk-live", RELAY);
    assert.equal(
      fw.localStorage.getItem(watermarkKey("pk-live")),
      null,
      "watermark starts absent",
    );
    await manager.subscribeToOrder(() => {});
    assert.ok(liveCallback !== null);
    liveCallback({
      pubkey: "pk-live",
      content: "!bad-cipher!",
      created_at: 1700005555,
      id: "live-evt-1",
    });
    await new Promise((r) => setTimeout(r, 0));
    assert.ok(
      Number(fw.localStorage.getItem(watermarkKey("pk-live")) ?? "0") >=
        1700005555,
    );
  } finally {
    restore();
    mock.reset();
  }
});

test("live: a decryptable event is handed to the subscriber", async () => {
  let liveCallback = null;
  mock.method(relayClient, "subscribeLive", (_filter, onEvent) => {
    liveCallback = onEvent;
    return Promise.resolve(async () => {});
  });
  const fw = makeFakeWindow();
  const restore = installFakeWindow(fw);
  const tauri = installTauriMock(
    JSON.stringify({ version: 1, order: ["b", "a"] }),
  );
  try {
    const manager = new ProjectOrderSyncManager("pk-live2", RELAY);
    const seen = [];
    await manager.subscribeToOrder((remote) => seen.push(remote));
    liveCallback({
      pubkey: "pk-live2",
      content: "good-cipher",
      created_at: 1700006666,
      id: "live-evt-2",
    });
    await new Promise((r) => setTimeout(r, 0));
    assert.equal(seen.length, 1);
    assert.deepEqual(seen[0].store.order, ["b", "a"]);
    assert.equal(seen[0].createdAt, 1700006666);
    assert.equal(seen[0].eventId, "live-evt-2");
  } finally {
    tauri.restore();
    restore();
    mock.reset();
  }
});

test("live: an event from another author is ignored", async () => {
  let liveCallback = null;
  mock.method(relayClient, "subscribeLive", (_filter, onEvent) => {
    liveCallback = onEvent;
    return Promise.resolve(async () => {});
  });
  const fw = makeFakeWindow();
  const restore = installFakeWindow(fw);
  const tauri = installTauriMock(JSON.stringify({ version: 1, order: ["x"] }));
  try {
    const manager = new ProjectOrderSyncManager("pk-mine", RELAY);
    const seen = [];
    await manager.subscribeToOrder((remote) => seen.push(remote));
    liveCallback({
      pubkey: "pk-someone-else",
      content: "good-cipher",
      created_at: 1700007777,
      id: "live-evt-3",
    });
    await new Promise((r) => setTimeout(r, 0));
    assert.equal(seen.length, 0);
    assert.equal(fw.localStorage.getItem(watermarkKey("pk-mine")), null);
  } finally {
    tauri.restore();
    restore();
    mock.reset();
  }
});
