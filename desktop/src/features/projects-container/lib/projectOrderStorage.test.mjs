import assert from "node:assert/strict";
import test from "node:test";

import {
  applyProjectOrder,
  boundProjectOrderStore,
  DEFAULT_STORE,
  MAX_PROJECT_ORDER_ENTRIES,
  parseProjectOrderPayload,
  readProjectOrderStore,
  storageKey,
  writeProjectOrderStore,
} from "./projectOrderStorage.ts";

if (typeof globalThis.window === "undefined") {
  const storage = new Map();
  globalThis.window = {
    localStorage: {
      getItem: (key) => storage.get(key) ?? null,
      setItem: (key, value) => storage.set(key, value),
      removeItem: (key) => storage.delete(key),
    },
  };
}

const PUBKEY = "a".repeat(64);
const RELAY = "wss://relay.example.com";

const project = (id) => ({ id });

test("storageKey scopes to pubkey and normalized relay", () => {
  const withRelay = storageKey(PUBKEY, RELAY);
  assert.ok(withRelay.startsWith(`buzz-project-order.v1:${PUBKEY}:`));
  // The same relay written two ways must produce one key.
  assert.equal(storageKey(PUBKEY, "WSS://Relay.Example.com/"), withRelay);
  // Different relays must never collide.
  assert.notEqual(storageKey(PUBKEY, "wss://other.example.com"), withRelay);
});

test("parseProjectOrderPayload rejects anything that is not a v1 order", () => {
  assert.equal(parseProjectOrderPayload(null), null);
  assert.equal(parseProjectOrderPayload("nope"), null);
  assert.equal(parseProjectOrderPayload({ order: ["a"] }), null);
  assert.equal(parseProjectOrderPayload({ version: 2, order: ["a"] }), null);
  assert.equal(parseProjectOrderPayload({ version: 1 }), null);
});

test("parseProjectOrderPayload drops non-string and empty entries", () => {
  const parsed = parseProjectOrderPayload({
    version: 1,
    order: ["a", 7, "", null, "b"],
  });
  assert.deepEqual(parsed.order, ["a", "b"]);
});

test("boundProjectOrderStore dedupes, keeping the first (higher) position", () => {
  const bounded = boundProjectOrderStore({
    version: 1,
    order: ["a", "b", "a", "c", "b"],
  });
  assert.deepEqual(bounded.order, ["a", "b", "c"]);
});

test("boundProjectOrderStore truncates to the head of the list", () => {
  const order = Array.from(
    { length: MAX_PROJECT_ORDER_ENTRIES + 10 },
    (_, index) => `p${index}`,
  );
  const bounded = boundProjectOrderStore({ version: 1, order });
  assert.equal(bounded.order.length, MAX_PROJECT_ORDER_ENTRIES);
  assert.equal(bounded.order[0], "p0");
  assert.equal(
    bounded.order[MAX_PROJECT_ORDER_ENTRIES - 1],
    `p${MAX_PROJECT_ORDER_ENTRIES - 1}`,
  );
});

test("boundProjectOrderStore returns the same reference when nothing changes", () => {
  const store = { version: 1, order: ["a", "b"] };
  assert.equal(boundProjectOrderStore(store), store);
});

test("read/write round-trips through localStorage", () => {
  assert.equal(readProjectOrderStore(PUBKEY, RELAY), DEFAULT_STORE);
  assert.equal(
    writeProjectOrderStore(PUBKEY, { version: 1, order: ["x", "y"] }, RELAY),
    true,
  );
  assert.deepEqual(readProjectOrderStore(PUBKEY, RELAY).order, ["x", "y"]);
  // A different relay must not see it.
  assert.equal(
    readProjectOrderStore(PUBKEY, "wss://other.example.com"),
    DEFAULT_STORE,
  );
});

test("readProjectOrderStore falls back on unparseable payloads", () => {
  window.localStorage.setItem(storageKey(PUBKEY, RELAY), "{not json");
  assert.equal(readProjectOrderStore(PUBKEY, RELAY), DEFAULT_STORE);
  window.localStorage.removeItem(storageKey(PUBKEY, RELAY));
});

test("applyProjectOrder permutes into the saved order", () => {
  const projects = [project("a"), project("b"), project("c")];
  const ordered = applyProjectOrder(projects, ["c", "a", "b"]);
  assert.deepEqual(
    ordered.map((p) => p.id),
    ["c", "a", "b"],
  );
});

test("applyProjectOrder returns the input untouched for an empty order", () => {
  const projects = [project("a"), project("b")];
  assert.equal(applyProjectOrder(projects, []), projects);
});

test("applyProjectOrder skips ids that no longer resolve to a project", () => {
  const projects = [project("a"), project("b")];
  const ordered = applyProjectOrder(projects, ["deleted", "b", "gone", "a"]);
  assert.deepEqual(
    ordered.map((p) => p.id),
    ["b", "a"],
  );
});

test("applyProjectOrder appends unnamed projects in incoming order", () => {
  // Incoming order is the alphabetical base order from sortProjectContainers,
  // so a project created since the last drag lands predictably at the end.
  const projects = [project("a"), project("b"), project("new")];
  const ordered = applyProjectOrder(projects, ["b", "a"]);
  assert.deepEqual(
    ordered.map((p) => p.id),
    ["b", "a", "new"],
  );
});

test("applyProjectOrder tolerates a duplicated id in the order", () => {
  const projects = [project("a"), project("b")];
  const ordered = applyProjectOrder(projects, ["b", "b", "a"]);
  assert.deepEqual(
    ordered.map((p) => p.id),
    ["b", "a"],
  );
});
