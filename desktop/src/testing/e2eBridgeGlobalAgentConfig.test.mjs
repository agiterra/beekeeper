/**
 * The mock bridge's unseeded `get_global_agent_config` must be a config the
 * product can actually read.
 *
 * It was not. The fallback omitted `allowed-bridge-pubkeys`, so the moment
 * Settings > Agents mounted, `trustRowsFromEntries`
 * (`src/features/coding-sessions/lib/codingSessionTrust.ts:94`) ran
 * `entries.map` on `undefined` inside the `useState` initializer of
 * `CodingSessionTrustFields` (`src/features/coding-sessions/ui/
 * CodingSessionTrustFields.tsx:51`), the app-level error boundary replaced the
 * window with "Something went wrong!", and roughly sixty smoke tests failed as
 * if the product were broken (ledger item 91 lane 6).
 *
 * Rust always emits the key — `#[serde(default, rename =
 * "allowed-bridge-pubkeys")]` on a `Vec` with no `skip_serializing_if`
 * (`desktop/src-tauri/src/managed_agents/global_config/mod.rs:103`) — and
 * `GlobalAgentConfig` declares it required (`src/shared/api/types.ts:964`).
 * Only the mock could produce that shape, so the assertion belongs here.
 */
import assert from "node:assert/strict";
import test from "node:test";

import { trustRowsFromEntries } from "@/features/coding-sessions/lib/codingSessionTrust";
import { MOCK_GLOBAL_AGENT_CONFIG_FALLBACK } from "./e2eBridge.ts";

test("the unseeded fallback carries every key GlobalAgentConfig declares", () => {
  assert.deepEqual(Object.keys(MOCK_GLOBAL_AGENT_CONFIG_FALLBACK).sort(), [
    "allowed-bridge-pubkeys",
    "env_vars",
    "model",
    "preferred_runtime",
    "provider",
  ]);
});

test("the product's own reader survives the fallback untouched", () => {
  // This is the exact call that threw: the trust field's seed.
  const rows = trustRowsFromEntries(
    MOCK_GLOBAL_AGENT_CONFIG_FALLBACK["allowed-bridge-pubkeys"],
  );
  assert.deepEqual(rows, []);
});
