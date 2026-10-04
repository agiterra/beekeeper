import assert from "node:assert/strict";
import test from "node:test";

import {
  compare,
  lineOfTitle,
  nearestRenderedIds,
  outcomes,
  registrationOnly,
  smokeSpecs,
  specNames,
  specStrings,
  testIdsIn,
  textLiteralsIn,
  untrustworthy,
} from "./e2e-affected.mjs";

test("testIdsIn reads literals, braces, ternaries, props and template heads", () => {
  const { exact, prefixes } = testIdsIn(`
    <div data-testid="plain-id" />
    <div data-testid={"braced-id"} />
    <div data-testid={open ? "panel-open" : 'panel-closed'} />
    <Row testId="row-from-prop" listTestId={\`session-row-\${id}\`} />
    const props = { "data-testid": "spread-id" };
    <div data-testid={\`exact-template\`} />
    <div data-testid={\`ab-\${x}\`} />
  `);
  assert.deepEqual([...exact].sort(), [
    "braced-id",
    "exact-template",
    "panel-closed",
    "panel-open",
    "plain-id",
    "row-from-prop",
    "spread-id",
  ]);
  // `ab-` is too short a head to mean anything and is dropped.
  assert.deepEqual([...prefixes], ["session-row-"]);
});

test("specNames matches exact ids, template heads both ways, and ignores short families", () => {
  const spec = specStrings(`
    await page.getByTestId("plain-id").click();
    page.locator('[data-testid="selector-id"]');
    page.getByTestId(\`coding-session-row-\${id}\`);
    page.getByTestId("session-row-42");
  `);
  const ids = (exact, prefixes = []) => ({
    exact: new Set(exact),
    prefixes: new Set(prefixes),
  });
  assert.equal(specNames(spec, ids(["plain-id"])), "plain-id");
  assert.equal(specNames(spec, ids(["selector-id"])), "selector-id");
  assert.equal(
    specNames(spec, ids(["coding-session-row-header"])),
    "coding-session-row-…",
  );
  assert.equal(specNames(spec, ids([], ["session-row-"])), "session-row-…");
  assert.equal(specNames(spec, ids(["unrelated-id"])), null);
  // `channel-` names a family, not a component.
  const family = specStrings(`page.getByTestId("channel-general")`);
  assert.equal(specNames(family, ids([], ["channel-"])), null);
});

test("nearestRenderedIds stops at the first file that renders ids", () => {
  // hook.ts → Panel.tsx (renders) → Page.tsx (renders): only Panel counts.
  const importers = new Map([
    ["src/missing/hook.ts", new Set(["src/missing/Panel.tsx"])],
    ["src/missing/Panel.tsx", new Set(["src/missing/Page.tsx"])],
  ]);
  // Neither file exists, so both render nothing and the climb ends empty —
  // a deleted file reaches no spec rather than throwing.
  const ids = nearestRenderedIds("src/missing/hook.ts", importers);
  assert.equal(ids.exact.size + ids.prefixes.size, 0);
});

test("registrationOnly accepts testMatch edits and refuses anything else", () => {
  const added = [
    "diff --git a/desktop/playwright.config.ts b/desktop/playwright.config.ts",
    "@@ -60,6 +60,8 @@",
    '         "**/old.spec.ts",',
    "+        // Wave A specs.",
    '+        "**/new-one.spec.ts",',
    '-        "**/gone.spec.ts",',
  ].join("\n");
  assert.deepEqual(registrationOnly(added), ["new-one.spec.ts"]);
  assert.equal(
    registrationOnly("@@ -1 +1 @@\n-  workers: 1,\n+  workers: 4,"),
    null,
  );
});

test("smokeSpecs takes every project whose name starts with smoke", () => {
  const config = `
    projects: [
      { name: "smoke", testMatch: ["**/a.spec.ts", "**/b.spec.ts"] },
      { name: "smoke-serial", testMatch: ["**/c.spec.ts"] },
      { name: "integration", testMatch: ["**/d.spec.ts"] },
    ]`;
  assert.deepEqual([...smokeSpecs(config)].sort(), [
    "a.spec.ts",
    "b.spec.ts",
    "c.spec.ts",
  ]);
});

