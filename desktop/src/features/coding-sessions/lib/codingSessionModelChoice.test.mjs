import assert from "node:assert/strict";
import test from "node:test";

import {
  codingSessionModelChoices,
  joinCodingSessionModelId,
  resolveCodingSessionContext,
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

test("each bracket is decoded as the dimension it is, or not at all", () => {
  assert.deepEqual(splitCodingSessionModelId("gpt-5.4[high]"), {
    model: "gpt-5.4",
    thinking: "high",
    context: null,
  });
  // `opus[1m]` is a context window, not a thinking level — and printing the
  // bracket in the model name was the complaint that produced this split.
  assert.deepEqual(splitCodingSessionModelId("opus[1m]"), {
    model: "opus",
    thinking: null,
    context: "1m",
  });
  assert.deepEqual(splitCodingSessionModelId("opus[1m][high]"), {
    model: "opus",
    thinking: "high",
    context: "1m",
  });
  assert.deepEqual(splitCodingSessionModelId("sonnet"), {
    model: "sonnet",
    thinking: null,
    context: null,
  });
  // An unknown bracket is part of the id: inventing a dimension the adapter
  // never offered is worse than a long name.
  assert.deepEqual(splitCodingSessionModelId("weird[preview]"), {
    model: "weird[preview]",
    thinking: null,
    context: null,
  });
  assert.deepEqual(splitCodingSessionModelId("[high]"), {
    model: "[high]",
    thinking: null,
    context: null,
  });
});

test("the split round-trips to the id the adapter published", () => {
  for (const id of [
    ...CODEX_MODELS,
    "opus[1m]",
    "opus[1m][high]",
    "sonnet",
    "weird[preview]",
  ]) {
    const { model, thinking, context } = splitCodingSessionModelId(id);
    assert.equal(joinCodingSessionModelId(model, thinking, context), id);
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

test("claude's list folds its context window out and sinks the alias", () => {
  const choices = codingSessionModelChoices([
    "default",
    "opus[1m]",
    "sonnet",
    "haiku",
  ]);
  assert.deepEqual(
    choices.models,
    ["opus", "sonnet", "haiku", "default"],
    "`default` is an alias, not a model, and must not sort among them",
  );
  assert.deepEqual(choices.contextByModel.get("opus"), ["1m"]);
  assert.deepEqual(choices.thinkingByModel.get("opus"), []);
  assert.ok(
    !choices.bareModels.has("opus"),
    "opus is published only at 1m, so there is no bare form to fall back to",
  );
  assert.ok(choices.bareModels.has("sonnet"));
});

test("a model's context window is carried across a model change like thinking is", () => {
  const choices = codingSessionModelChoices([
    "opus[1m]",
    "opus[200k]",
    "sonnet",
  ]);
  assert.equal(resolveCodingSessionContext(choices, "opus", "200k"), "200k");
  // sonnet has no windows and a bare form: nothing to carry.
  assert.equal(resolveCodingSessionContext(choices, "sonnet", "200k"), null);
  // opus has no bare form, so an unavailable preference falls to its first.
  assert.equal(resolveCodingSessionContext(choices, "opus", "512k"), "1m");
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
