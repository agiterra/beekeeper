import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import test from "node:test";

import {
  compareRubricToCatalog,
  parseRubricBlock,
  resolveRubricStaleness,
  compareRegistryToCatalog,
  resolveRegistryStaleness,
} from "./registryStaleness.ts";

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
    { notOffered: [], unassigned: [], variants: [] },
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
      variants: [],
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
    { notOffered: [], unassigned: [], variants: [] },
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
      variants: [],
    },
  );
});

test("a row naming one variant covers every variant of that base", () => {
  const result = compareRubricToCatalog(
    [
      {
        tier: "tier-2",
        roles: ["builder"],
        provider: "codex-primary",
        model: "gpt-5.6-terra[high]",
        reason: "…",
      },
    ],
    offered([
      ["codex-primary", "gpt-5.6-terra"],
      ["codex-primary", "gpt-5.6-terra[high]"],
      ["codex-primary", "gpt-5.6-terra[low]"],
      ["codex-primary", "gpt-5.6-terra[ultra]"],
    ]),
  );
  assert.deepEqual(result.notOffered, []);
  assert.deepEqual(result.unassigned, []);
  // Nothing is hidden by the collapse.
  assert.deepEqual(result.variants, [
    "codex-primary/gpt-5.6-terra",
    "codex-primary/gpt-5.6-terra[low]",
    "codex-primary/gpt-5.6-terra[ultra]",
  ]);
});

test("a row naming a context variant covers the bare base", () => {
  const { rows } = parseRubricBlock(DOCUMENT);
  const result = compareRubricToCatalog(
    rows,
    offered([
      ["claude-primary", "opus"],
      ["claude-primary", "opus[1m]"],
      ["claude-primary", "sonnet"],
      ["claude-primary", "haiku"],
    ]),
  );
  assert.deepEqual(result.unassigned, []);
  assert.deepEqual(result.variants, ["claude-primary/opus"]);
});

test("an uncovered base is reported once, with an id the catalog offers", () => {
  const { rows } = parseRubricBlock(DOCUMENT);
  const catalog = offered([
    ["claude-primary", "opus[1m]"],
    ["claude-primary", "sonnet"],
    ["claude-primary", "haiku"],
    ["codex-primary", "gpt-5.6-sol"],
    ["codex-primary", "gpt-5.6-sol[high]"],
    ["codex-primary", "gpt-5.6-sol[max]"],
  ]);
  assert.deepEqual(compareRubricToCatalog(rows, catalog).unassigned, [
    "codex-primary/gpt-5.6-sol",
  ]);
  // Paste that id into a row and the gap closes without becoming "not offered".
  const after = compareRubricToCatalog(
    [
      ...rows,
      {
        tier: "tier-2",
        roles: ["builder"],
        provider: "codex-primary",
        model: "gpt-5.6-sol",
        reason: "…",
      },
    ],
    catalog,
  );
  assert.deepEqual(after.notOffered, []);
  assert.deepEqual(after.unassigned, []);
});

test("a base the catalog never offers bare is reported as its variant", () => {
  const result = compareRubricToCatalog(
    [],
    offered([["claude-primary", "claude-fable-5[1m]"]]),
  );
  assert.deepEqual(result.unassigned, ["claude-primary/claude-fable-5[1m]"]);
  assert.deepEqual(result.variants, []);
});

test("the default alias is never unassigned", () => {
  const { rows } = parseRubricBlock(DOCUMENT);
  const state = resolveRubricStaleness({
    rubricText: DOCUMENT,
    offered: offered([
      ["claude-primary", "default"],
      ["claude-primary", "opus[1m]"],
      ["claude-primary", "sonnet"],
      ["claude-primary", "haiku"],
      ["goose-primary", "default"],
    ]),
  });
  assert.equal(state.state, "fresh");
  assert.deepEqual(compareRubricToCatalog(rows, offered([["p", "default"]])), {
    notOffered: [
      "claude-primary/opus[1m]",
      "claude-primary/sonnet",
      "*/haiku",
    ].sort(),
    unassigned: [],
    variants: [],
  });
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
  // The exact fixture asserted in crates/beekeeper-cli/src/commands/sessions/rubric.rs
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
      variants: [],
    },
  );
});

/**
 * The cross-implementation contract. `testdata/routing/live-catalog-665076ce.json`
 * is the kind:44222 catalog this repository's relay really served; the Rust
 * check (`crates/beekeeper-cli/src/commands/sessions/rubric.rs`) asserts the same
 * recorded lists against the same file, so the two cannot drift apart.
 */
