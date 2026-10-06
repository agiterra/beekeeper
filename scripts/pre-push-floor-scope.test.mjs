// Contract tests for the pre-push floor's path -> scope mapping.
//
// The mapping decides what a push runs. Two failure shapes matter, and they
// are opposites: running too much brings back the 60 s `git push` that made
// seats pass `--no-verify` in the first place, and running too little turns a
// green push into a red CI run nobody was watching. Every case below is one or
// the other.
//
// Run: node --test scripts/pre-push-floor-scope.test.mjs

import assert from "node:assert/strict";
import test from "node:test";

import {
  buildGraph,
  byWorkspace,
  deriveScope,
  desktopTestPlan,
} from "./pre-push-floor-scope.mjs";

// A graph shaped like this repository's, small enough to read. `sprig` and
// `beekeeper-dev-mcp` really do depend on `beekeeper-cli` (crates/beekeeper-dev-mcp/
// Cargo.toml:17), which is why the CLI is not the leaf it looks like.
const metadataRoot = {
  packages: [
    {
      name: "beekeeper-core",
      manifest_path: "/repo/crates/beekeeper-core/Cargo.toml",
      dependencies: [],
    },
    {
      name: "beekeeper-cli",
      manifest_path: "/repo/crates/beekeeper-cli/Cargo.toml",
      dependencies: [{ name: "beekeeper-core" }, { name: "beekeeper-persona" }],
    },
    {
      name: "beekeeper-persona",
      manifest_path: "/repo/crates/beekeeper-persona/Cargo.toml",
      dependencies: [{ name: "beekeeper-core" }],
    },
    {
      name: "beekeeper-dev-mcp",
      manifest_path: "/repo/crates/beekeeper-dev-mcp/Cargo.toml",
      dependencies: [{ name: "beekeeper-cli" }, { name: "serde" }],
    },
    {
      name: "sprig",
      manifest_path: "/repo/crates/sprig/Cargo.toml",
      dependencies: [{ name: "beekeeper-cli" }],
    },
    {
      name: "beekeeper-acp",
      manifest_path: "/repo/crates/beekeeper-acp/Cargo.toml",
      dependencies: [{ name: "beekeeper-core" }],
    },
  ],
};

const metadataTauri = {
  packages: [
    {
      name: "beekeeper-desktop",
      manifest_path: "/repo/desktop/src-tauri/Cargo.toml",
      dependencies: [{ name: "beekeeper-core" }, { name: "beekeeper-terminal" }],
    },
    {
      name: "beekeeper-terminal",
      manifest_path: "/repo/desktop/src-tauri/crates/beekeeper-terminal/Cargo.toml",
      dependencies: [],
    },
  ],
};

const graph = buildGraph([
  { workspace: "root", root: "/repo", metadata: metadataRoot },
  { workspace: "tauri", root: "/repo", metadata: metadataTauri },
]);

// A graph where beekeeper-cli genuinely has no dependents, for the one case the
// lane spec names literally.
const leafGraph = buildGraph([
  {
    workspace: "root",
    root: "/repo",
    metadata: {
      packages: [
        {
          name: "beekeeper-cli",
          manifest_path: "/repo/crates/beekeeper-cli/Cargo.toml",
          dependencies: [],
        },
      ],
    },
  },
]);

test("buildGraph derives manifest dirs, workspaces and transitive dependents", () => {
  assert.equal(graph.packages["beekeeper-cli"].dir, "crates/beekeeper-cli");
  assert.equal(graph.packages["beekeeper-cli"].workspace, "root");
  assert.equal(graph.packages["beekeeper-terminal"].workspace, "tauri");
  assert.equal(
    graph.packages["beekeeper-terminal"].dir,
    "desktop/src-tauri/crates/beekeeper-terminal",
  );

  // Transitive and cross-workspace: beekeeper-core reaches the Tauri crate.
  assert.deepEqual(graph.dependents["beekeeper-core"], [
    "beekeeper-acp",
    "beekeeper-cli",
    "beekeeper-desktop",
    "beekeeper-dev-mcp",
    "beekeeper-persona",
    "sprig",
  ]);
  assert.deepEqual(graph.dependents["beekeeper-cli"], ["beekeeper-dev-mcp", "sprig"]);
  assert.deepEqual(graph.dependents.sprig, []);

  // `serde` is not a workspace member, so it never appears as something to
  // lint with `-p`.
  assert.ok(!("serde" in graph.dependents));
});

test("a CLI-only change selects the CLI and nothing else", () => {
  const scope = deriveScope(["crates/beekeeper-cli/src/lib.rs"], leafGraph);
  assert.equal(scope.full, false);
  assert.deepEqual(scope.changedPackages, ["beekeeper-cli"]);
  assert.deepEqual(scope.lintPackages, ["beekeeper-cli"]);
  assert.equal(scope.desktop, false);
  assert.equal(scope.web, false);
  assert.equal(scope.mobile, false);
  assert.deepEqual(scope.unmapped, []);
});

