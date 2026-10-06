#!/usr/bin/env node
// Path -> scope mapping for the pre-push floor.
//
// The floor is small on purpose. Before this existed, `pre-push` ran a gate
// sized for a human with a coffee: `just test-unit` is `cargo test
// --workspace` whatever the push touched, and the Tauri row ran
// `desktop-tauri-clippy && desktop-tauri-test` for any `crates/**` change. A
// seat pushing a one-line CLI fix hit its 60 s tool timeout on `git push`
// (LIVE-RUN-TeamRolesV1, 11:01), and the answer that night was to tell seats
// `--no-verify`. A gate everyone skips is not a gate, so the budget is fixed
// by making the floor small rather than by making people skip it.
//
// Everything here is a pure function of its arguments so the mapping can be
// tested without a git push and without cargo: `deriveScope` takes the changed
// paths plus a package graph, `buildGraph` takes already-parsed `cargo
// metadata` documents. Only `main()` at the bottom touches the world.
//
// Two rules are load-bearing:
//
//   - A path that maps to no scope maps to the FULL floor, never to nothing.
//     An unmapped path is a coverage hole, and the safe direction is to run
//     more. The full branch says which path put it there.
//   - Dependents are derived from `cargo metadata`, never from a hand-kept
//     list. A hand-kept list is the drift `lefthook.yml`'s own header warns
//     about, and it silently under-runs the moment a crate gains a dependant.

/**
 * Paths that select the full floor outright: they can change how anything
 * builds, so no narrower scope is honest. `justfile` and `scripts/**` are here
 * because the floor calls into them.
 */
export const FULL_FLOOR_EXACT = new Set([
  "Cargo.toml",
  "Cargo.lock",
  "rust-toolchain.toml",
  "deny.toml",
  "justfile",
  "lefthook.yml",
  ".rustfmt.toml",
  "rustfmt.toml",
]);

/** Directory prefixes that select the full floor. */
export const FULL_FLOOR_PREFIXES = [
  ".lefthook/",
  "scripts/",
  "migrations/",
  "schema/",
];

/**
 * Paths that select nothing at all. Only locations that cannot reach a build.
 *
 * `**\/*.md` is deliberately NOT here: sixteen `.md` files live under
 * `crates/`, and `crates/beekeeper-acp/src/base_prompt.md` is `include_str!`'d into
 * a `const` — compiled source that happens to end in `.md`. This is the same
 * trap `scripts/test-woodpecker-path-filter.sh` exists to prevent, and the
 * package-prefix rule below runs first so those files map to their crate.
 */
export const NO_SCOPE_PREFIXES = [
  "docs/",
  ".github/",
  ".woodpecker/",
  ".claude/",
];

/** Repo-root files that select nothing (documentation and repo furniture). */
export const NO_SCOPE_ROOT_EXTENSIONS = [".md"];

/** JS lockfiles / workspace manifests: both JS surfaces, no cargo. */
export const JS_WORKSPACE_PATHS = new Set([
  "pnpm-lock.yaml",
  "pnpm-workspace.yaml",
  "package.json",
]);

/**
 * `personas/**` is data that a Rust test asserts over: `crates/beekeeper-persona/
 * tests/pack_rules.rs` reads the pack files byte-for-byte, so a persona edit
 * can turn that crate red. Named here rather than left unmapped so the floor
 * runs the one crate that covers it instead of everything.
 */
export const PATH_PACKAGE_ROOTS = [
  { prefix: "personas/", package: "buzz-persona" },
];

/**
 * Build the package graph two `cargo metadata --no-deps` documents describe.
 *
 * @param {Array<{workspace: string, metadata: object}>} documents
 *   Each entry is one workspace: `workspace` is the label the floor uses to
 *   pick a `--manifest-path`, `metadata` is the parsed `cargo metadata
 *   --no-deps --format-version 1` output for it.
 * @returns {{packages: Record<string, {workspace: string, dir: string}>,
 *            dependents: Record<string, string[]>}}
 *   `packages` maps a package name to the workspace it lives in and its
 *   manifest directory relative to the repository root (as given). `dependents`
 *   maps a package name to the transitive closure of packages that depend on
 *   it, across every workspace in `documents`.
 */
