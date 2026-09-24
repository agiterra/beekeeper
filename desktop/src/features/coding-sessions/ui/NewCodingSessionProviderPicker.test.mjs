import assert from "node:assert/strict";
import test from "node:test";
import React from "react";
import { renderToStaticMarkup } from "react-dom/server";

import {
  NewCodingSessionModelDisclosure,
  NewCodingSessionProviderPicker,
  newCodingSessionEffectiveModel,
  newCodingSessionSeatModelBlocksCreate,
  resolveNewCodingSessionSeatModel,
} from "./NewCodingSessionProviderPicker.tsx";

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
// Provider and model are now one control; the rows it lists are covered by
// `codingSessionModelPickerModel.test.mjs`, and what this asserts is that the
// row states the current choice plainly and no flat model select survives.
test("provider and model are one control that names the current choice", () => {
  const markup = render({ model: "gpt-5.6-terra" });
  assert.match(markup, /data-testid="coding-session-model-picker"/);
  // The glyph says whose model it is, so the trigger says which — by name,
  // not by wire id.
  assert.match(markup, />GPT-5\.6 Terra</);
  assert.doesNotMatch(markup, /gpt-5\.6-terra/);
  assert.equal(optionValues(markup, "new-coding-session-model"), null);
  assert.doesNotMatch(markup, /data-testid="new-coding-session-provider"/);
});

test("a model with no levels and no windows shows no thinking control at all", () => {
  const markup = render({ model: "gpt-5.6-terra" });
  assert.doesNotMatch(markup, /data-testid="coding-session-traits-picker"/);
});

test("a model with levels gets its own control, summarising the current choice", () => {
  const withBare = render({ model: "gpt-5.3-codex-spark" });
  assert.match(withBare, /data-testid="coding-session-traits-picker"/);
  // No level chosen and the adapter publishes the bare id: that is a real
  // state, and it says so rather than naming a level nobody picked.
  assert.match(withBare, /Adapter default/);

  const levelled = render({ model: "gpt-5.6-luna[high]" });
  assert.match(levelled, /data-testid="coding-session-traits-picker"/);
  assert.match(levelled, />High</);
});

test("the selected level is the one shown, not folded away", () => {
  const markup = render({ model: "gpt-5.6-luna[max]" });
  assert.match(markup, />Max</);
  // And it is not smuggled back into the model name.
  assert.doesNotMatch(markup, /luna\[max\]/);
});

test("claude's context window is a trait, and never part of the model name", () => {
  const claude = ["default", "opus[1m]", "sonnet", "haiku"];
  const markup = render({
    model: "opus[1m]",
    targets: [target(claude)],
    selectedTarget: target(claude),
  });
  // The trigger names the model the way a person does…
  assert.match(markup, />Opus</);
  // …and the bracket is the traits control's business, printed as 1M.
  assert.doesNotMatch(markup, /opus\[1m\]/);
  assert.match(markup, /data-testid="coding-session-traits-picker"/);
  assert.match(markup, />1M</);
});

// Asked live, 2026-08-24: "how is the permission setting being done in Buzz?
// Are we even handling that?" It is not: every `session/request_permission` is
// answered `allow_once` in the ACP read loop (§2 item 13). The row says so
// rather than offering modes nothing enforces.
test("the access row states full access instead of offering a choice", () => {
  const markup = render({ model: "gpt-5.6-terra" });
  assert.match(markup, /data-testid="coding-session-access-notice"/);
  assert.match(markup, /Full access/);
  // Not a control: no select, no options, nothing to change.
  assert.equal(optionValues(markup, "coding-session-access-notice"), null);
});

// The "Runs the runtime's default model — the record will not name it."
// line left the dialog on 2026-09-10 (Andy). The wire is unchanged; the
// sentence is simply no longer printed under the picker.
test("a create that will carry `default` prints no sentence under the picker", () => {
  const html = renderToStaticMarkup(
    React.createElement(NewCodingSessionModelDisclosure, {
      model: "default",
      catalog: { defaultModel: "default", allowedModels: ["default", "opus"] },
    }),
  );
  assert.equal(html, "");
});

test("a runtime that is not installed gets no hint row — its option already says so", () => {
  // The sign-in case keeps its hint and Connect row; it needs a query client
  // and is exercised on the founded page (CodingSessionFoundedSetupCard).
  const goose = {
    ...target(["default"]),
    selectionKey: "local:goose-primary",
    availability: {
      state: "missing",
      label: "Goose",
      hint: "Goose is not installed on this computer.",
    },
    provider: {
      ...target(["default"]).provider,
      runtime: "goose",
      providerInstanceRef: "goose-primary",
    },
  };
  const html = render({ targets: [target(CODEX), goose] });
  assert.doesNotMatch(html, /is not installed on this computer/);
  // The option's own "not installed" note lives inside the picker's popover,
  // which static markup does not open; `CodingSessionModelPicker`'s tests
  // cover it.
});

test("a create that names a model says nothing extra", () => {
  const html = renderToStaticMarkup(
    React.createElement(NewCodingSessionModelDisclosure, {
      model: "opus[1m]",
      catalog: {
        defaultModel: "default",
        allowedModels: ["default", "opus[1m]"],
      },
    }),
  );
  assert.equal(html, "");
});

// Seating an identity used to change nothing about the model: the picker kept
// the adapter default (`default` on claude-primary), so a record that named
// `claude-fable-5[1m]` produced a create that named nothing.
test("seating an identity preselects the model its record names", () => {
  assert.deepEqual(
    resolveNewCodingSessionSeatModel({
      agentModel: "opus[1m]",
      allowedModels: ["default", "opus[1m]", "sonnet"],
      providerInstanceRef: "claude-primary",
      selectionExplicit: false,
    }),
    { model: "opus[1m]", note: null, mustPick: false, substitutedFor: null },
  );
});