test("fmt and tests stay on the changed crate; clippy widens to dependents", () => {
  const scope = deriveScope(["crates/beekeeper-cli/src/lib.rs"], graph);
  // What the crate's own fmt and tests cover.
  assert.deepEqual(scope.changedPackages, ["beekeeper-cli"]);
  // What must still compile: the crates that depend on it.
  assert.deepEqual(scope.lintPackages, ["beekeeper-cli", "beekeeper-dev-mcp", "sprig"]);
});

test("a beekeeper-core change selects its dependents, across both workspaces", () => {
  const scope = deriveScope(["crates/beekeeper-core/src/kind.rs"], graph);
  assert.deepEqual(scope.changedPackages, ["beekeeper-core"]);
  assert.deepEqual(scope.lintPackages, [
    "beekeeper-acp",
    "beekeeper-cli",
    "beekeeper-core",
    "beekeeper-desktop",
    "beekeeper-dev-mcp",
    "beekeeper-persona",
    "sprig",
  ]);
  assert.deepEqual(byWorkspace(scope.lintPackages, graph).tauri, [
    "beekeeper-desktop",
  ]);
  assert.equal(
    scope.desktop,
    false,
    "Rust dependents do not pull in the TS surface",
  );
});

test("Cargo.lock, rust-toolchain.toml and justfile each select the full floor", () => {
  for (const path of ["Cargo.lock", "rust-toolchain.toml", "justfile"]) {
    const scope = deriveScope([path], graph);
    assert.equal(scope.full, true, `${path} must select the full floor`);
    assert.ok(
      scope.fullReasons.some((reason) => reason.includes(path)),
      `${path} must say why it widened the floor, got ${JSON.stringify(scope.fullReasons)}`,
    );
    assert.deepEqual(scope.changedPackages, Object.keys(graph.packages).sort());
    assert.equal(scope.desktop, true);
    assert.equal(scope.web, true);
    assert.equal(scope.mobile, true);
  }
});

test("the floor's own tooling selects the full floor", () => {
  for (const path of [
    "lefthook.yml",
    "scripts/pre-push-floor.sh",
    ".lefthook/pre-push/x.sh",
  ]) {
    assert.equal(
      deriveScope([path], graph).full,
      true,
      `${path} must widen the floor`,
    );
  }
});

test("an unmapped path selects the full floor and says which path did it", () => {
  const scope = deriveScope(["some/new/surface/main.go"], graph);
  assert.equal(scope.full, true);
  assert.deepEqual(scope.unmapped, ["some/new/surface/main.go"]);
  assert.deepEqual(scope.fullReasons, [
    "some/new/surface/main.go maps to no scope, so the full floor runs",
  ]);
});

test("desktop, web and mobile paths select their own surface only", () => {
  const desktop = deriveScope(["desktop/src/app/App.tsx"], graph);
  assert.deepEqual(desktop, {
    full: false,
    fullReasons: [],
    changedPackages: [],
    lintPackages: [],
    desktop: true,
    web: false,
    mobile: false,
    unmapped: [],
    noScope: [],
  });

  const web = deriveScope(["web/src/main.ts"], graph);
  assert.equal(web.web, true);
  assert.equal(web.desktop, false);
  assert.deepEqual(web.changedPackages, []);

  const mobile = deriveScope(["mobile/lib/main.dart"], graph);
  assert.equal(mobile.mobile, true);
  assert.deepEqual(mobile.changedPackages, []);
  assert.equal(mobile.desktop, false);
});

test("desktop/src-tauri is the Tauri crate, not the TS surface", () => {
  const scope = deriveScope(["desktop/src-tauri/src/commands/mod.rs"], graph);
  assert.deepEqual(scope.changedPackages, ["beekeeper-desktop"]);
  assert.equal(scope.desktop, false);

  const terminal = deriveScope(
    ["desktop/src-tauri/crates/beekeeper-terminal/src/lib.rs"],
    graph,
  );
  assert.deepEqual(
    terminal.changedPackages,
    ["beekeeper-terminal"],
    "the longest manifest dir wins, or a nested crate is attributed to its parent",
  );
});

test("a markdown file under crates/ is source, not documentation", () => {
  // crates/beekeeper-acp/src/base_prompt.md is include_str!'d into a const.
  const scope = deriveScope(["crates/beekeeper-acp/src/base_prompt.md"], graph);
  assert.deepEqual(scope.changedPackages, ["beekeeper-acp"]);
  assert.deepEqual(scope.noScope, []);
});

test("personas/** selects the crate whose tests read the packs", () => {
  const scope = deriveScope(
    ["personas/roles/lead/skills/hire/SKILL.md"],
    graph,
  );
  assert.deepEqual(scope.changedPackages, ["beekeeper-persona"]);
  assert.equal(scope.full, false);
});

test("docs and CI configuration select nothing", () => {
  const scope = deriveScope(
    [
      "docs/INTEGRATION.md",
      "README.md",
      ".github/workflows/ci.yml",
      ".woodpecker/gate.yml",
    ],
    graph,
  );
  assert.equal(scope.full, false);
  assert.deepEqual(scope.changedPackages, []);
  assert.equal(scope.desktop, false);
  assert.equal(scope.noScope.length, 4);
});