const report = {
  suites: [
    {
      title: "known.spec.ts",
      file: "known.spec.ts",
      specs: [
        {
          title: "still broken",
          file: "known.spec.ts",
          line: 10,
          tests: [{ status: "unexpected" }],
        },
        {
          title: "fixed now",
          file: "known.spec.ts",
          line: 20,
          tests: [{ status: "expected" }],
        },
      ],
      suites: [
        {
          title: "group",
          file: "known.spec.ts",
          specs: [
            {
              title: "regressed",
              file: "known.spec.ts",
              line: 30,
              tests: [{ status: "unexpected" }],
            },
          ],
        },
      ],
    },
  ],
};

test("outcomes keeps describe titles in the title path", () => {
  assert.deepEqual(
    outcomes(report).map((o) => o.title),
    ["still broken", "fixed now", "group › regressed"],
  );
});

test("compare separates new failures from known ones and names known passes", () => {
  const known = [
    { spec: "known.spec.ts", title: "still broken" },
    { spec: "known.spec.ts", title: "fixed now" },
  ];
  const r = compare(report, known);
  assert.deepEqual(
    r.fresh.map((x) => x.title),
    ["group › regressed"],
  );
  assert.deepEqual(
    r.stillKnown.map((x) => x.title),
    ["still broken"],
  );
  assert.deepEqual(
    r.nowPassing.map((x) => x.title),
    ["fixed now"],
  );
});

const reportOf = (specs, errors = []) => ({
  errors,
  suites: [
    {
      title: "x.spec.ts",
      file: "x.spec.ts",
      specs: specs.map(([title, status]) => ({
        title,
        file: "x.spec.ts",
        line: 1,
        tests: [{ status }],
      })),
    },
  ],
});

test("untrustworthy refuses a run that never ran, errored, or exited unexplained", () => {
  // A spec that fails to import aborts the run: zero results, one error.
  const broken = {
    errors: [{ message: "Error: Cannot find module './gone'" }],
    suites: [],
  };
  const why = untrustworthy([broken, { suites: [] }], [1, 0]);
  assert.ok(why.some((w) => w.includes("Cannot find module")));
  assert.ok(why.some((w) => w.includes("no test ran")));
  assert.ok(why.some((w) => w.includes("exited 1 with no failing test")));
  // A failing test explains a non-zero exit; a clean run is trusted.
  assert.deepEqual(untrustworthy([reportOf([["a", "unexpected"]])], [1]), []);
  assert.deepEqual(untrustworthy([reportOf([["a", "expected"]])], [0]), []);
});

test("untrustworthy names a core test that did not run", () => {
  const core = [
    { spec: "x.spec.ts", title: "a" },
    { spec: "x.spec.ts", title: "missing" },
  ];
  const why = untrustworthy([reportOf([["a", "expected"]])], [0], core);
  assert.deepEqual(why, ["core test did not run: x.spec.ts › missing"]);
});

test("textLiteralsIn keeps visible text and drops paths, classes and selectors", () => {
  const t = textLiteralsIn(`
    { label: "In 30 minutes" }, "Tomorrow at 9am", 'flex items-center gap-2',
    "./some/path here", "[data-testid=x] y", "short", "nospacesatall"
  `);
  assert.deepEqual([...t].sort(), ["In 30 minutes", "Tomorrow at 9am"]);
});

test("lineOfTitle returns the test( call line even when the title wraps", () => {
  const text = [
    "import x;",
    "test(",
    '  "a wrapped title",',
    "  async () => {});",
    'test("inline title", async () => {});',
  ].join("\n");
  assert.equal(lineOfTitle(text, "a wrapped title"), 2);
  assert.equal(lineOfTitle(text, "inline title"), 5);
  assert.equal(lineOfTitle(text, "absent"), null);
});
