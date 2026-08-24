import assert from "node:assert/strict";
import test from "node:test";

import {
  CODING_SESSION_MODEL_FAVORITES_RAIL,
  codingSessionModelFavoriteKey,
  codingSessionModelPickerInitialRail,
  codingSessionModelPickerRows,
  codingSessionProviderBaseModels,
  scoreCodingSessionModelRow,
} from "./codingSessionModelPickerModel.ts";

const CLAUDE = {
  selectionKey: "local:claude-primary",
  runtime: "claude",
  label: "Claude Code · Primary",
  models: ["default", "opus[1m]", "sonnet", "haiku"],
  ready: true,
  unavailableNote: null,
};
const CODEX = {
  selectionKey: "local:codex-primary",
  runtime: "codex",
  label: "Codex · Primary",
  models: ["gpt-5.6-terra", "gpt-5.6-luna", "gpt-5.3-codex-spark"],
  ready: true,
  unavailableNote: null,
};
const PROVIDERS = [CLAUDE, CODEX];
const NO_FAVORITES = new Set();

function rows(overrides = {}) {
  return codingSessionModelPickerRows({
    providers: PROVIDERS,
    favorites: NO_FAVORITES,
    rail: CODEX.selectionKey,
    query: "",
    ...overrides,
  });
}

test("a provider rail shows that provider's models only", () => {
  assert.deepEqual(
    rows().map((row) => row.model),
    CODEX.models,
  );
  assert.deepEqual(
    rows({ rail: CLAUDE.selectionKey }).map((row) => row.model),
    CLAUDE.models,
  );
});

test("the favourites rail crosses providers and carries stable shortcuts", () => {
  const favorites = new Set([
    codingSessionModelFavoriteKey(CLAUDE.selectionKey, "sonnet"),
    codingSessionModelFavoriteKey(CODEX.selectionKey, "gpt-5.6-terra"),
  ]);
  const favorited = codingSessionModelPickerRows({
    providers: PROVIDERS,
    favorites,
    rail: CODING_SESSION_MODEL_FAVORITES_RAIL,
    query: "",
  });
  assert.deepEqual(
    favorited.map((row) => `${row.runtime}:${row.model}:${row.shortcut}`),
    ["claude:sonnet:1", "codex:gpt-5.6-terra:2"],
  );

  // The same model keeps its number inside its own provider rail: ⌘2 means
  // one thing, wherever it is read.
  const inCodexRail = codingSessionModelPickerRows({
    providers: PROVIDERS,
    favorites,
    rail: CODEX.selectionKey,
    query: "",
  });
  assert.equal(
    inCodexRail.find((row) => row.model === "gpt-5.6-terra")?.shortcut,
    2,
  );
  assert.equal(
    inCodexRail.find((row) => row.model === "gpt-5.6-luna")?.shortcut,
    null,
  );
});

test("a favourite is provider-scoped, never a bare model name", () => {
  const favorites = new Set([
    codingSessionModelFavoriteKey(CODEX.selectionKey, "gpt-5.6-terra"),
  ]);
  const twin = {
    ...CODEX,
    selectionKey: "catalog:codex-elsewhere",
    label: "Codex · Another machine",
  };
  const favorited = codingSessionModelPickerRows({
    providers: [CODEX, twin],
    favorites,
    rail: CODING_SESSION_MODEL_FAVORITES_RAIL,
    query: "",
  });
  assert.deepEqual(
    favorited.map((row) => row.selectionKey),
    [CODEX.selectionKey],
    "pinning one machine's Codex must not pin another's",
  );
});

test("search matches model, provider label, and runtime, and every token must hit", () => {
  const found = rows({ rail: CODEX.selectionKey, query: "spark" });
  assert.deepEqual(
    found.map((row) => row.model),
    ["gpt-5.3-codex-spark"],
  );
  // Separators are not significant: "gpt 5.6" finds the dashed ids.
  assert.deepEqual(
    rows({ query: "gpt 5.6" }).map((row) => row.model),
    ["gpt-5.6-terra", "gpt-5.6-luna"],
  );
  // A token that matches nothing removes the row rather than widening.
  assert.deepEqual(rows({ query: "spark nonsense" }), []);
});

test("a prefix outranks a mid-string hit, and a favourite outranks both", () => {
  const plain = {
    model: "gpt-5.6-terra",
    providerLabel: "Codex",
    runtime: "codex",
    favorite: false,
  };
  const mid = {
    model: "my-gpt-5.6",
    providerLabel: "Codex",
    runtime: "codex",
    favorite: false,
  };
  assert.ok(
    scoreCodingSessionModelRow(plain, "gpt") <
      scoreCodingSessionModelRow(mid, "gpt"),
  );
  assert.ok(
    scoreCodingSessionModelRow({ ...mid, favorite: true }, "gpt") <
      scoreCodingSessionModelRow(plain, "gpt"),
  );
  assert.equal(scoreCodingSessionModelRow(plain, "claude"), null);
});

test("the picker opens where the person already is", () => {
  assert.equal(
    codingSessionModelPickerInitialRail({
      providers: PROVIDERS,
      favorites: NO_FAVORITES,
      selectionKey: CODEX.selectionKey,
      model: "gpt-5.6-terra",
    }),
    CODEX.selectionKey,
  );
  // Favourites win only when the current selection is itself pinned…
  assert.equal(
    codingSessionModelPickerInitialRail({
      providers: PROVIDERS,
      favorites: new Set([
        codingSessionModelFavoriteKey(CODEX.selectionKey, "gpt-5.6-terra"),
      ]),
      selectionKey: CODEX.selectionKey,
      model: "gpt-5.6-terra[high]",
    }),
    CODING_SESSION_MODEL_FAVORITES_RAIL,
    "the thinking level is not part of the model's identity here",
  );
  // …never over an empty favourites rail.
  assert.equal(
    codingSessionModelPickerInitialRail({
      providers: PROVIDERS,
      favorites: NO_FAVORITES,
      selectionKey: null,
      model: null,
    }),
    CLAUDE.selectionKey,
  );
});

test("a provider's rows are its base models, levels folded away", () => {
  assert.deepEqual(
    codingSessionProviderBaseModels([
      "gpt-5.6-terra",
      "gpt-5.6-luna[high]",
      "gpt-5.6-luna[max]",
      "opus[1m]",
    ]),
    ["gpt-5.6-terra", "gpt-5.6-luna", "opus[1m]"],
  );
});

test("a provider with no catalog models still has a row that explains itself", () => {
  const signedOut = {
    selectionKey: "local:claude-primary",
    runtime: "claude",
    label: "Claude Code · Primary",
    models: [],
    ready: false,
    unavailableNote: "sign-in needed",
  };
  const found = codingSessionModelPickerRows({
    providers: [signedOut],
    favorites: NO_FAVORITES,
    rail: signedOut.selectionKey,
    query: "",
  });
  assert.equal(found.length, 1, "the provider must not vanish from its rail");
  assert.equal(found[0].model, "", "empty means: let the adapter choose");
  assert.equal(found[0].ready, false);
  assert.equal(found[0].unavailableNote, "sign-in needed");
});
