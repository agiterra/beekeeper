import assert from "node:assert/strict";
import test from "node:test";

import {
  codingSessionContextLabel,
  codingSessionEffortLabel,
  codingSessionModelDisplayName,
  codingSessionModelTitle,
  codingSessionTraitsSummary,
} from "./codingSessionModelDisplay.ts";

test("ids become names without a hand-written table", () => {
  assert.equal(
    codingSessionModelDisplayName("claude-fable-5"),
    "Claude Fable 5",
  );
  assert.equal(codingSessionModelDisplayName("gpt-5.6-terra"), "GPT-5.6 Terra");
  assert.equal(codingSessionModelDisplayName("gpt-5.4-mini"), "GPT-5.4 mini");
  assert.equal(codingSessionModelDisplayName("sonnet"), "Sonnet");
  // Brackets never reach the name: they are other controls' business.
  assert.equal(codingSessionModelDisplayName("opus[1m]"), "Opus");
});

test("a hyphen-spelled version reads as the vendor writes it", () => {
  assert.equal(
    codingSessionModelDisplayName("claude-opus-5-5"),
    "Claude Opus 5.5",
  );
  assert.equal(
    codingSessionModelDisplayName("claude-haiku-4-5"),
    "Claude Haiku 4.5",
  );
  // A date stamp is no version part: it keeps its space.
  assert.equal(
    codingSessionModelDisplayName("claude-sonnet-4-5-20250929"),
    "Claude Sonnet 4.5 20250929",
  );
  assert.equal(codingSessionModelDisplayName("gpt-5-mini"), "GPT-5 mini");
});

test("a model nobody here has heard of still gets a readable name", () => {
  // The transform is total on purpose — a lookup table would have to guess.
  assert.equal(
    codingSessionModelDisplayName("wombat-9000-turbo"),
    "Wombat 9000 Turbo",
  );
  assert.equal(codingSessionModelDisplayName("x"), "X");
});

// One fact, one sentence. `useNewCodingSessionCreate.ts` discloses the same
// state as "Runtime default (not named on the record)"
// (`CODING_SESSION_CREATE_UNNAMED_MODEL_LABEL`, pinned by
// `useNewCodingSessionCreate.test.mjs`), and the model picker's row and closed
// trigger read from here — so "Adapter default" made the One-session dialog
// say two different things about the same unnamed model.
test("the adapter's alias says what it is rather than posing as a model", () => {
  assert.equal(
    codingSessionModelDisplayName("default"),
    "Runtime default (not named on the record)",
  );
  assert.equal(
    codingSessionModelDisplayName(""),
    "Runtime default (not named on the record)",
  );
});

test("traits summarise the way people write them, or not at all", () => {
  assert.equal(codingSessionContextLabel("1m"), "1M");
  assert.equal(
    codingSessionTraitsSummary({ thinking: "high", context: "1m" }),
    "High · 1M",
  );
  assert.equal(
    codingSessionTraitsSummary({ thinking: null, context: "200k" }),
    "200K",
  );
  assert.equal(
    codingSessionTraitsSummary({ thinking: null, context: null }),
    null,
    "a model with neither dimension must be able to hide the control",
  );
});

// "Claude Fable 5 1" was the derived name for `claude-fable-5-1`; the runtime
// calls it "Fable 5.1", and when its catalog row says so that is the title.
// (The derived fallback now reads "Claude Fable 5.1": short version parts
// join with a dot.)
test("the runtime's own name wins, and the derived name is only the fallback", () => {
  assert.equal(
    codingSessionModelTitle("claude-fable-5-1", "Fable 5.1"),
    "Fable 5.1",
  );
  assert.equal(
    codingSessionModelTitle("default", "Default (recommended)"),
    "Default (recommended)",
  );
  assert.equal(
    codingSessionModelTitle("claude-fable-5-1", null),
    "Claude Fable 5.1",
  );
  assert.equal(codingSessionModelTitle("gpt-5.6-terra", "  "), "GPT-5.6 Terra");
});

test("efforts and fast mode summarise the way people write them", () => {
  assert.equal(codingSessionEffortLabel("xhigh"), "Extra high");
  assert.equal(codingSessionEffortLabel("default"), "Default");
  assert.equal(codingSessionEffortLabel("ultra"), "Ultra");
  assert.equal(
    codingSessionTraitsSummary({ thinking: "max", context: null, fast: true }),
    "Max · Fast",
  );
});
