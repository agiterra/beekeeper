import assert from "node:assert/strict";
import test from "node:test";

import {
  getCodingSessionComposerState,
  shouldSubmitCodingSessionComposerKey,
} from "./codingSessionComposerModel.ts";

test("member gate is fail-closed while the composer remains reachable", () => {
  assert.deepEqual(
    getCodingSessionComposerState({
      canSteer: false,
      isMember: false,
      isWorking: false,
      text: "Steer",
    }),
    {
      canSend: false,
      primaryLabel: "Send",
      sendLabel: "Send",
      showAuthorityFailure: true,
      showStopAction: false,
    },
  );
});

test("a running turn offers Steer beside the interrupt, and Enter steers", () => {
  assert.equal(
    getCodingSessionComposerState({
      canSteer: true,
      isMember: true,
      isWorking: true,
      text: "Steer",
    }).sendLabel,
    "Steer",
  );
  assert.equal(
    getCodingSessionComposerState({
      canSteer: true,
      isMember: true,
      isWorking: true,
      text: "Steer",
    }).showStopAction,
    true,
  );
  assert.equal(
    shouldSubmitCodingSessionComposerKey({ key: "Enter", shiftKey: false }),
    true,
  );
  assert.equal(
    shouldSubmitCodingSessionComposerKey({ key: "Enter", shiftKey: true }),
    false,
  );
});

// Asked live, 2026-08-24: "why can it no longer be resumed? Should there be a
// pause and a stop?" Part of that confusion was the composer itself — the
// turn-level control and the terminal one both read "Stop", side by side.
test("only the terminal control is called Stop", () => {
  const working = getCodingSessionComposerState({
    canSteer: true,
    isMember: true,
    isWorking: true,
    text: "go",
  });
  assert.equal(working.primaryLabel, "Interrupt");
  assert.notEqual(working.primaryLabel, "Stop");
  assert.equal(working.sendLabel, "Steer");
  assert.equal(working.showStopAction, true);
});

// The label is a promise about what the provider will do with the words. An
// execution whose runtime advertised no native steering cannot keep it: the
// turn reaches the provider now and runs at the next turn boundary.
test("a non-steering execution is not offered a steer it cannot get", () => {
  assert.equal(
    getCodingSessionComposerState({
      canSteer: false,
      isMember: true,
      isWorking: true,
      text: "go",
    }).sendLabel,
    "Send next",
  );
  assert.equal(
    getCodingSessionComposerState({
      canSteer: false,
      isMember: true,
      isWorking: false,
      text: "go",
    }).sendLabel,
    "Send",
  );
  assert.equal(
    getCodingSessionComposerState({
      canSteer: true,
      isMember: true,
      isWorking: true,
      text: "go",
    }).sendLabel,
    "Steer",
  );
});

test("a caller that says nothing about steering does not get a Steer button", () => {
  // Fail-safe, not fail-open. `canSteer` is this execution's own capability,
  // learned from its 44223 metadata; a call site that forgets to pass it used
  // to get "Steer" and a `deliver: "steer"` command against a provider that
  // cannot steer — a control that degrades one hundred per cent of the time,
  // which is exactly what the delivery classes exist to prevent.
  assert.equal(
    getCodingSessionComposerState({
      isMember: true,
      isWorking: true,
      text: "Steer",
    }).sendLabel,
    "Send next",
  );
});

test("send is held closed while an attachment is unsettled", () => {
  const base = {
    canSteer: false,
    isMember: true,
    isWorking: false,
    text: "look at this",
  };
  assert.equal(getCodingSessionComposerState(base).canSend, true);
  // Mid-upload: publishing here would sign a turn whose attachment list is
  // short of what the composer is showing.
  assert.equal(
    getCodingSessionComposerState({ ...base, hasUnsettledAttachments: true })
      .canSend,
    false,
  );
  // The label is unaffected — only the gate closes, so the button does not
  // start claiming a different action while an upload finishes.
  assert.equal(
    getCodingSessionComposerState({ ...base, hasUnsettledAttachments: true })
      .sendLabel,
    "Send",
  );
});
