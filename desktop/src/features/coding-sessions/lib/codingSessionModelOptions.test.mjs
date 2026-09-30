import assert from "node:assert/strict";
import test from "node:test";

import {
  codingSessionModelOptions,
  codingSessionProviderPickerModels,
  isCodingSessionModelSelectionOffered,
  joinCodingSessionModelSelection,
  resolveCodingSessionModelPick,
  splitCodingSessionModelSelection,
} from "./codingSessionModelOptions.ts";

const EFFORTS = ["default", "low", "medium", "high", "xhigh", "max"];

/** What the updated Claude adapter reports, as the catalog carries it. */
const CLAUDE_OFFER = {
  allowedModels: [
    "default",
    "claude-fable-5",
    "claude-fable-5-1",
    "claude-opus-5",
    "haiku",
    "opus",
    "opus[1m]",
    "sonnet",
  ],
  models: [
    {
      id: "default",
      name: "Default (recommended)",
      description: "Opus 5.5",
      efforts: EFFORTS,
      fastMode: true,
      rank: 0,
    },
    { id: "claude-fable-5", name: "Fable 5", efforts: EFFORTS, rank: 6 },
    { id: "claude-fable-5-1", name: "Fable 5.1", efforts: EFFORTS, rank: 2 },
    { id: "claude-opus-5", name: "Opus 5", efforts: EFFORTS, rank: 5 },
    { id: "haiku", name: "Haiku 4.5", rank: 4 },
    {
      id: "opus",
      name: "Opus 5.5",
      efforts: EFFORTS,
      fastMode: true,
      rank: 1,
    },
    { id: "sonnet", name: "Sonnet 5.5", efforts: EFFORTS, rank: 3 },
  ],
};

/** Codex: efforts with no `default`, published bracket variants, `ultra`. */
const CODEX_OFFER = {
  allowedModels: [
    "gpt-6.1-sol",
    "gpt-5.6-sol",
    "gpt-5.6-sol[high]",
    "gpt-5.6-sol[ultra]",
    "gpt-6-astra",
  ],
  models: [
    {
      id: "gpt-6.1-sol",
      name: "6.1 Sol",
      efforts: ["low", "medium", "high", "xhigh"],
      fastMode: true,
      rank: 0,
    },
    {
      id: "gpt-5.6-sol",
      name: "5.6 Sol",
      efforts: ["low", "medium", "high", "ultra"],
      rank: 2,
    },
    { id: "gpt-6-astra", name: "6 Astra", efforts: ["low", "high"], rank: 1 },
  ],
};

test("the list follows the runtime's own order and names", () => {
  const { models, details } = codingSessionProviderPickerModels(CLAUDE_OFFER);
  assert.deepEqual(models, [
    "default",
    "opus",
    "claude-fable-5-1",
    "sonnet",
    "haiku",
    "claude-opus-5",
    "claude-fable-5",
  ]);
  assert.equal(details.get("opus")?.name, "Opus 5.5");
  assert.equal(details.get("default")?.description, "Opus 5.5");
});

test("unranked models follow the ranked ones in today's order", () => {
  const { models } = codingSessionProviderPickerModels({
    allowedModels: ["default", "alpha", "beta", "gamma"],
    models: [{ id: "gamma", name: "Gamma", rank: 0 }],
  });
  // `default` still sinks among the unranked, exactly as before rows existed.
  assert.deepEqual(models, ["gamma", "alpha", "beta", "default"]);
});

test("a provider with no rows lists exactly what it listed before", () => {
  const { models, details } = codingSessionProviderPickerModels({
    allowedModels: [
      "default",
      "gpt-5.6-luna[high]",
      "gpt-5.6-terra",
      "opus[1m]",
    ],
  });
  assert.deepEqual(models, [
    "gpt-5.6-luna",
    "gpt-5.6-terra",
    "opus",
    "default",
  ]);
  assert.equal(details.size, 0);
});

test("bracketed effort variants fold into their base when the runtime reports efforts", () => {
  const { models } = codingSessionProviderPickerModels(CODEX_OFFER);
  // `gpt-5.6-sol[ultra]` is not an enumerated level, but the row names it.
  assert.deepEqual(models, ["gpt-6.1-sol", "gpt-6-astra", "gpt-5.6-sol"]);
});

