/**
 * The provenance word a 44246 row prints, asserted as a value.
 *
 * Lane D's follow-up: `measured` was rendered raw, beside `observed`, with no
 * hedge — so a bench row a seat signed for itself read exactly like a row a
 * mechanism watched. Nothing on the wire checks who signed a `measured` row
 * (`codingSessionObservationWire.ts:28-38`), and the fold publishes no per-row
 * provider answer for one, so the word here must disclose the absence of the
 * check rather than imply it passed.
 */
import assert from "node:assert/strict";
import { test } from "node:test";

import {
  GATE_SOURCE_DECLARED,
  GATE_SOURCE_MEASURED_SELF_REPORTED,
  GATE_SOURCE_MEASURED_UNVERIFIED,
  GATE_SOURCE_OBSERVED,
  GATE_SOURCE_TITLES,
  gateSourceLabel,
  gateSourceTitle,
} from "@/features/coding-sessions/lib/codingSessionObservationView";

test("observed and declared keep their own words", () => {
  assert.equal(gateSourceLabel("observed"), "observed");
  assert.equal(gateSourceLabel("declared"), "declared");
  assert.equal(GATE_SOURCE_OBSERVED, "observed");
  assert.equal(GATE_SOURCE_DECLARED, "declared");
});

test("measured is never bare, in any of the three states", () => {
  for (const answer of [undefined, null, true, false]) {
    assert.notEqual(gateSourceLabel("measured", answer), "measured");
  }
});

test("no per-row provider answer reads unverified, not self-reported", () => {
  assert.equal(gateSourceLabel("measured"), "measured (unverified)");
  assert.equal(gateSourceLabel("measured", null), "measured (unverified)");
  assert.equal(GATE_SOURCE_MEASURED_UNVERIFIED, "measured (unverified)");
  assert.match(
    gateSourceTitle("measured"),
    /Nothing here checks who signed it/,
  );
});

test("a signer known not to be the session's provider reads self-reported", () => {
  assert.equal(gateSourceLabel("measured", false), "measured (self-reported)");
  assert.equal(GATE_SOURCE_MEASURED_SELF_REPORTED, "measured (self-reported)");
  assert.match(
    gateSourceTitle("measured", false),
    /not one of this session's providers/,
  );
});

test("a signer that IS the session's provider still does not read bare", () => {
  // `true` is not a verification of the bench, only of the signer, so the
  // word stays hedged rather than promoting itself to a plain "measured".
  assert.equal(gateSourceLabel("measured", true), "measured (unverified)");
});

test("each word has its own tooltip, and none is reused", () => {
  const titles = [
    gateSourceTitle("observed"),
    gateSourceTitle("declared"),
    gateSourceTitle("measured", false),
    gateSourceTitle("measured", null),
  ];
  assert.equal(new Set(titles).size, 4);
  assert.equal(GATE_SOURCE_TITLES.declared, titles[1]);
});
