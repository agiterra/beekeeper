import assert from "node:assert/strict";
import test from "node:test";

import {
  globalAgentConfigQueryKey,
  publishSavedGlobalAgentConfig,
} from "./useGlobalAgentConfig.ts";

function fakeCache() {
  const calls = { setQueryData: [], invalidateQueries: [] };
  return {
    calls,
    setQueryData(queryKey, data) {
      calls.setQueryData.push({ queryKey, data });
      return data;
    },
    invalidateQueries(options) {
      calls.invalidateQueries.push(options);
      return Promise.resolve();
    },
  };
}

const SAVED_CONFIG = {
  env_vars: {},
  provider: null,
  model: null,
  preferred_runtime: null,
  "allowed-bridge-pubkeys": [{ pubkey: "a".repeat(64), label: "Build box" }],
};

test("a save seeds the shared cache with the backend's canonical config", () => {
  const cache = fakeCache();
  publishSavedGlobalAgentConfig(cache, SAVED_CONFIG);

  assert.equal(cache.calls.setQueryData.length, 1);
  assert.deepEqual(
    cache.calls.setQueryData[0].queryKey,
    globalAgentConfigQueryKey,
  );
  assert.equal(cache.calls.setQueryData[0].data, SAVED_CONFIG);
});

test("a save also invalidates, or trust edits would need an app restart", () => {
  // `useGlobalAgentConfig` sets `staleTime: Infinity`, so a seed-only save
  // leaves the entry fresh forever and the query function never runs again.
  // Rust rewrites this same file behind the UI (`session_provider/trust.rs`
  // re-seeds the local provider on every start), so the invalidation is what
  // makes the on-disk allowlist authoritative rather than this render's copy.
  const cache = fakeCache();
  publishSavedGlobalAgentConfig(cache, SAVED_CONFIG);

  assert.equal(cache.calls.invalidateQueries.length, 1);
  assert.deepEqual(cache.calls.invalidateQueries[0], {
    queryKey: globalAgentConfigQueryKey,
  });
});

test("the seed happens before the invalidation, so no reader sees a gap", () => {
  const order = [];
  publishSavedGlobalAgentConfig(
    {
      setQueryData() {
        order.push("setQueryData");
      },
      invalidateQueries() {
        order.push("invalidateQueries");
        return Promise.resolve();
      },
    },
    SAVED_CONFIG,
  );

  assert.deepEqual(order, ["setQueryData", "invalidateQueries"]);
});
