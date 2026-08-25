import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import test from "node:test";

import { describePendingCodingSessionTurn } from "./CodingSessionPendingTurns.tsx";

// Asked for 2026-08-25: remove the spinner and the waiting-on-provider copy.
// A normal in-flight turn is just the person's message; the session's own
// status announces work beginning.
test("an accepted turn says nothing about itself", () => {
  assert.equal(describePendingCodingSessionTurn("sending"), null);
  assert.equal(describePendingCodingSessionTurn("waiting"), null);
  const source = readFileSync(
    new URL("./CodingSessionPendingTurns.tsx", import.meta.url),
    "utf8",
  );
  assert.doesNotMatch(source, /LoaderCircle|animate-spin/);
  assert.doesNotMatch(source, /waiting for the provider/i);
});

test("silence stops where it would become a lie", () => {
  assert.equal(
    describePendingCodingSessionTurn("stalled"),
    "Not picked up yet",
  );
});

test("nothing claims the model is thinking", () => {
  for (const state of ["sending", "waiting", "stalled"]) {
    const caption = describePendingCodingSessionTurn(state);
    if (caption === null) continue;
    assert.doesNotMatch(caption, /thinking|working|generating/i, caption);
  }
});