export function buildGraph(documents) {
  const packages = {};
  const directDependents = new Map();

  for (const { workspace, metadata, root } of documents) {
    const repoRoot = root ?? "";
    for (const pkg of metadata.packages ?? []) {
      let dir = (pkg.manifest_path ?? "").replace(/\/Cargo\.toml$/, "");
      if (repoRoot && dir.startsWith(`${repoRoot}/`)) {
        dir = dir.slice(repoRoot.length + 1);
      }
      packages[pkg.name] = { workspace, dir };
      for (const dep of pkg.dependencies ?? []) {
        if (!directDependents.has(dep.name))
          directDependents.set(dep.name, new Set());
        directDependents.get(dep.name).add(pkg.name);
      }
    }
  }

  const dependents = {};
  for (const name of Object.keys(packages)) {
    const seen = new Set();
    const queue = [name];
    while (queue.length > 0) {
      const current = queue.pop();
      for (const parent of directDependents.get(current) ?? []) {
        // Only workspace members can be built with `-p`; a third-party crate
        // that happens to share a name is not ours to lint.
        if (!(parent in packages)) continue;
        if (parent === name || seen.has(parent)) continue;
        seen.add(parent);
        queue.push(parent);
      }
    }
    dependents[name] = [...seen].sort();
  }

  return { packages, dependents };
}

/**
 * Map a changed-file set onto the floor's scopes.
 *
 * @param {string[]} paths Repository-relative changed paths.
 * @param {{packages: Record<string, {workspace: string, dir: string}>,
 *          dependents: Record<string, string[]>}} graph From {@link buildGraph}.
 * @returns {{full: boolean, fullReasons: string[], changedPackages: string[],
 *            lintPackages: string[], desktop: boolean, web: boolean,
 *            mobile: boolean, unmapped: string[], noScope: string[]}}
 *   `changedPackages` are the packages whose own sources moved (fmt + tests
 *   run over these). `lintPackages` adds their transitive dependents (clippy
 *   runs over these, because a changed crate breaks its dependents' build, not
 *   its own). `full` means every scope is selected; `fullReasons` says why, one
 *   sentence per cause, and is never empty when `full` is true.
 */
export function deriveScope(paths, graph) {
  const packages = graph?.packages ?? {};
  const dependents = graph?.dependents ?? {};

  // Longest manifest directory first, so `desktop/src-tauri/crates/beekeeper-terminal`
  // wins over `desktop/src-tauri` for a file inside it.
  const packageDirs = Object.entries(packages)
    .filter(([, meta]) => meta.dir)
    .map(([name, meta]) => ({ name, dir: meta.dir }))
    .sort((a, b) => b.dir.length - a.dir.length);

  const scope = {
    full: false,
    fullReasons: [],
    changedPackages: [],
    lintPackages: [],
    desktop: false,
    web: false,
    mobile: false,
    unmapped: [],
    noScope: [],
  };

  const changed = new Set();
  const addFullReason = (reason) => {
    scope.full = true;
    if (!scope.fullReasons.includes(reason)) scope.fullReasons.push(reason);
  };

  for (const raw of paths) {
    const path = String(raw).trim();
    if (path === "") continue;

    if (FULL_FLOOR_EXACT.has(path)) {
      addFullReason(`${path} changes how everything builds`);
      continue;
    }
    if (FULL_FLOOR_PREFIXES.some((prefix) => path.startsWith(prefix))) {
      addFullReason(`${path} is build or gate tooling`);
      continue;
    }
    if (JS_WORKSPACE_PATHS.has(path)) {
      scope.desktop = true;
      scope.web = true;
      continue;
    }

    const owner = packageDirs.find(
      (candidate) =>
        path === candidate.dir || path.startsWith(`${candidate.dir}/`),
    );
    if (owner) {
      changed.add(owner.name);
      continue;
    }

    const declared = PATH_PACKAGE_ROOTS.find((entry) =>
      path.startsWith(entry.prefix),
    );
    if (declared) {
      if (declared.package in packages) {
        changed.add(declared.package);
      } else {
        addFullReason(
          `${path} maps to package ${declared.package}, which this graph does not know`,
        );
      }
      continue;
    }

    if (path.startsWith("desktop/")) {
      scope.desktop = true;
      continue;
    }
    if (path.startsWith("web/")) {
      scope.web = true;
      continue;
    }
    if (path.startsWith("mobile/")) {
      scope.mobile = true;
      continue;
    }

    if (NO_SCOPE_PREFIXES.some((prefix) => path.startsWith(prefix))) {
      scope.noScope.push(path);
      continue;
    }
    if (
      !path.includes("/") &&
      NO_SCOPE_ROOT_EXTENSIONS.some((extension) => path.endsWith(extension))
    ) {
      scope.noScope.push(path);
      continue;
    }

    scope.unmapped.push(path);
    addFullReason(`${path} maps to no scope, so the full floor runs`);
  }

  if (scope.full) {
    scope.changedPackages = Object.keys(packages).sort();
    scope.lintPackages = scope.changedPackages;
    scope.desktop = true;
    scope.web = true;
    scope.mobile = true;
    return scope;
  }

  const lint = new Set(changed);
  for (const name of changed) {
    for (const dependent of dependents[name] ?? []) lint.add(dependent);
  }
  scope.changedPackages = [...changed].sort();
  scope.lintPackages = [...lint].sort();
  return scope;
}

