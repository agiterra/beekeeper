import assert from "node:assert/strict";
import test from "node:test";

import {
  boundProjectLastRouteState,
  DM_LAST_ROUTE_KEY,
  MAX_REMEMBERED_ROUTES,
  parseProjectLastRouteState,
  projectLastRouteStorageKey,
  readProjectLastRouteState,
  withProjectLastRoute,
  writeProjectLastRouteState,
} from "./projectLastRouteStorage.ts";

const OWNER = "a".repeat(64);

function withLocalStorage(items, run) {
  const previous = globalThis.window;
  globalThis.window = {
    localStorage: {
      getItem: (key) => (key in items ? items[key] : null),
      setItem: (key, value) => {
        items[key] = value;
      },
    },
  };
  try {
    return run();
  } finally {
    globalThis.window = previous;
  }
}

test("the key scopes by pubkey and relay", () => {
  assert.equal(
    projectLastRouteStorageKey(OWNER, "wss://relay.example"),
    `buzz.projects.lastRoute.v1:${OWNER}:wss://relay.example`,
  );
  assert.equal(
    projectLastRouteStorageKey(undefined, undefined),
    "buzz.projects.lastRoute.v1:anon:unknown",
  );
});

test("a stored blob round-trips through storage", () => {
  const items = {};
  withLocalStorage(items, () => {
    writeProjectLastRouteState(OWNER, "wss://relay.example", {
      "owner:beekeeper": { href: "/coding-sessions/c/g", at: 10 },
    });
    assert.deepEqual(readProjectLastRouteState(OWNER, "wss://relay.example"), {
      "owner:beekeeper": { href: "/coding-sessions/c/g", at: 10 },
    });
  });
});

test("the direct-messages slot is just another id", () => {
  const state = withProjectLastRoute({}, DM_LAST_ROUTE_KEY, "/channels/x", 1);
  assert.equal(state[DM_LAST_ROUTE_KEY].href, "/channels/x");
});

test("entries without a usable href are dropped, not kept as blanks", () => {
  assert.deepEqual(
    parseProjectLastRouteState(
      JSON.stringify({
        good: { href: "/projects/a", at: 3 },
        blank: { href: "" },
        wrong: "not an object",
      }),
    ),
    { good: { href: "/projects/a", at: 3 } },
  );
});

test("a missing or corrupt blob reads as empty rather than throwing", () => {
  assert.deepEqual(parseProjectLastRouteState(null), {});
  assert.deepEqual(parseProjectLastRouteState("{{{"), {});
  assert.deepEqual(parseProjectLastRouteState("[]"), {});
});

// Re-recording the same page on every render tick would be a storage write per
// render; the identity return is what makes the caller's skip possible.
test("recording an unchanged href returns the same state reference", () => {
  const state = { p: { href: "/projects/p", at: 1 } };
  assert.equal(withProjectLastRoute(state, "p", "/projects/p", 2), state);
});

test("a moved href replaces the entry", () => {
  const state = withProjectLastRoute(
    { p: { href: "/projects/p", at: 1 } },
    "p",
    "/projects/p/pulse",
    5,
  );
  assert.deepEqual(state.p, { href: "/projects/p/pulse", at: 5 });
});

test("the map is bounded, and the least recently visited go first", () => {
  const state = {};
  for (let index = 0; index < MAX_REMEMBERED_ROUTES + 5; index += 1) {
    state[`p${index}`] = { href: `/projects/p${index}`, at: index };
  }
  const bounded = boundProjectLastRouteState(state);
  assert.equal(Object.keys(bounded).length, MAX_REMEMBERED_ROUTES);
  assert.ok(!("p0" in bounded));
  assert.ok(`p${MAX_REMEMBERED_ROUTES + 4}` in bounded);
});
