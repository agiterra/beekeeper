import assert from "node:assert/strict";
import { test } from "node:test";

import { projectSessionObservationLabel } from "./projectSessionObservation.ts";

test("the project shelf reports metadata as history, never current liveness", () => {
  assert.equal(
    projectSessionObservationLabel({ kind: "working", label: "Working" }),
    "Reported working",
  );
  assert.equal(
    projectSessionObservationLabel({ kind: "idle", label: "Idle" }),
    "Reported idle",
  );
  assert.equal(
    projectSessionObservationLabel({ kind: "unknown", label: "Disconnected" }),
    "Reported disconnected",
  );
});