/**
 * Group package names by the workspace they belong to, preserving order.
 *
 * @param {string[]} names Package names.
 * @param {{packages: Record<string, {workspace: string, dir: string}>}} graph
 * @returns {Record<string, string[]>} workspace label -> package names.
 */
export function byWorkspace(names, graph) {
  const grouped = {};
  for (const name of names) {
    const workspace = graph?.packages?.[name]?.workspace ?? "root";
    if (!grouped[workspace]) grouped[workspace] = [];
    grouped[workspace].push(name);
  }
  return grouped;
}

/**
 * Desktop scope beyond the coarse `desktop: true` boolean: which existing
 * test files a push under `desktop/src/**` should actually run.
 *
 * Every push used to run the ENTIRE desktop suite (`just desktop-test`,
 * 7588 tests / 81 suites) for any `desktop/**` change. On a shared host that
 * alone measured 989-1410s with nowhere near the 120s budget (batch 3 L14,
 * fix round 1, 2026-09-02) — the suite itself, not the floor's own overhead,
 * is what blew the budget. A small source change does not warrant the whole
 * suite, so the floor now runs only the tests a changed file could plausibly
 * break: the test file itself when the change IS a test, or the sibling test
 * living beside a changed source file (same directory, same basename prefix,
 * `.test.mjs` — the convention every one of the 751 existing desktop test
 * files already follows, e.g. `AppShell.helpers.ts` / `AppShell.helpers.test.mjs`).
 * The full suite moves to exactly where the rest of this lane already put
 * `cargo test --workspace`: CI only.
 *
 * A source file with no sibling test is not silently skipped: it is named in
 * `untested`, the same way an unmapped path is named for the repo-wide scope.
 * A change under `desktop/` but OUTSIDE `desktop/src/**` (build config,
 * tooling, `desktop/package.json`, …) can change how EVERY test runs, so it
 * is not covered by the sibling rule at all — it forces the full desktop
 * suite, the same "no narrower scope is honest" direction as every other full
 * -floor case in this file.
 *
 * `existsFn` is injected so this stays a pure function under test: real
 * callers pass a real filesystem check, tests pass a fake set membership
 * check — no disk, no fixtures.
 *
 * @param {string[]} paths Repository-relative changed paths.
 * @param {(path: string) => boolean} existsFn Given a repo-relative path,
 *   true if that file exists in the working tree.
 * @returns {{full: boolean, fullReasons: string[], testFiles: string[],
 *            untested: string[]}}
 *   `testFiles` are repo-relative paths (`desktop/src/...`), deduplicated and
 *   sorted — every file `node --test` should actually run. `untested` are
 *   changed source files with no sibling test, named for disclosure. `full`
 *   means a desktop change fell outside `desktop/src/**` and the whole
 *   desktop suite must run instead; `fullReasons` says which path did it.
 */
export function desktopTestPlan(paths, existsFn) {
  const testFiles = new Set();
  const untested = [];
  let full = false;
  const fullReasons = [];

  for (const raw of paths) {
    const path = String(raw).trim();
    if (path === "" || !path.startsWith("desktop/")) continue;
    if (path.startsWith("desktop/src-tauri/")) continue; // Rust, not TS

    if (!path.startsWith("desktop/src/")) {
      full = true;
      const reason = `${path} is desktop build/tooling config, not covered by the sibling-test rule`;
      if (!fullReasons.includes(reason)) fullReasons.push(reason);
      continue;
    }

    if (path.endsWith(".test.mjs")) {
      testFiles.add(path);
      continue;
    }

    const sibling = path.replace(/\.[^./]+$/, "") + ".test.mjs";
    if (existsFn(sibling)) {
      testFiles.add(sibling);
    } else {
      untested.push(path);
    }
  }

  return {
    full,
    fullReasons,
    testFiles: [...testFiles].sort(),
    untested: [...new Set(untested)].sort(),
  };
}