test("a model the person picked outranks the identity's record", () => {
  assert.deepEqual(
    resolveNewCodingSessionSeatModel({
      agentModel: "opus[1m]",
      allowedModels: ["default", "opus[1m]", "sonnet"],
      providerInstanceRef: "claude-primary",
      selectionExplicit: true,
    }),
    { model: null, note: null, mustPick: false, substitutedFor: null },
  );
});

// Brian's ruling, 2026-08-29: falling silently to the runtime default here was
// the create half of the fake match. With no same-family id and no default to
// disclose, nothing is preselected, the record's id is named, and the create
// waits for a real pick.
test("an identity whose model this runtime cannot run blocks the create and says why", () => {
  const resolved = resolveNewCodingSessionSeatModel({
    agentModel: "gpt-5.6-terra",
    allowedModels: ["default", "opus[1m]"],
    providerInstanceRef: "claude-primary",
    selectionExplicit: false,
  });
  assert.equal(resolved.model, null);
  assert.equal(resolved.mustPick, true);
  assert.equal(
    resolved.note,
    "This identity's record names gpt-5.6-terra, which claude-primary does " +
      "not offer. Pick a model; the record keeps gpt-5.6-terra until you " +
      "change it on the Agents screen.",
  );
});

test("an identity with no model on its record asks for nothing", () => {
  assert.deepEqual(
    resolveNewCodingSessionSeatModel({
      agentModel: null,
      allowedModels: ["default", "opus[1m]"],
      providerInstanceRef: "claude-primary",
      selectionExplicit: false,
    }),
    { model: null, note: null, mustPick: false, substitutedFor: null },
  );
});

// The dialog's Create button and the id it would write both hang off this, so
// it is asserted where it can be read rather than left implicit in JSX.
test("a blocked seat model leaves the create with no model to write", () => {
  const blocked = resolveNewCodingSessionSeatModel({
    agentModel: "gpt-5.6-terra",
    allowedModels: ["default", "opus[1m]"],
    providerInstanceRef: "claude-primary",
    selectionExplicit: false,
  });
  assert.equal(
    newCodingSessionEffectiveModel({
      seatModel: blocked,
      providerModel: "opus[1m]",
    }),
    null,
  );
  assert.equal(newCodingSessionSeatModelBlocksCreate(blocked), true);
  // One hand-picked model later, the same seat is free to create.
  const picked = resolveNewCodingSessionSeatModel({
    agentModel: "gpt-5.6-terra",
    allowedModels: ["default", "opus[1m]"],
    providerInstanceRef: "claude-primary",
    selectionExplicit: true,
  });
  assert.equal(
    newCodingSessionEffectiveModel({
      seatModel: picked,
      providerModel: "opus[1m]",
    }),
    "opus[1m]",
  );
  assert.equal(newCodingSessionSeatModelBlocksCreate(picked), false);
});

test("a record naming a model this runtime lacks is said, not swallowed", () => {
  const html = renderToStaticMarkup(
    React.createElement(NewCodingSessionModelDisclosure, {
      model: "sonnet",
      catalog: { defaultModel: "sonnet", allowedModels: ["sonnet"] },
      note: "This agent's record names gpt-5.6-terra, which the selected provider does not offer.",
    }),
  );
  assert.match(html, /data-testid="new-coding-session-seat-model-note"/);
  assert.match(html, /gpt-5\.6-terra/);
  assert.doesNotMatch(html, /new-coding-session-model-disclosure/);
});

// Ledger 255(e): the stand-in is preselected, so the create has a model to
// write and Start is not held; the notice names both ids.
test("a record naming opus[1m] on a runtime offering opus creates on opus, disclosed", () => {
  const seat = resolveNewCodingSessionSeatModel({
    agentModel: "opus[1m]",
    allowedModels: ["default", "opus", "sonnet"],
    defaultModel: "default",
    providerInstanceRef: "claude-primary",
    selectionExplicit: false,
  });
  assert.equal(
    newCodingSessionEffectiveModel({
      seatModel: seat,
      providerModel: "default",
    }),
    "opus",
  );
  assert.equal(newCodingSessionSeatModelBlocksCreate(seat), false);
  const html = renderToStaticMarkup(
    React.createElement(NewCodingSessionModelDisclosure, { note: seat.note }),
  );
  assert.match(html, /names opus\[1m\], which claude-primary does not offer/);
  assert.match(html, /so opus, the nearest model it offers, is preselected/);
});

test("the record-update action names both ids and writes only on click", () => {
  let clicks = 0;
  const recordUpdate = {
    identityLabel: "Kiln",
    recordedModel: "opus[1m]",
    nextModel: "opus",
    state: "idle",
    error: null,
    onUpdate: () => {
      clicks += 1;
    },
  };
  const html = renderToStaticMarkup(
    React.createElement(NewCodingSessionModelDisclosure, {
      note: "note",
      recordUpdate,
    }),
  );
  assert.match(
    html,
    /data-testid="new-coding-session-seat-model-update-record"/,
  );
  assert.match(html, /Update Kiln&#x27;s record from opus\[1m\] to opus/);
  // Rendering is not a write.
  assert.equal(clicks, 0);
  const failed = renderToStaticMarkup(
    React.createElement(NewCodingSessionModelDisclosure, {
      note: null,
      recordUpdate: { ...recordUpdate, state: "error", error: "disk full" },
    }),
  );
  assert.match(failed, /The record still names opus\[1m\]: disk full/);
  assert.equal(
    renderToStaticMarkup(
      React.createElement(NewCodingSessionModelDisclosure, {
        note: null,
        recordUpdate: null,
      }),
    ),
    "",
  );
});
