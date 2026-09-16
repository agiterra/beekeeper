/**
 * Which runtime a hire reads off an agent's record.
 *
 * Ledger 135(b): Kiln is Codex on the Agents screen and pins no runtime of its
 * own, because it inherits the harness from its persona. Reading the pin alone
 * sent it to `claude-primary` with an OpenAI model id.
 */
import assert from "node:assert/strict";
import { test } from "node:test";

import {
  codingSessionHireRuntimeIdLookup,
  describeCodingSessionHireModelSource,
  describeCodingSessionHireRuntimeSource,
  resolveCodingSessionHireAgentRuntime,
} from "./codingSessionHireAgentRuntime.ts";

const CATALOG = codingSessionHireRuntimeIdLookup([
  { id: "claude", command: "claude-code-acp" },
  { id: "codex", command: "codex-acp" },
  { id: "goose", command: null },
]);

test("a record that pins a runtime is taken at its word", () => {
  assert.deepEqual(
    resolveCodingSessionHireAgentRuntime({
      runtime: "Codex",
      agentCommand: "claude-code-acp",
      runtimeIdForCommand: CATALOG,
    }),
    { runtime: "codex", source: "record", read: "Codex" },
  );
});

test("an inherited harness names the runtime the Agents screen shows", () => {
  assert.deepEqual(
    resolveCodingSessionHireAgentRuntime({
      runtime: null,
      agentCommand: "codex-acp",
      provider: null,
      runtimeIdForCommand: CATALOG,
    }),
    { runtime: "codex", source: "harness", read: "codex-acp" },
  );
});

test("a command that is itself a runtime id resolves the same way", () => {
  assert.equal(
    resolveCodingSessionHireAgentRuntime({
      agentCommand: "goose",
      runtimeIdForCommand: CATALOG,
    }).runtime,
    "goose",
  );
});

test("the inference provider is the last fallback, and says so", () => {
  assert.deepEqual(
    resolveCodingSessionHireAgentRuntime({
      runtime: null,
      agentCommand: "some-unknown-harness",
      provider: "anthropic",
      runtimeIdForCommand: CATALOG,
    }),
    { runtime: "anthropic", source: "provider", read: "anthropic" },
  );
});

test("no catalog read means the harness tier answers nothing, never a guess", () => {
  assert.deepEqual(
    resolveCodingSessionHireAgentRuntime({
      runtime: null,
      agentCommand: "codex-acp",
      provider: null,
    }),
    { runtime: null, source: null, read: null },
  );
});

test("a record that names nothing names nothing", () => {
  assert.deepEqual(
    resolveCodingSessionHireAgentRuntime({
      runtime: "  ",
      agentCommand: "",
      provider: null,
      runtimeIdForCommand: CATALOG,
    }),
    { runtime: null, source: null, read: null },
  );
});

// --- Ledger 139: the native effective_runtime field ------------------------

test("an inherited harness's native effective_runtime is preferred over the catalog reconstruction", () => {
  assert.deepEqual(
    resolveCodingSessionHireAgentRuntime({
      runtime: null,
      effectiveRuntime: "Codex",
      effectiveRuntimeSource: "definition",
      // A wrong/absent catalog must not matter: the native field answers
      // before the catalog reconstruction ever runs.
      agentCommand: "some-unknown-harness",
      provider: null,
    }),
    { runtime: "codex", source: "harness", read: "Codex" },
  );
});

test("a native effective_runtime pinned on the instance itself reports as record", () => {
  assert.deepEqual(
    resolveCodingSessionHireAgentRuntime({
      runtime: "codex",
      effectiveRuntime: "codex",
      effectiveRuntimeSource: "instance",
      agentCommand: null,
      provider: null,
    }),
    { runtime: "codex", source: "record", read: "codex" },
  );
});

test("a host that has not republished effective_runtime falls back to the catalog chain unchanged", () => {
  assert.deepEqual(
    resolveCodingSessionHireAgentRuntime({
      runtime: null,
      effectiveRuntime: null,
      effectiveRuntimeSource: null,
      agentCommand: "codex-acp",
      provider: null,
      runtimeIdForCommand: CATALOG,
    }),
    { runtime: "codex", source: "harness", read: "codex-acp" },
  );
});

test("each tier has a sentence a person can act on", () => {
  assert.match(
    describeCodingSessionHireRuntimeSource("record", "codex"),
    /set on its record on this computer/,
  );
  assert.match(
    describeCodingSessionHireRuntimeSource("harness", "codex-acp"),
    /harness its record resolves to, codex-acp/,
  );
  assert.match(
    describeCodingSessionHireRuntimeSource("provider", "anthropic"),
    /inference provider, anthropic/,
  );
});

test("a model's tier is named, and an unreported tier claims no file", () => {
  assert.match(
    describeCodingSessionHireModelSource("definition"),
    /the persona its record is linked to names/,
  );
  assert.match(
    describeCodingSessionHireModelSource("instance"),
    /its own record on this computer names/,
  );
  assert.match(
    describeCodingSessionHireModelSource("global"),
    /this computer's default model is/,
  );
  assert.match(
    describeCodingSessionHireModelSource(null),
    /the record this computer resolved for it says/,
  );
});
