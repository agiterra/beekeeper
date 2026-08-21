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

test("running primary action is stop and editor key behavior steers", () => {
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
