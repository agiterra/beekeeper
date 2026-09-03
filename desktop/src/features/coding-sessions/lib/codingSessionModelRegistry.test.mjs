import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import test from "node:test";

import {
  codingSessionGraceDaysLeft,
  codingSessionOpinionTraits,
  codingSessionProvenanceClause,
  codingSessionScoreProvenance,
  parseModelRegistry,
} from "./codingSessionModelRegistry.ts";

const SHIPPED = readFileSync(
  fileURLToPath(
    new URL("../../../../../team/model-registry.yaml", import.meta.url),
  ),
  "utf8",
);

const MEASURED_BLOCK = `
    measured:
      role: verifier
      benchVersion: 2
      benchHash: "0123456789abcdef"
      measuredAt: "2026-09-02"
      measuredBy: "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
      runs:
        - "e1"
        - "e2"
        - "e3"
      traits:
        reasoning: { score: 4.6, n: 3, min: 4.4, max: 4.8 }
        judgment: { score: 4.8, n: 3, min: 4.7, max: 4.9 }
        verification: { score: 4.9, n: 3, min: 4.8, max: 5.0 }
`;

/** The shipped registry with a `measured:` block spliced onto one row. */
function withMeasured(block = MEASURED_BLOCK) {
  const anchor = "  - provider: claude-primary\n    model: opus[1m]\n";
  assert.ok(SHIPPED.includes(anchor), "the anchor row must exist");
  // Only the leading newline goes: the block's own indentation is what
  // makes it a member of the row.
  return SHIPPED.replace(anchor, `${anchor}${block.replace(/^\n/, "")}\n`);
}

test("the shipped registry parses unchanged and every row is legacy", () => {
  const result = parseModelRegistry(SHIPPED);
  assert.equal(result.ok, true, result.ok ? "" : result.why);
  assert.equal(result.registry.targets.length, 11);
  for (const target of result.registry.targets) {
    assert.equal(target.measured, undefined);
    assert.equal(codingSessionScoreProvenance(target), "legacy");
    assert.match(
      codingSessionProvenanceClause(target),
      /scores are operational priors, not measurements \(rating: operational_opinion, confidence low, brian 2026-08-30\) — this row is legacy/,
    );
    // Every one of the ten is still an opinion.
    assert.equal(codingSessionOpinionTraits(target).length, 10);
  }
});

test("a measured block is read, and it names the bench in the clause", () => {
  const result = parseModelRegistry(withMeasured());
  assert.equal(result.ok, true, result.ok ? "" : result.why);
  const row = result.registry.targets.find(
    (target) => target.model === "opus[1m]",
  );
  assert.ok(row, "the row must still be there");
  assert.equal(codingSessionScoreProvenance(row), "measured");
  assert.equal(row.measured.benchVersion, 2);
  assert.deepEqual(row.measured.runs, ["e1", "e2", "e3"]);
  assert.equal(row.measured.traits.reasoning.min, 4.4);
  assert.equal(
    codingSessionProvenanceClause(row),
    "scores measured by registry-bench/verifier v2 on 2026-09-02 (n=3)",
  );
});

test("a half-measured row says which traits are still opinions", () => {
  const result = parseModelRegistry(withMeasured());
  const row = result.registry.targets.find(
    (target) => target.model === "opus[1m]",
  );
  // Three of ten measured; the other seven are named, never elided.
  assert.deepEqual(codingSessionOpinionTraits(row), [
    "coding",
    "taste",
    "agency",
    "discipline",
    "context",
    "velocity",
    "costEfficiency",
  ]);
});

test("a malformed measured block is read as absent, never as partial", () => {
  const cases = [
    // No benchHash.
    `
    measured:
      role: verifier
      benchVersion: 2
      measuredAt: "2026-09-02"
      measuredBy: "aa"
      runs: []
      traits:
        reasoning: { score: 4.6, n: 3, min: 4.4, max: 4.8 }
`,
    // A trait row missing `max`.
    `
    measured:
      role: verifier
      benchVersion: 2
      benchHash: "abc"
      measuredAt: "2026-09-02"
      measuredBy: "aa"
      runs: []
      traits:
        reasoning: { score: 4.6, n: 3, min: 4.4 }
`,
    // No traits at all — a measurement of nothing.
    `
    measured:
      role: verifier
      benchVersion: 2
      benchHash: "abc"
      measuredAt: "2026-09-02"
      measuredBy: "aa"
      runs: []
      traits: {}
`,
  ];
  for (const block of cases) {
    const result = parseModelRegistry(withMeasured(block));
    assert.equal(result.ok, true, result.ok ? "" : result.why);
    const row = result.registry.targets.find(
      (target) => target.model === "opus[1m]",
    );
    assert.equal(
      row.measured,
      undefined,
      "a block this reader cannot vouch for is absent, not partial",
    );
    assert.equal(codingSessionScoreProvenance(row), "legacy");
  }
});

test("the measured key changes no other reading of the row", () => {
  const before = parseModelRegistry(SHIPPED).registry.targets.find(
    (target) => target.model === "opus[1m]",
  );
  const after = parseModelRegistry(withMeasured()).registry.targets.find(
    (target) => target.model === "opus[1m]",
  );
  assert.deepEqual(after.scores, before.scores);
  assert.deepEqual(after.facts, before.facts);
  assert.deepEqual(after.status, before.status);
  assert.deepEqual(after.rating, before.rating);
});

test("the measured clause carries n, the spread and the binding trait", () => {
  const registry = parseModelRegistry(withMeasured()).registry;
  const row = registry.targets.find((target) => target.model === "opus[1m]");
  // The same numbers the Rust router prints. The words used to match and the
  // clauses did not, so the figures a reader needs were CLI-only.
  assert.equal(
    codingSessionProvenanceClause(row, registry.classes.verifier),
    "scores measured by registry-bench/verifier v2 on 2026-09-02 " +
      "(n=3, spread 4.4–4.8 on the binding trait reasoning)",
  );
});

test("a legacy row counts down its grace, and an unparseable date retires nothing", () => {
  const registry = parseModelRegistry(SHIPPED).registry;
  const row = registry.targets.find((target) => target.model === "opus[1m]");
  const gate = {
    ...registry.classes.verifier,
    benchAvailableSince: "2026-09-02",
  };

  assert.equal(codingSessionGraceDaysLeft(gate, "2026-09-20"), 12);
  assert.match(
    codingSessionProvenanceClause(row, gate, "2026-09-20"),
    /— legacy row · bench available · 12 days left$/,
  );

  // No bench for the class: no clock, and the plain legacy sentence.
  assert.equal(
    codingSessionGraceDaysLeft(registry.classes.verifier, "2026-09-20"),
    null,
  );
  assert.match(
    codingSessionProvenanceClause(row, registry.classes.verifier, "2026-09-20"),
    /— this row is legacy$/,
  );

  // A date this reader cannot parse is no date, never an expiry.
  const broken = { ...gate, benchAvailableSince: "last tuesday" };
  assert.equal(codingSessionGraceDaysLeft(broken, "2030-01-01"), null);
  assert.match(
    codingSessionProvenanceClause(row, broken, "2030-01-01"),
    /— this row is legacy$/,
  );
});