test("selection strings split and join per the contract", () => {
  for (const [selection, split] of [
    ["opus", { model: "opus", context: null, effort: null, fast: false }],
    [
      "opus[high]",
      { model: "opus", context: null, effort: "high", fast: false },
    ],
    [
      "opus[high][fast]",
      { model: "opus", context: null, effort: "high", fast: true },
    ],
    ["opus[fast]", { model: "opus", context: null, effort: null, fast: true }],
    [
      "opus[1m][max]",
      { model: "opus", context: "1m", effort: "max", fast: false },
    ],
  ]) {
    assert.deepEqual(
      splitCodingSessionModelSelection(selection, CLAUDE_OFFER),
      split,
      selection,
    );
    assert.equal(joinCodingSessionModelSelection(split), selection);
  }
  // `default` is the runtime's own default: no token is sent for it.
  assert.equal(
    joinCodingSessionModelSelection({
      model: "sonnet",
      context: null,
      effort: "default",
      fast: false,
    }),
    "sonnet",
  );
  // Without a row offering fast mode, `[fast]` stays part of the id.
  assert.equal(
    splitCodingSessionModelSelection("gpt-x[fast]", { allowedModels: [] }).fast,
    false,
  );
});

test("the options control reads the selected model's row", () => {
  const opus = codingSessionModelOptions(CLAUDE_OFFER, "opus", null);
  assert.deepEqual(opus, {
    source: "runtime",
    levels: EFFORTS,
    defaultOption: "named",
    fastMode: true,
  });
  const sonnet = codingSessionModelOptions(CLAUDE_OFFER, "sonnet", null);
  assert.equal(sonnet.fastMode, false, "no row switch, no fast toggle");
  // Haiku reports no efforts: nothing to choose, and no invented levels.
  assert.deepEqual(codingSessionModelOptions(CLAUDE_OFFER, "haiku", null), {
    source: "variants",
    levels: [],
    defaultOption: "unnamed",
    fastMode: false,
  });
  // A window is a different id; `opus`'s row is not assumed to hold for it.
  assert.equal(
    codingSessionModelOptions(CLAUDE_OFFER, "opus", "1m").source,
    "variants",
  );
  // Codex names no default, and a published variant stays reachable.
  const sol = codingSessionModelOptions(CODEX_OFFER, "gpt-5.6-sol", null);
  assert.deepEqual(sol.levels, ["low", "medium", "high", "ultra"]);
  assert.equal(sol.defaultOption, "unnamed");
});

test("changing model keeps what the new model offers and resets the rest", () => {
  const previous = { context: null, effort: "max", fast: true };
  assert.equal(
    resolveCodingSessionModelPick({
      offer: CLAUDE_OFFER,
      model: "opus",
      previous,
    }),
    "opus[max][fast]",
  );
  // Sonnet has `max` but no fast-mode switch.
  assert.equal(
    resolveCodingSessionModelPick({
      offer: CLAUDE_OFFER,
      model: "sonnet",
      previous,
    }),
    "sonnet[max]",
  );
  // Haiku has neither.
  assert.equal(
    resolveCodingSessionModelPick({
      offer: CLAUDE_OFFER,
      model: "haiku",
      previous,
    }),
    "haiku",
  );
  // Codex 6.1 Sol has fast mode but no `max`.
  assert.equal(
    resolveCodingSessionModelPick({
      offer: CODEX_OFFER,
      model: "gpt-6.1-sol",
      previous,
    }),
    "gpt-6.1-sol[fast]",
  );
  // Providers without rows keep the variant-level rule.
  assert.equal(
    resolveCodingSessionModelPick({
      offer: { allowedModels: ["luna[high]", "luna[max]"] },
      model: "luna",
      previous,
    }),
    "luna[max]",
  );
});

test("a composed selection is offered only when its row offers every token", () => {
  for (const offered of [
    "opus",
    "opus[high]",
    "opus[high][fast]",
    "opus[fast]",
    "default[xhigh][fast]",
    "sonnet[max]",
    "opus[1m]",
  ]) {
    assert.equal(
      isCodingSessionModelSelectionOffered(CLAUDE_OFFER, offered),
      true,
      offered,
    );
  }
  for (const refused of [
    "sonnet[fast]", // no fast-mode switch on sonnet
    "haiku[high]", // haiku reports no efforts
    "opus[default]", // the default is sent as no token
    "opus[ultra]", // not one of opus's efforts
    "opus[1m][high]", // `opus[1m]` has no row of its own
    "mythos[high]", // not offered at all
    "opus[high][fast][fast]",
    "",
  ]) {
    assert.equal(
      isCodingSessionModelSelectionOffered(CLAUDE_OFFER, refused),
      false,
      refused,
    );
  }
  // Codex: row efforts, and published variants, both count.
  assert.equal(
    isCodingSessionModelSelectionOffered(CODEX_OFFER, "gpt-5.6-sol[ultra]"),
    true,
  );
  assert.equal(
    isCodingSessionModelSelectionOffered(
      CODEX_OFFER,
      "gpt-6.1-sol[xhigh][fast]",
    ),
    true,
  );
  assert.equal(
    isCodingSessionModelSelectionOffered(CODEX_OFFER, "gpt-6-astra[medium]"),
    false,
  );
});
