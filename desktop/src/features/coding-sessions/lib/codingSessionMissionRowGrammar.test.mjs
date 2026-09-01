import assert from "node:assert/strict";
import test from "node:test";

import {
  missionRowBodyClass,
  missionRowClass,
  missionRowMetaClass,
  missionRowTitleClass,
} from "./codingSessionMissionRowGrammar.ts";

const WEIGHTS = ["attention", "standard", "quiet"];

test("U-T7: the row grammar emits only theme tokens and rem text tokens", () => {
  const emitted = [
    ...WEIGHTS.map((weight) => missionRowClass(weight)),
    missionRowClass("attention", { tone: "critical" }),
    missionRowClass("attention", { tone: "caution" }),
    missionRowClass("standard", { className: "pl-4" }),
    missionRowTitleClass(),
    missionRowBodyClass(),
    missionRowMetaClass(),
  ];
  for (const classes of emitted) {
    assert.doesNotMatch(
      classes,
      /text-\[/,
      `arbitrary text literal in: ${classes}`,
    );
    assert.doesNotMatch(
      classes,
      /#[0-9a-f]{3,8}\b/i,
      `hex colour in: ${classes}`,
    );
    assert.doesNotMatch(
      classes,
      /\b(?:rgb|hsl|oklch)\(/i,
      `raw colour function in: ${classes}`,
    );
  }
});

test("U-T7: three weights are three visibly different shells", () => {
  const attention = missionRowClass("attention", { tone: "critical" });
  const caution = missionRowClass("attention", { tone: "caution" });
  const standard = missionRowClass("standard");
  const quiet = missionRowClass("quiet");
  assert.match(attention, /border-destructive\/40/);
  assert.match(caution, /border-amber-500\/45/);
  assert.match(standard, /border-border\/60/);
  assert.doesNotMatch(quiet, /\bborder\b/);
  assert.match(quiet, /text-muted-foreground/);
  assert.notEqual(attention, standard);
  assert.notEqual(standard, quiet);
  assert.notEqual(attention, caution);
});

test("U-T7: caller classes merge last so layout never forks the grammar", () => {
  assert.match(missionRowClass("standard", { className: "pl-4" }), /pl-4/);
  assert.match(
    missionRowClass("quiet", { className: "text-center" }),
    /text-center/,
  );
});

test("U-T7: the text ramp is title text-sm, body text-xs, meta text-2xs", () => {
  assert.match(missionRowTitleClass(), /\btext-sm\b/);
  assert.match(missionRowBodyClass(), /\btext-xs\b/);
  assert.match(missionRowMetaClass(), /\btext-2xs\b/);
});
