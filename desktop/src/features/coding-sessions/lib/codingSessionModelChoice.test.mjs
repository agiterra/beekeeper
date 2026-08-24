import assert from "node:assert/strict";
import test from "node:test";

import {
  codingSessionModelChoices,
  joinCodingSessionModelId,
  resolveCodingSessionThinking,
  splitCodingSessionModelId,
} from "./codingSessionModelChoice.ts";

// The exact list codex-acp offered on 2026-08-24, trimmed to the shapes that
// matter: a bare model, a family with five levels, and a level-only family.
const CODEX_MODELS = [
  "gpt-5.6-terra",
  "gpt-5.3-codex-spark",
  "gpt-5.3-codex-spark[high]",
  "gpt-5.3-codex-spark[low]",
  "gpt-5.3-codex-spark[medium]",
  "gpt-5.3-codex-spark[xhigh]",
  "gpt-5.6-luna[high]",
  "gpt-5.6-luna[max]",
];

test("a reasoning suffix splits and a context suffix does not", () => {
  assert.deepEqual(splitCodingSessionModelId("gpt-5.4[high]"), {
    model: "gpt-5.4",
    thinking: "high",
  });
  // claude-agent-acp's `opus[1m]` is a context window. Splitting it would
  // invent a thinking level the adapter never offered.
  assert.deepEqual(splitCodingSessionModelId("opus[1m]"), {
    model: "opus[1m]",
    thinking: null,
  });
  assert.deepEqual(splitCodingSessionModelId("sonnet"), {
    model: "sonnet",
    thinking: null,
  });
  assert.deepEqual(splitCodingSessionModelId("[high]"), {
    model: "[high]",
    thinking: null,
  });
});

test("the split round-trips to the id the adapter published", () => {
  for (const id of [...CODEX_MODELS, "opus[1m]", "sonnet"]) {
    const { model, thinking } = splitCodingSessionModelId(id);
    assert.equal(joinCodingSessionModelId(model, thinking), id);
  }
});

test("thirty rows fold into models plus their levels, in adapter order", () => {
  const choices = codingSessionModelChoices(CODEX_MODELS);
  assert.deepEqual(choices.models, [
    "gpt-5.6-terra",
    "gpt-5.3-codex-spark",
    "gpt-5.6-luna",
  ]);
  assert.deepEqual(choices.thinkingByModel.get("gpt-5.6-terra"), []);
  assert.deepEqual(choices.thinkingByModel.get("gpt-5.3-codex-spark"), [
    "high",
    "low",
    "medium",
    "xhigh",
  ]);
  assert.deepEqual(choices.thinkingByModel.get("gpt-5.6-luna"), [
    "high",
    "max",
  ]);
  assert.ok(choices.bareModels.has("gpt-5.3-codex-spark"));
  assert.ok(!choices.bareModels.has("gpt-5.6-luna"));
});

test("claude's list is untouched by the fold", () => {
  const choices = codingSessionModelChoices([
    "default",
    "opus[1m]",
    "sonnet",
    "haiku",
  ]);
  assert.deepEqual(choices.models, ["default", "opus[1m]", "sonnet", "haiku"]);
  for (const model of choices.models) {
    assert.deepEqual(choices.thinkingByModel.get(model), []);
  }
});

test("switching model keeps the level when it exists, and never invents one", () => {
  const choices = codingSessionModelChoices(CODEX_MODELS);
  assert.equal(
    resolveCodingSessionThinking(choices, "gpt-5.6-luna", "high"),
    "high",
  );
  // luna offers no bare id, so a level it does not have falls to its first.
  assert.equal(
    resolveCodingSessionThinking(choices, "gpt-5.6-luna", "medium"),
    "high",
  );
  // spark has a bare id: the honest fallback is the adapter's own default.
  assert.equal(
    resolveCodingSessionThinking(choices, "gpt-5.3-codex-spark", "max"),
    null,
  );
  assert.equal(
    resolveCodingSessionThinking(choices, "gpt-5.6-terra", "high"),
    null,
  );
});
