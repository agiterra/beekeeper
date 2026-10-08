import assert from "node:assert/strict";
import test from "node:test";

import {
  CODING_SESSION_MODEL_SWITCH_EFFORT_TEXT,
  CODING_SESSION_MODEL_SWITCH_OPERATORS_ONLY_TEXT,
  CODING_SESSION_MODEL_SWITCH_UNSUPPORTED_TEXT,
  codingSessionModelSwitchNote,
  codingSessionModelSwitchRows,
} from "./codingSessionModelSwitchRows.ts";

const label = (selection) =>
  ({
    "sonnet[high]": "Sonnet 5 · High",
    sonnet: "Sonnet 5",
  })[selection] ?? selection;

test("pending says the switch waits for the next turn", () => {
  assert.deepEqual(
    codingSessionModelSwitchNote(
      { kind: "pending", requested: "sonnet[high]" },
      label,
    ),
    { tone: "muted", text: "Switching to Sonnet 5 · High at the next turn" },
  );
});

test("accepted without metadata does not name a running model", () => {
  const note = codingSessionModelSwitchNote(
    { kind: "accepted", requested: "sonnet[high]" },
    label,
  );
  assert.match(note.text, /waiting for the provider's record/);
  assert.doesNotMatch(note.text, /running/);
});

test("an applied match says nothing; a mismatch says asked · running", () => {
  assert.equal(
    codingSessionModelSwitchNote(
      {
        kind: "applied",
        requested: "sonnet[high]",
        running: "sonnet[high]",
        matches: true,
      },
      label,
    ),
    null,
  );
  assert.deepEqual(
    codingSessionModelSwitchNote(
      {
        kind: "applied",
        requested: "sonnet[high]",
        running: "sonnet",
        matches: false,
      },
      label,
    ),
    { tone: "warning", text: "Asked Sonnet 5 · High · running Sonnet 5" },
  );
});

test("each refusal code has its own sentence; unknown codes keep the provider's", () => {
  const refused = (code, outcome = "refused") =>
    codingSessionModelSwitchNote(
      {
        kind: "refused",
        requested: "sonnet[high]",
        outcome,
        code,
        message: "provider words",
      },
      label,
    ).text;
  assert.equal(
    refused("MODEL_SWITCH_UNSUPPORTED"),
    `Not switched: ${CODING_SESSION_MODEL_SWITCH_UNSUPPORTED_TEXT}`,
  );
  assert.match(refused("MODEL_NOT_OFFERED"), /does not offer Sonnet 5 · High/);
  assert.match(refused("MODEL_SWITCH_FAILED"), /kept its previous model/);
  assert.equal(
    refused("UNAUTHORIZED_OPERATOR"),
    "Not switched: provider words",
  );
  assert.match(refused("QUEUE_FULL", "dropped"), /dropped.*QUEUE_FULL/);
});

test("display-only and operators-only rows replace the old fixed copy", () => {
  const idle = { kind: "idle" };
  const unsupported = codingSessionModelSwitchRows({
    availability: "unsupported",
    state: idle,
    effectiveHasEffort: false,
    label,
  });
  assert.deepEqual(unsupported, [CODING_SESSION_MODEL_SWITCH_UNSUPPORTED_TEXT]);
  const viewer = codingSessionModelSwitchRows({
    availability: "not-controller",
    state: idle,
    effectiveHasEffort: false,
    label,
  });
  assert.deepEqual(viewer, [CODING_SESSION_MODEL_SWITCH_OPERATORS_ONLY_TEXT]);
  for (const rows of [unsupported, viewer]) {
    assert.ok(rows.every((row) => !/fixed/i.test(row)));
  }
});

test("effort is disclosed as an acknowledgement, and rows stay at three", () => {
  const rows = codingSessionModelSwitchRows({
    availability: "available",
    state: { kind: "pending", requested: "sonnet[high]" },
    effectiveHasEffort: true,
    label,
  });
  assert.equal(rows.length, 3);
  assert.equal(rows[1], "Switching to Sonnet 5 · High at the next turn");
  assert.equal(rows[2], CODING_SESSION_MODEL_SWITCH_EFFORT_TEXT);
});