test("the live catalog fixture produces the recorded lists", () => {
  const fixture = JSON.parse(
    readFileSync(
      new URL(
        "../../../../../testdata/routing/live-catalog-665076ce.json",
        import.meta.url,
      ),
      "utf8",
    ),
  );
  const rubric = parseRubricBlock(fixture.rubricBlock);
  const result = compareRubricToCatalog(rubric.rows, fixture.offered);
  assert.deepEqual(result.notOffered, fixture.expected.notOffered);
  assert.deepEqual(result.unassigned, fixture.expected.unassigned);
  assert.deepEqual(result.variants, fixture.expected.variants);
  // Every offered id lands in exactly one bucket: the `default` alias, a row's
  // literal id, an unassigned base, or a variant.
  const aliases = fixture.offered.filter(
    (pair) => pair.model.toLowerCase() === "default",
  ).length;
  const named = fixture.offered.filter(
    (pair) =>
      pair.model.toLowerCase() !== "default" &&
      rubric.rows.some(
        (row) =>
          (row.provider === "*" || row.provider === pair.providerInstanceRef) &&
          row.model === pair.model,
      ),
  ).length;
  assert.equal(
    aliases + named + result.unassigned.length + result.variants.length,
    fixture.offered.length,
  );
});

/**
 * The registry badge, which asks the question the routing ruling made the
 * real one: **is anything this host offers missing from the registry?**
 *
 * The other direction is deliberately not staleness. A row for a model this
 * host does not offer today is dormant, which is the registry remembering
 * something (spec §10) — counted and named, never scored.
 */
const ROWS = [
  { provider: "claude-primary", model: "sonnet" },
  { provider: "claude-primary", model: "opus[1m]" },
  { provider: "claude-primary", model: "claude-fable-5[1m]" },
  { provider: "claude-primary", model: "haiku" },
  { provider: "codex-primary", model: "gpt-5.6-sol" },
];

test("an offered target with no registry row is what makes the badge stale", () => {
  const state = resolveRegistryStaleness({
    rows: ROWS,
    version: 1,
    offered: [
      { providerInstanceRef: "claude-primary", model: "sonnet" },
      { providerInstanceRef: "codex-primary", model: "gpt-5.6-sol[high]" },
      { providerInstanceRef: "codex-primary", model: "gpt-5.6-luna[low]" },
    ],
  });
  assert.equal(state.state, "stale");
  assert.deepEqual(state.unregistered, ["codex-primary/gpt-5.6-luna"]);
  assert.match(state.label, /^Registry v1 · 5 rows · 1 offered with no row$/);
});

test("a row the catalog does not offer is dormant, not stale", () => {
  const state = resolveRegistryStaleness({
    rows: ROWS,
    version: 1,
    offered: [{ providerInstanceRef: "claude-primary", model: "sonnet" }],
  });
  assert.equal(state.state, "fresh");
  assert.equal(state.dormant.length, 4);
  assert.match(state.detail, /dormant/);
  assert.match(state.label, /covers the catalog/);
});

test("effort variants are settings of a base a row already names", () => {
  const { unregistered, covered } = compareRegistryToCatalog(ROWS, [
    { providerInstanceRef: "codex-primary", model: "gpt-5.6-sol" },
    { providerInstanceRef: "codex-primary", model: "gpt-5.6-sol[high]" },
    { providerInstanceRef: "codex-primary", model: "gpt-5.6-sol[xhigh]" },
    { providerInstanceRef: "claude-primary", model: "default" },
  ]);
  assert.deepEqual(unregistered, []);
  // `default` is an alias, not a model, and is counted by neither side.
  assert.equal(covered, 3);
});

test("every no-answer is an answer, and none of them reads as fresh", () => {
  const unreadable = resolveRegistryStaleness({
    rows: null,
    unreadableBecause:
      "The router reads /x/team/model-registry.yaml, and this app has no command that reads a project file.",
    offered: [],
  });
  assert.equal(unreadable.state, "unknown");
  assert.equal(unreadable.reason, "registry-unreadable");
  assert.equal(unreadable.label, "Registry: unknown (not readable)");
  assert.match(unreadable.detail, /model-registry\.yaml/);

  const empty = resolveRegistryStaleness({ rows: [], offered: [] });
  assert.equal(empty.state, "unknown");
  assert.equal(empty.reason, "registry-unparseable");

  const noCatalog = resolveRegistryStaleness({ rows: ROWS, offered: null });
  assert.equal(noCatalog.state, "unknown");
  assert.equal(noCatalog.reason, "no-catalog");
  assert.match(noCatalog.label, /no provider catalog/);
});

