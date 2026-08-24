import assert from "node:assert/strict";
import test from "node:test";
import React from "react";
import { renderToStaticMarkup } from "react-dom/server";

import { NewCodingSessionProviderPicker } from "./NewCodingSessionScreen.tsx";

/** The shape the screen builds from a provider catalog entry. */
function target(allowedModels) {
  return {
    selectionKey: "local:codex-primary",
    isLocalProvider: true,
    availability: { state: "ready", label: "Codex", hint: null },
    provider: {
      runtime: "codex",
      providerInstanceRef: "codex-primary",
      providerAuthorityPubkey: "a".repeat(64),
      defaultModel: allowedModels[0],
      allowedModels,
    },
  };
}

const CODEX = [
  "gpt-5.6-terra",
  "gpt-5.3-codex-spark",
  "gpt-5.3-codex-spark[high]",
  "gpt-5.3-codex-spark[low]",
  "gpt-5.6-luna[high]",
  "gpt-5.6-luna[max]",
];

function render(props) {
  return renderToStaticMarkup(
    React.createElement(NewCodingSessionProviderPicker, {
      disabled: false,
      onModelChange() {},
      onTargetChange() {},
      targets: [target(CODEX)],
      selectedTarget: target(CODEX),
      ...props,
    }),
  );
}

function optionValues(markup, testid) {
  const select = markup.match(
    new RegExp(`<select[^>]*data-testid="${testid}"[^>]*>(.*?)</select>`, "s"),
  );
  if (!select) return null;
  return [...select[1].matchAll(/value="([^"]*)"/g)].map((match) => match[1]);
}

// Observed live 2026-08-24: discovery turned Codex's four models into a
// thirty-row dropdown, because the adapter encodes reasoning effort in the id.
test("the model dropdown lists models, not model×effort combinations", () => {
  const markup = render({ model: "gpt-5.6-terra" });
  assert.deepEqual(optionValues(markup, "new-coding-session-model"), [
    "gpt-5.6-terra",
    "gpt-5.3-codex-spark",
    "gpt-5.6-luna",
  ]);
});

test("a model with no levels shows no thinking control at all", () => {
  const markup = render({ model: "gpt-5.6-terra" });
  assert.equal(optionValues(markup, "new-coding-session-thinking"), null);
});

test("a model with levels gets its own control, defaulting where the adapter has a bare id", () => {
  const withBare = render({ model: "gpt-5.3-codex-spark" });
  assert.deepEqual(optionValues(withBare, "new-coding-session-thinking"), [
    "",
    "high",
    "low",
  ]);

  // luna is offered only at named levels, so "adapter default" would name a
  // model id the provider never published.
  const levelOnly = render({ model: "gpt-5.6-luna[high]" });
  assert.deepEqual(optionValues(levelOnly, "new-coding-session-thinking"), [
    "high",
    "max",
  ]);
});

test("the selected level is the one shown, not folded away", () => {
  const markup = render({ model: "gpt-5.6-luna[max]" });
  assert.match(
    markup,
    /data-testid="new-coding-session-thinking"[^>]*>(?:(?!<\/select>).)*value="max" selected/s,
  );
});

test("claude's list keeps its context variant intact and shows no thinking control", () => {
  const claude = ["default", "opus[1m]", "sonnet", "haiku"];
  const markup = render({
    model: "opus[1m]",
    targets: [target(claude)],
    selectedTarget: target(claude),
  });
  assert.deepEqual(
    optionValues(markup, "new-coding-session-model"),
    claude,
    "opus[1m] is a context window, not a thinking level",
  );
  assert.equal(optionValues(markup, "new-coding-session-thinking"), null);
});
