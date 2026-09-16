import assert from "node:assert/strict";
import { test } from "node:test";

import { runtimeText } from "@/features/project-agents/ui/projectAgentsCopy";

test("runtimeText discloses a missing runtime rather than omitting it", () => {
  assert.equal(
    runtimeText({ effectiveRuntime: null, model: null }),
    "runtime not set",
  );
});

test("runtimeText uses the dashboard's runtime display name", () => {
  assert.equal(
    runtimeText({ effectiveRuntime: "codex", model: null }),
    "Codex",
  );
  assert.equal(
    runtimeText({ effectiveRuntime: "claude", model: null }),
    "Claude",
  );
});

test("runtimeText appends the model when the record names one", () => {
  assert.equal(
    runtimeText({ effectiveRuntime: "codex", model: "gpt-5.6-terra" }),
    "Codex · gpt-5.6-terra",
  );
});

test("runtimeText ignores a model with no runtime — never invents one", () => {
  assert.equal(
    runtimeText({ effectiveRuntime: null, model: "opus" }),
    "runtime not set",
  );
});

// --- Ledger 139: an inherited runtime is not "not set" ---------------------

test("runtimeText shows an inherited runtime, not 'runtime not set' (ledger 135(d), 139)", () => {
  // The raw per-instance pin is null — Kiln inherits its harness from its
  // persona — but the effective runtime the agent actually runs on is
  // Codex, and that is what the card must show.
  assert.equal(
    runtimeText({ effectiveRuntime: "codex", model: null }),
    "Codex",
  );
});