// ── CLI entry: the only impure part ──────────────────────────────────────────
// Reads changed paths on stdin (one per line) and prints the scope as JSON.
// The graph comes from `cargo metadata --no-deps` unless
// BUZZ_PRE_PUSH_FLOOR_GRAPH names a JSON file holding one, which is how
// `scripts/test-pre-push-floor.sh` keeps its cargo stub honest about how many
// times the floor really invokes cargo.
async function main() {
  const { readFileSync, realpathSync, existsSync } = await import("node:fs");
  const { execFileSync } = await import("node:child_process");
  const { join } = await import("node:path");

  const stdin = readFileSync(0, "utf8");
  const paths = stdin.split("\n").filter((line) => line.trim() !== "");

  let graph;
  const injected = process.env.BUZZ_PRE_PUSH_FLOOR_GRAPH;
  if (injected) {
    graph = JSON.parse(readFileSync(injected, "utf8"));
  } else {
    const root = realpathSync(process.cwd());
    const metadata = (manifest) =>
      JSON.parse(
        execFileSync(
          "cargo",
          [
            "metadata",
            "--no-deps",
            "--format-version",
            "1",
            ...(manifest ? ["--manifest-path", manifest] : []),
          ],
          { encoding: "utf8", maxBuffer: 64 * 1024 * 1024 },
        ),
      );
    graph = buildGraph([
      { workspace: "root", root, metadata: metadata(null) },
      {
        workspace: "tauri",
        root,
        metadata: metadata("desktop/src-tauri/Cargo.toml"),
      },
    ]);
  }

  const scope = deriveScope(paths, graph);
  scope.workspaces = {
    changed: byWorkspace(scope.changedPackages, graph),
    lint: byWorkspace(scope.lintPackages, graph),
  };
  // Real disk check, scoped to cwd (the repo root — or the stub suite's
  // scratch repo, which is why `test-pre-push-floor.sh` can exercise this
  // with real seeded files rather than a fake).
  scope.desktopTests = desktopTestPlan(paths, (relative) =>
    existsSync(join(process.cwd(), relative)),
  );

  if (process.argv.includes("--shell")) {
    process.stdout.write(toShell(scope));
    return;
  }
  process.stdout.write(`${JSON.stringify(scope, null, 2)}\n`);
}

/**
 * Render a scope as `KEY='value'` lines for a shell to `eval`.
 *
 * Single-quoted with the usual `'\''` escape, so a path containing a quote
 * cannot break out into a command.
 *
 * @param {ReturnType<typeof deriveScope> & {workspaces: object}} scope
 * @returns {string}
 */
export function toShell(scope) {
  const quote = (value) => `'${String(value).replaceAll("'", "'\\''")}'`;
  const list = (values) => quote((values ?? []).join(" "));
  return [
    `FLOOR_FULL=${scope.full ? 1 : 0}`,
    `FLOOR_FULL_REASONS=${quote((scope.fullReasons ?? []).join("; "))}`,
    `FLOOR_CHANGED_ROOT=${list(scope.workspaces?.changed?.root)}`,
    `FLOOR_CHANGED_TAURI=${list(scope.workspaces?.changed?.tauri)}`,
    `FLOOR_LINT_ROOT=${list(scope.workspaces?.lint?.root)}`,
    `FLOOR_LINT_TAURI=${list(scope.workspaces?.lint?.tauri)}`,
    `FLOOR_DESKTOP=${scope.desktop ? 1 : 0}`,
    `FLOOR_WEB=${scope.web ? 1 : 0}`,
    `FLOOR_MOBILE=${scope.mobile ? 1 : 0}`,
    `FLOOR_UNMAPPED=${list(scope.unmapped)}`,
    // Sibling-scoped desktop test plan (desktopTestPlan): FULL means a
    // desktop/** change fell outside desktop/src/**, so the whole desktop
    // suite runs rather than a narrower, dishonest guess.
    `FLOOR_DESKTOP_TEST_FULL=${scope.desktopTests?.full ? 1 : 0}`,
    `FLOOR_DESKTOP_TEST_FULL_REASONS=${quote((scope.desktopTests?.fullReasons ?? []).join("; "))}`,
    `FLOOR_DESKTOP_TEST_FILES=${list(scope.desktopTests?.testFiles)}`,
    `FLOOR_DESKTOP_UNTESTED=${list(scope.desktopTests?.untested)}`,
    "",
  ].join("\n");
}

// `realpathSync` rather than a raw string compare: on macOS $TMPDIR is
// /var/folders/… symlinked to /private/var/folders/…, so `import.meta.url` is
// the resolved path while `process.argv[1]` is the one typed. Comparing them
// unresolved made this module import cleanly and then do nothing at all — a
// silent no-op, which for a gate is the worst possible failure.
if (process.argv[1]) {
  const { realpathSync } = await import("node:fs");
  const { pathToFileURL } = await import("node:url");
  if (import.meta.url === pathToFileURL(realpathSync(process.argv[1])).href) {
    await main();
  }
}