test("pnpm-lock.yaml selects both JS surfaces and no cargo", () => {
  const scope = deriveScope(["pnpm-lock.yaml"], graph);
  assert.equal(scope.desktop, true);
  assert.equal(scope.web, true);
  assert.deepEqual(scope.changedPackages, []);
  assert.equal(scope.full, false);
});

test("a mixed change is the union, and one unmapped path still widens it", () => {
  const union = deriveScope(
    ["crates/beekeeper-cli/src/lib.rs", "desktop/src/app/App.tsx", "docs/x.md"],
    graph,
  );
  assert.deepEqual(union.changedPackages, ["beekeeper-cli"]);
  assert.equal(union.desktop, true);
  assert.equal(union.mobile, false);
  assert.equal(union.full, false);

  const widened = deriveScope(
    ["crates/beekeeper-cli/src/lib.rs", "Makefile"],
    graph,
  );
  assert.equal(widened.full, true);
  assert.deepEqual(widened.unmapped, ["Makefile"]);
});

test("an empty change set runs no scope rather than everything", () => {
  const scope = deriveScope([], graph);
  assert.equal(scope.full, false);
  assert.deepEqual(scope.changedPackages, []);
  assert.equal(scope.desktop, false);
});

// ── desktopTestPlan: batch 3 L14 fix round 1 ─────────────────────────────────
// Every push used to run the full desktop suite (7588 tests / 81 suites,
// 989-1410s measured on a shared host) for any desktop/** change. These
// cases pin the sibling-test mapping that replaces it. `existsFn` is a fake
// set-membership check standing in for the filesystem — no disk touched.

test("a changed source file selects its sibling .test.mjs, not the suite", () => {
  const exists = (p) => p === "desktop/src/app/AppShell.helpers.test.mjs";
  const plan = desktopTestPlan(
    ["desktop/src/app/AppShell.helpers.ts"],
    exists,
  );
  assert.equal(plan.full, false);
  assert.deepEqual(plan.testFiles, [
    "desktop/src/app/AppShell.helpers.test.mjs",
  ]);
  assert.deepEqual(plan.untested, []);
});

test("a changed test file selects itself", () => {
  const exists = () => false; // irrelevant: the changed path IS the test
  const plan = desktopTestPlan(
    ["desktop/src/shared/deep-link.test.mjs"],
    exists,
  );
  assert.deepEqual(plan.testFiles, ["desktop/src/shared/deep-link.test.mjs"]);
  assert.equal(plan.full, false);
});

test("a source file with no sibling test is named, not silently dropped", () => {
  const exists = () => false;
  const plan = desktopTestPlan(["desktop/src/app/App.tsx"], exists);
  assert.deepEqual(plan.testFiles, []);
  assert.deepEqual(plan.untested, ["desktop/src/app/App.tsx"]);
  assert.equal(plan.full, false);
});

test("a multi-dot basename maps to the whole prefix, not just the last segment", () => {
  const exists = (p) => p === "desktop/src/app/AppShell.helpers.test.mjs";
  const plan = desktopTestPlan(
    ["desktop/src/app/AppShell.helpers.ts"],
    exists,
  );
  assert.deepEqual(plan.testFiles, [
    "desktop/src/app/AppShell.helpers.test.mjs",
  ]);
});

test("desktop/** outside desktop/src/** is not covered by the sibling rule: full suite", () => {
  const plan = desktopTestPlan(["desktop/vite.config.ts"], () => true);
  assert.equal(plan.full, true);
  assert.deepEqual(plan.testFiles, []);
  assert.ok(
    plan.fullReasons.some((reason) => reason.includes("desktop/vite.config.ts")),
  );
});

test("desktop/src-tauri/** is Rust, not the sibling-test surface", () => {
  const plan = desktopTestPlan(
    ["desktop/src-tauri/src/commands/mod.rs"],
    () => false,
  );
  assert.equal(plan.full, false);
  assert.deepEqual(plan.testFiles, []);
  assert.deepEqual(plan.untested, []);
});

test("a mixed desktop change unions tested, untested and dedupes repeats", () => {
  let calls = 0;
  const exists = (p) => {
    calls += 1;
    return p === "desktop/src/app/Foo.test.mjs";
  };
  const plan = desktopTestPlan(
    [
      "desktop/src/app/Foo.tsx",
      "desktop/src/app/Foo.test.mjs", // the sibling itself also changed
      "desktop/src/app/Bar.tsx", // no sibling
    ],
    exists,
  );
  assert.deepEqual(plan.testFiles, ["desktop/src/app/Foo.test.mjs"]);
  assert.deepEqual(plan.untested, ["desktop/src/app/Bar.tsx"]);
  assert.equal(plan.full, false);
  assert.ok(calls > 0, "existsFn must be consulted for the source file");
});

test("no desktop paths at all: an empty, non-full plan", () => {
  const plan = desktopTestPlan(["crates/beekeeper-cli/src/lib.rs"], () => true);
  assert.deepEqual(plan, { full: false, fullReasons: [], testFiles: [], untested: [] });
});