test("the badge says how many rows it read, in every state that read any", () => {
  // Ledger 97(C): the desktop can read `team/model-registry.yaml` now, so the
  // badge stops saying "not readable" and starts saying what it read. A count
  // is the smallest honest proof that a file was really opened — without it,
  // "no provider catalog" reads identically whether the registry was read or
  // not.
  const stale = resolveRegistryStaleness({
    rows: ROWS,
    version: 1,
    offered: [
      { providerInstanceRef: "claude-primary", model: "sonnet" },
      { providerInstanceRef: "codex-primary", model: "gpt-5.6-luna[low]" },
    ],
  });
  assert.equal(stale.label, "Registry v1 · 5 rows · 1 offered with no row");

  const fresh = resolveRegistryStaleness({
    rows: ROWS,
    version: 1,
    offered: [{ providerInstanceRef: "claude-primary", model: "sonnet" }],
  });
  // Five rows, none of them measured, so the badge says so. Covering the
  // catalog and being decided on evidence are different claims.
  assert.equal(
    fresh.label,
    "Registry v1 · 5 rows · covers the catalog · 1 unmeasured",
  );

  const noCatalog = resolveRegistryStaleness({
    rows: ROWS,
    version: 1,
    offered: null,
  });
  assert.equal(
    noCatalog.label,
    "Registry v1 · 5 rows · no provider catalog yet",
  );
  assert.match(noCatalog.detail, /5 registry rows/);
});

test("unmeasured is its own word, and it never fails the badge", () => {
  const offered = [
    { providerInstanceRef: "claude-primary", model: "sonnet" },
    { providerInstanceRef: "claude-primary", model: "opus[1m]" },
  ];

  // Every row an opinion — the shipped state today.
  const legacy = resolveRegistryStaleness({ rows: ROWS, version: 1, offered });
  assert.equal(legacy.state, "fresh", "unmeasured is not staleness");
  assert.deepEqual(legacy.unmeasured, [
    "claude-primary/opus[1m]",
    "claude-primary/sonnet",
  ]);
  assert.match(legacy.detail, /route on operational priors nobody sampled/);

  // One row measured: it drops out of the list, the other stays.
  const measuredRows = ROWS.map((row) =>
    row.model === "opus[1m]" ? { ...row, measured: true } : row,
  );
  const partly = resolveRegistryStaleness({
    rows: measuredRows,
    version: 1,
    offered,
  });
  assert.equal(partly.state, "fresh");
  assert.deepEqual(partly.unmeasured, ["claude-primary/sonnet"]);

  // All measured: the word disappears rather than reading "0 unmeasured".
  const allMeasured = resolveRegistryStaleness({
    rows: ROWS.map((row) => ({ ...row, measured: true })),
    version: 1,
    offered,
  });
  assert.deepEqual(allMeasured.unmeasured, []);
  assert.equal(allMeasured.label, "Registry v1 · 5 rows · covers the catalog");
  assert.doesNotMatch(allMeasured.detail, /unmeasured/);
});

test("the three words name three different things and never overlap", () => {
  const { unregistered, dormant, unmeasured } = compareRegistryToCatalog(
    [
      { provider: "claude-primary", model: "sonnet" },
      { provider: "claude-primary", model: "opus[1m]", measured: true },
      { provider: "codex-primary", model: "gpt-5.6-sol" },
    ],
    [
      { providerInstanceRef: "claude-primary", model: "sonnet" },
      { providerInstanceRef: "claude-primary", model: "opus[1m]" },
      { providerInstanceRef: "claude-primary", model: "gpt-6-nova[high]" },
    ],
  );
  assert.deepEqual(unregistered, ["claude-primary/gpt-6-nova"]);
  assert.deepEqual(dormant, ["codex-primary/gpt-5.6-sol"]);
  assert.deepEqual(unmeasured, ["claude-primary/sonnet"]);
  for (const label of unmeasured) {
    assert.ok(!unregistered.includes(label));
    assert.ok(!dormant.includes(label));
  }
});

test("a stale registry still reports its unmeasured rows", () => {
  const state = resolveRegistryStaleness({
    rows: ROWS,
    version: 1,
    offered: [
      { providerInstanceRef: "claude-primary", model: "sonnet" },
      { providerInstanceRef: "claude-primary", model: "gpt-6-nova[high]" },
    ],
  });
  assert.equal(state.state, "stale");
  assert.deepEqual(state.unmeasured, ["claude-primary/sonnet"]);
  assert.match(state.detail, /Offered here and in no registry row/);
  assert.match(state.detail, /unmeasured/);
});

test("F4: the badge's measured flag comes from the row, so it can ever change", () => {
  // The producer used to map { provider, model } and nothing else, so the
  // badge said "11 unmeasured" forever — right today by accident, and wrong
  // the moment a row is genuinely measured.
  const measuredRows = ROWS.map((row) =>
    row.model === "sonnet" ? { ...row, measured: true } : row,
  );
  const state = resolveRegistryStaleness({
    rows: measuredRows,
    version: 1,
    offered: [
      { providerInstanceRef: "claude-primary", model: "sonnet" },
      { providerInstanceRef: "claude-primary", model: "opus[1m]" },
    ],
  });
  assert.deepEqual(state.unmeasured, ["claude-primary/opus[1m]"]);
  assert.equal(
    state.label,
    "Registry v1 · 5 rows · covers the catalog · 1 unmeasured",
  );
});
