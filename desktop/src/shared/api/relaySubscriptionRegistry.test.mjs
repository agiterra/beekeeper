import assert from "node:assert/strict";
import test from "node:test";

import {
  RelaySubscriptionRegistry,
  subscriptionKey,
} from "./relaySubscriptionRegistry.ts";

function event(id, createdAt = 1) {
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

/** A fake transport: records opens/closes and exposes the fan-out. */
function fakeOpener() {
  const opened = [];
  const closed = [];
  const open = async (filters, onEvent, onReady) => {
    const entry = { filters, onEvent, onReady };
    opened.push(entry);
    return async () => {
      closed.push(filters);
    };
  };
  return { opened, closed, open };
}

test("subscriptionKey ignores key and value order but not since/limit", () => {
  const a = subscriptionKey([
    { kinds: [9, 7], "#h": ["b", "a"], limit: 0, since: 5 },
  ]);
  const b = subscriptionKey([
    { since: 5, limit: 0, "#h": ["a", "b"], kinds: [7, 9] },
  ]);
  assert.equal(a, b);
  assert.notEqual(
    a,
    subscriptionKey([{ kinds: [9, 7], "#h": ["b", "a"], limit: 0, since: 6 }]),
  );
  assert.notEqual(
    a,
    subscriptionKey([{ kinds: [9, 7], "#h": ["b", "a"], limit: 1, since: 5 }]),
  );
});

test("two joins for the same filters open one REQ; CLOSE only on the last leave", async () => {
  const registry = new RelaySubscriptionRegistry();
  const { opened, closed, open } = fakeOpener();
  const filters = [{ kinds: [9], "#h": ["ch-1"], limit: 0 }];
  const seenA = [];
  const seenB = [];

  const leaveA = await registry.join(
    filters,
    (e) => seenA.push(e.id),
    undefined,
    open,
  );
  const leaveB = await registry.join(
    [{ "#h": ["ch-1"], kinds: [9], limit: 0 }],
    (e) => seenB.push(e.id),
    undefined,
    open,
  );
  assert.equal(opened.length, 1, "one underlying subscription");
  assert.equal(registry.refCount(filters), 2);
  assert.equal(registry.size(), 1);

  opened[0].onEvent(event("live-1"));
  assert.deepEqual(seenA, ["live-1"]);
  assert.deepEqual(seenB, ["live-1"]);

  await leaveA();
  assert.equal(closed.length, 0, "B still holds it");
  assert.equal(registry.refCount(filters), 1);
  opened[0].onEvent(event("live-2"));
  assert.deepEqual(seenA, ["live-1"], "A left");
  assert.deepEqual(seenB, ["live-1", "live-2"]);

  await leaveB();
  assert.equal(closed.length, 1, "last leave sends CLOSE");
  assert.equal(registry.size(), 0);
  await leaveB();
  assert.equal(closed.length, 1, "leaving twice is idempotent");
});

test("a late joiner receives the newest `limit` events already delivered, then live", async () => {
  const registry = new RelaySubscriptionRegistry();
  const { opened, open } = fakeOpener();
  const filters = [{ kinds: [9], "#h": ["ch-1"], limit: 2 }];
  await registry.join(filters, () => {}, undefined, open);
  opened[0].onEvent(event("old-1", 1));
  opened[0].onEvent(event("old-2", 2));
  opened[0].onEvent(event("old-3", 3));

  const seen = [];
  await registry.join(filters, (e) => seen.push(e.id), undefined, open);
  assert.deepEqual(seen, ["old-2", "old-3"], "ring bounded by limit");
  opened[0].onEvent(event("live", 4));
  assert.deepEqual(seen, ["old-2", "old-3", "live"]);
});

test("readiness is shared: a joiner after EOSE is told at once, one before it waits", async () => {
  const registry = new RelaySubscriptionRegistry();
  const { opened, open } = fakeOpener();
  const filters = [{ kinds: [9], limit: 0 }];
  const readiness = [];
  await registry.join(
    filters,
    () => {},
    (r) => readiness.push(["first", r]),
    open,
  );
  const joinBefore = registry.join(
    filters,
    () => {},
    (r) => readiness.push(["early", r]),
    open,
  );
  assert.deepEqual(readiness, []);
  opened[0].onReady("eose");
  await joinBefore;
  assert.deepEqual(readiness, [
    ["first", "eose"],
    ["early", "eose"],
  ]);
  await registry.join(
    filters,
    () => {},
    (r) => readiness.push(["late", r]),
    open,
  );
  assert.deepEqual(readiness[2], ["late", "eose"]);
});

test("the same handler joining twice is two members", async () => {
  const registry = new RelaySubscriptionRegistry();
  const { closed, open } = fakeOpener();
  const filters = [{ kinds: [9], limit: 0 }];
  const handler = () => {};
  const leave1 = await registry.join(filters, handler, undefined, open);
  const leave2 = await registry.join(filters, handler, undefined, open);
  assert.equal(registry.refCount(filters), 2);
  await leave1();
  assert.equal(closed.length, 0);
  await leave2();
  assert.equal(closed.length, 1);
});

test("a failed open removes the entry so the next join retries", async () => {
  const registry = new RelaySubscriptionRegistry();
  let attempts = 0;
  const open = async () => {
    attempts++;
    if (attempts === 1) throw new Error("socket down");
    return async () => {};
  };
  const filters = [{ kinds: [9], limit: 0 }];
  await assert.rejects(
    registry.join(filters, () => {}, undefined, open),
    /socket down/,
  );
  assert.equal(registry.size(), 0);
  await registry.join(filters, () => {}, undefined, open);
  assert.equal(attempts, 2);
  assert.equal(registry.size(), 1);
});
