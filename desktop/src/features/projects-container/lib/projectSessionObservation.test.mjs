import assert from "node:assert/strict";
import { test } from "node:test";

import {
  projectSessionObservationLabel,
  projectSessionObservationTitle,
} from "./projectSessionObservation.ts";

test("the shelf shows the bare status word", () => {
  assert.equal(
    projectSessionObservationLabel({ kind: "working", label: "Working" }),
    "Working",
  );
  assert.equal(
    projectSessionObservationLabel({ kind: "idle", label: "Idle" }),
    "Idle",
  );
  assert.equal(
    projectSessionObservationLabel({ kind: "unknown", label: "Disconnected" }),
    "Disconnected",
  );
});

test("the provenance the word drops is kept on the hover, not deleted", () => {
  const title = projectSessionObservationTitle({
    kind: "working",
    label: "Working",
  });
  assert.match(title, /last reported/);
  assert.match(
    title,
    /not a live lease/,
    "the shelf reads metadata; it must never be read as proof of liveness",
  );
});
