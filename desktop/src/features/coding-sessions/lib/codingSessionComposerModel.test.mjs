import assert from "node:assert/strict";
import test from "node:test";

import {
  getCodingSessionComposerState,
  shouldSubmitCodingSessionComposerKey,
} from "./codingSessionComposerModel.ts";

test("member gate is fail-closed while the composer remains reachable", () => {
  assert.deepEqual(
    getCodingSessionComposerState({
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
      isMember: true,
      isWorking: true,
      text: "Steer",
    }).sendLabel,
    "Steer",
  );
  assert.equal(
    getCodingSessionComposerState({
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
    isMember: true,
    isWorking: true,
    text: "go",
  });
  assert.equal(working.primaryLabel, "Interrupt");
  assert.notEqual(working.primaryLabel, "Stop");
  assert.equal(working.sendLabel, "Steer");
  assert.equal(working.showStopAction, true);
});
