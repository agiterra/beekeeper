import assert from "node:assert/strict";
import test from "node:test";

import {
  applyMockCodingSessionNamingSave,
  handleMockCodingSessionNamingCommand,
} from "./e2eBridgeCodingSessionNaming.ts";

const seed = {
  titleMode: "agent",
  provider: "off",
  baseUrl: "",
  model: "",
  hasApiKey: false,
};

function reset() {
  globalThis.window = {};
}

test("unseeded, both commands stay unsupported and others pass through", () => {
  reset();
  assert.throws(
    () =>
      handleMockCodingSessionNamingCommand(
        "coding_session_naming_settings",
        null,
        undefined,
      ),
    /Unsupported mocked Tauri command: coding_session_naming_settings/,
  );
  assert.equal(
    handleMockCodingSessionNamingCommand("something_else", null, seed),
    undefined,
  );
});

test("seeded, a save is recorded with its titleMode and then read back", () => {
  reset();
  assert.deepEqual(
    handleMockCodingSessionNamingCommand(
      "coding_session_naming_settings",
      null,
      seed,
    ),
    { ...seed, hostModeMismatch: null },
  );
  const saved = handleMockCodingSessionNamingCommand(
    "set_coding_session_naming_settings",
    { provider: "off", titleMode: "off" },
    seed,
  );
  assert.equal(saved.titleMode, "off");
  assert.deepEqual(
    globalThis.window.__BEEKEEPER_E2E_CODING_SESSION_NAMING_SET_CALLS__,
    [{ provider: "off", titleMode: "off" }],
  );
  assert.equal(
    handleMockCodingSessionNamingCommand(
      "coding_session_naming_settings",
      null,
      seed,
    ).titleMode,
    "off",
  );
});

test("the mock applies the host's rules", () => {
  // Omitted mode keeps the one in force.
  assert.equal(
    applyMockCodingSessionNamingSave(seed, { provider: "anthropic" }).titleMode,
    "agent",
  );
  // my-model needs an endpoint.
  assert.throws(() =>
    applyMockCodingSessionNamingSave(seed, {
      provider: "off",
      titleMode: "my-model",
    }),
  );
  // An empty key deletes; an omitted one stays.
  const keyed = { ...seed, hasApiKey: true };
  assert.equal(
    applyMockCodingSessionNamingSave(keyed, { provider: "off", apiKey: "" })
      .hasApiKey,
    false,
  );
  assert.equal(
    applyMockCodingSessionNamingSave(keyed, { provider: "off", apiKey: null })
      .hasApiKey,
    true,
  );
});

test("a seeded host mismatch is read back until a save tells the host", () => {
  reset();
  const stale = {
    ...seed,
    titleMode: "off",
    hostModeMismatch: "this computer's agent host is not on off: p is on agent",
  };
  assert.match(
    handleMockCodingSessionNamingCommand(
      "coding_session_naming_settings",
      null,
      stale,
    ).hostModeMismatch,
    /not on off/,
  );
  const saved = handleMockCodingSessionNamingCommand(
    "set_coding_session_naming_settings",
    { provider: "off", titleMode: "off" },
    stale,
  );
  assert.equal(saved.hostModeMismatch, null);
  assert.equal(
    handleMockCodingSessionNamingCommand(
      "coding_session_naming_settings",
      null,
      stale,
    ).hostModeMismatch,
    null,
  );
});
