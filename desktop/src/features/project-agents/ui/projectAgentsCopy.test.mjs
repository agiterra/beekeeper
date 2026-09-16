import assert from "node:assert/strict";
import { test } from "node:test";

import { runtimeText } from "@/features/project-agents/ui/projectAgentsCopy";

test("runtimeText discloses a missing runtime rather than omitting it", () => {
  assert.equal(runtimeText({ runtime: null, model: null }), "runtime not set");
});

test("runtimeText uses the dashboard's runtime display name", () => {
  assert.equal(runtimeText({ runtime: "codex", model: null }), "Codex");
  assert.equal(runtimeText({ runtime: "claude", model: null }), "Claude");
});

test("runtimeText appends the model when the record names one", () => {
  assert.equal(
    runtimeText({ runtime: "codex", model: "gpt-5.6-terra" }),
    "Codex · gpt-5.6-terra",
  );
});

test("runtimeText ignores a model with no runtime — never invents one", () => {
  assert.equal(
    runtimeText({ runtime: null, model: "opus" }),
    "runtime not set",
  );
});
