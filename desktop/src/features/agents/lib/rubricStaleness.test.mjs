import assert from "node:assert/strict";
import test from "node:test";

import {
  compareRubricToCatalog,
  parseRubricBlock,
  resolveRubricStaleness,
} from "./rubricStaleness.ts";

const DOCUMENT = [
  "# Choose a model",
  "",
  "Some prose.",
  "",
  "```rubric v3",
  "| tier | role(s) | provider | model id | reason |",
  "| --- | --- | --- | --- | --- |",
  "| deep | lead, architect | claude-primary | opus[1m] | planning |",
  "| fast | builder, runner | claude-primary | `sonnet` | throughput |",
  "| any | poker | * | haiku | cheap adversarial passes |",
  "```",
  "",
  "More prose.",
].join("\n");

function offered(pairs) {
  return pairs.map(([providerInstanceRef, model]) => ({
    providerInstanceRef,
    model,
  }));
}

test("the rubric block is parsed with its version, roles and ids", () => {
  const rubric = parseRubricBlock(DOCUMENT);
  assert.equal(rubric.version, "v3");
  assert.equal(rubric.rows.length, 3);
  assert.deepEqual(rubric.rows[0].roles, ["lead", "architect"]);
  assert.equal(rubric.rows[0].model, "opus[1m]");
  // Backticks are markdown, not part of the id.
  assert.equal(rubric.rows[1].model, "sonnet");
  assert.equal(rubric.rows[2].provider, "*");
});

test("a document with no rubric block parses to null", () => {
  assert.equal(parseRubricBlock("# nothing here\n\n```json\n{}\n```\n"), null);
  assert.equal(
    parseRubricBlock("```not-rubric\n| a | b | c | d | e |\n```\n"),
    null,
  );
  // A header with no data rows is not a rubric either.
  assert.equal(
    parseRubricBlock(
      "```rubric v1\n| a | b | c | d | e |\n| - | - | - | - | - |\n```\n",
    ),
    null,
  );
});

test("a rubric with no version parses and reports null", () => {
  const rubric = parseRubricBlock(
    "```rubric\n| tier | role(s) | provider | model id | reason |\n| - | - | - | - | - |\n| a | lead | p | m | r |\n```\n",
  );
  assert.equal(rubric.version, null);
  assert.equal(rubric.rows.length, 1);
});

test("a rubric matching the catalog has both lists empty", () => {
  const { rows } = parseRubricBlock(DOCUMENT);
  assert.deepEqual(
    compareRubricToCatalog(
      rows,
      offered([
        ["claude-primary", "opus[1m]"],
        ["claude-primary", "sonnet"],
        ["claude-primary", "haiku"],
      ]),
    ),
    { notOffered: [], unassigned: [] },
  );
});

test("both ways a rubric goes stale are reported by name, never repaired", () => {
  const { rows } = parseRubricBlock(DOCUMENT);
  assert.deepEqual(
    compareRubricToCatalog(
      rows,
      offered([
        ["claude-primary", "sonnet"],
        ["claude-primary", "haiku"],
        ["claude-primary", "claude-fable-5[1m]"],
      ]),
    ),
    {
      notOffered: ["claude-primary/opus[1m]"],
      unassigned: ["claude-primary/claude-fable-5[1m]"],
    },
  );
});

test("a wildcard provider covers every provider offering the id", () => {
  const { rows } = parseRubricBlock(DOCUMENT);
  assert.deepEqual(
    compareRubricToCatalog(
      rows,
      offered([
        ["claude-primary", "opus[1m]"],
        ["claude-primary", "sonnet"],
        ["claude-primary", "haiku"],
        ["second-host", "haiku"],
      ]),
    ),
    { notOffered: [], unassigned: [] },
  );
});

test("the right id on the wrong provider is not offered", () => {
  assert.deepEqual(
    compareRubricToCatalog(
      [
        {
          tier: "deep",
          roles: ["lead"],
          provider: "codex-primary",
          model: "opus[1m]",
          reason: "…",
        },
      ],
      offered([["claude-primary", "opus[1m]"]]),
    ),
    {
      notOffered: ["codex-primary/opus[1m]"],
      unassigned: ["claude-primary/opus[1m]"],
    },
  );
});

test("an unreadable pack is disclosed, never rendered as fresh", () => {
  const state = resolveRubricStaleness({ rubricText: null, offered: [] });
  assert.equal(state.state, "unknown");
  assert.equal(state.reason, "pack-unreadable");
  assert.match(state.label, /pack not readable/);
});

test("an absent catalog is disclosed, never rendered as fresh", () => {
  const state = resolveRubricStaleness({
    rubricText: DOCUMENT,
    offered: null,
  });
  assert.equal(state.state, "unknown");
  assert.equal(state.reason, "no-catalog");
  assert.match(state.label, /no provider catalog/);
});

test("a pack with no rubric block is its own disclosure", () => {
  const state = resolveRubricStaleness({
    rubricText: "# lead\n\nno rubric here\n",
    offered: [],
  });
  assert.equal(state.state, "unknown");
  assert.equal(state.reason, "no-rubric-block");
});

test("an empty catalog never passes as a fresh rubric", () => {
  const state = resolveRubricStaleness({ rubricText: DOCUMENT, offered: [] });
  assert.equal(state.state, "stale");
  assert.equal(state.notOffered.length, 3);
  assert.deepEqual(state.unassigned, []);
});

test("the badge label counts both lists and carries the version", () => {
  const stale = resolveRubricStaleness({
    rubricText: DOCUMENT,
    offered: offered([
      ["claude-primary", "sonnet"],
      ["claude-primary", "haiku"],
      ["claude-primary", "claude-fable-5[1m]"],
    ]),
  });
  assert.equal(stale.state, "stale");
  assert.equal(stale.version, "v3");
  assert.equal(stale.label, "Rubric stale — 1 not offered · 1 unassigned");

  const fresh = resolveRubricStaleness({
    rubricText: DOCUMENT,
    offered: offered([
      ["claude-primary", "opus[1m]"],
      ["claude-primary", "sonnet"],
      ["claude-primary", "haiku"],
    ]),
  });
  assert.equal(fresh.state, "fresh");
  assert.equal(fresh.label, "Rubric v3 matches the catalog");
});

/**
 * The desktop rule and `bee sessions rubric check` must not drift: the same
 * rubric and the same catalog have to produce the same two lists, or the badge
 * and the command would disagree about whether the rubric is true.
 */
test("the rule matches the CLI's, including its wildcard and pair semantics", () => {
  const { rows } = parseRubricBlock(DOCUMENT);
  // The exact fixture asserted in crates/buzz-cli/src/commands/sessions/rubric.rs
  // (`an_unoffered_row_and_an_unnamed_model_are_both_reported`).
  assert.deepEqual(
    compareRubricToCatalog(
      rows,
      offered([
        ["claude-primary", "sonnet"],
        ["claude-primary", "haiku"],
        ["claude-primary", "claude-fable-5[1m]"],
      ]),
    ),
    {
      notOffered: ["claude-primary/opus[1m]"],
      unassigned: ["claude-primary/claude-fable-5[1m]"],
    },
  );
});
