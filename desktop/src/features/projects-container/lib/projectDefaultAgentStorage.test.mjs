import assert from "node:assert/strict";
import { test } from "node:test";

import {
  projectDefaultAgentStorageKey,
  readProjectDefaultAgentState,
} from "./projectDefaultAgentStorage.ts";

const OWNER = "a".repeat(64);
const AGENT = "b".repeat(64);

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

test("storage key scopes by pubkey and relay", () => {
  assert.equal(
    projectDefaultAgentStorageKey(OWNER, "wss://relay.example"),
    `buzz.projects.defaultAgent.v1:${OWNER}:wss://relay.example`,
  );
  assert.equal(
    projectDefaultAgentStorageKey(undefined, undefined),
    "buzz.projects.defaultAgent.v1:anon:unknown",
  );
});

test("a stored seat round-trips with pubkey lowercased", () => {
  const key = projectDefaultAgentStorageKey(OWNER, "wss://relay.example");
  const items = {
    [key]: JSON.stringify({
      [`${OWNER}:skunkworks`]: { pubkey: AGENT.toUpperCase(), role: "lead" },
    }),
  };
  withLocalStorage(items, () => {
    const state = readProjectDefaultAgentState(key);
    assert.deepEqual(state[`${OWNER}:skunkworks`], {
      pubkey: AGENT,
      role: "lead",
    });
  });
});

test("an emptied-out role reads as the builder default, not a dropped seat", () => {
  const key = "k";
  const items = {
    [key]: JSON.stringify({
      p1: { pubkey: AGENT, role: "  " },
      p2: { pubkey: AGENT },
    }),
  };
  withLocalStorage(items, () => {
    const state = readProjectDefaultAgentState(key);
    assert.equal(state.p1.role, "builder");
    assert.equal(state.p2.role, "builder");
  });
});

test("malformed blobs and entries read as empty rather than throwing", () => {
  const items = {
    a: "not json",
    b: JSON.stringify(["array"]),
    c: JSON.stringify({
      ok: { pubkey: AGENT, role: "lead" },
      "no-pubkey": { role: "lead" },
      "bad-shape": 7,
    }),
  };
  withLocalStorage(items, () => {
    assert.deepEqual(readProjectDefaultAgentState("a"), {});
    assert.deepEqual(readProjectDefaultAgentState("b"), {});
    assert.deepEqual(readProjectDefaultAgentState("missing"), {});
    const state = readProjectDefaultAgentState("c");
    assert.deepEqual(Object.keys(state), ["ok"]);
  });
});
