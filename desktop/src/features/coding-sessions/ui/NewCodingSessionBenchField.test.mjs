/**
 * The bench default (ledger 186, finding 178(h)): "use the team" means the
 * team. The bench was empty until a person ticked seven agents by hand, so a
 * launch that skipped the field gave the lead nobody to hire.
 */
import assert from "node:assert/strict";
import test from "node:test";

import {
  newCodingSessionBenchSelection,
  newCodingSessionBenchToggle,
} from "./NewCodingSessionBenchField.tsx";

const OPTIONS = [
  { value: "aa".repeat(32), label: "Codey", detail: "builder", group: "Hive" },
  {
    value: "bb".repeat(32),
    label: "Grokker",
    detail: "verifier",
    group: "Hive",
  },
  { value: "cc".repeat(32), label: "Scribe", detail: "scribe", group: "Hive" },
];

test("an untouched bench is every project agent it shows", () => {
  assert.deepEqual(
    newCodingSessionBenchSelection({ ticked: null, options: OPTIONS }),
    OPTIONS.map((option) => option.value),
  );
  // Nothing to bench stays nothing to bench — the default never invents a row.
  assert.deepEqual(
    newCodingSessionBenchSelection({ ticked: null, options: [] }),
    [],
  );
});

test("the first untick drops exactly that agent and keeps the rest", () => {
  const next = newCodingSessionBenchToggle({
    ticked: null,
    options: OPTIONS,
    value: OPTIONS[1].value,
    selected: false,
  });
  assert.deepEqual(next, [OPTIONS[0].value, OPTIONS[2].value]);
});

test("the control stays fully unticked-able: an empty bench survives", () => {
  let ticked = null;
  for (const option of OPTIONS) {
    ticked = newCodingSessionBenchToggle({
      ticked,
      options: OPTIONS,
      value: option.value,
      selected: false,
    });
  }
  assert.deepEqual(ticked, []);
  // And it stays empty — an empty selection is a choice, not "untouched".
  assert.deepEqual(
    newCodingSessionBenchSelection({ ticked, options: OPTIONS }),
    [],
  );
});

test("re-ticking is idempotent and adds no duplicate", () => {
  const once = newCodingSessionBenchToggle({
    ticked: [],
    options: OPTIONS,
    value: OPTIONS[0].value,
    selected: true,
  });
  const twice = newCodingSessionBenchToggle({
    ticked: once,
    options: OPTIONS,
    value: OPTIONS[0].value,
    selected: true,
  });
  assert.deepEqual(twice, [OPTIONS[0].value]);
});
