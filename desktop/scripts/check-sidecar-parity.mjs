#!/usr/bin/env node
/**
 * The sidecar set is declared in several places. This asserts they are one set.
 *
 * `tauri.conf.json`'s `externalBin` is what the bundle *declares*; the refresh
 * recipes are what actually gets built and copied next to the app. When those
 * drift, nothing fails — the app simply runs an old binary, and the drift is
 * invisible until someone dates the files by hand. That happened on
 * 2026-09-05: `buzz-shell-host` beside the app was dated Aug 22 while the
 * other seven were 10:17, and nobody could say whether that meant "unchanged"
 * or "missed" (PLAN-2026-09-05 §1). A stale sidecar should fail a check, not a
 * morning.
 *
 * The expected set is `externalBin` plus `buzz-session-provider`. The provider
 * is deliberately not in the base config: `tauri.local-prod.conf.json` is the
 * tracked delta that adds it for bundles that carry it, and that file is
 * checked here too.
 *
 * Each refresh path is compared as the union of what it *builds* and what it
 * *copies*, because the two legitimately differ: `just desktop-standalone`
 * builds the provider (the supervisor spawns it out of `target/debug`) without
 * copying it into `binaries/`, since `tauri dev` does not bundle. A name that
 * appears in neither list is the failure this catches.
 *
 * The three artifacts added on 2026-09-30 are the ones that had already
 * drifted, unwatched, and broke every macOS and Linux release and canary
 * build: `scripts/bundle-sidecars.sh` required a `buzz-shell-host` that not
 * one of the eight CI "Build sidecars" steps built, and no root-workspace
 * crate depended on it either (ledger 295). The two locally-exercised paths
 * were correct, which is exactly why nobody hit it — the checked lists were
 * right and the unchecked ones were wrong. Adding artifacts to a set of
 * already-drifting unchecked lists without closing them is how the next
 * stale-binary morning happens.
 *
 * Two sidecars are **Unix-only** and this asserts that too, in both
 * directions: `buzz-shell-host` and `beekeeper-host` both exit non-zero off Unix
 * (their `main.rs`), so `tauri.windows.conf.json` must not declare them and
 * the Windows build lines must not build them. A Windows bundle that declared
 * one would fail Tauri's compile-time `externalBin` check with a message
 * naming Tauri rather than the missing name.
 *
 * Run: node desktop/scripts/check-sidecar-parity.mjs  (also `pnpm check:sidecars`)
 */

import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const REPO = join(dirname(fileURLToPath(import.meta.url)), "..", "..");

/**
 * Cargo package -> the binary it produces, where they differ. The packages
 * are named `beekeeper-*` but the binaries keep their deployed `buzz-*` names
 * (and `beekeeper-cli` builds `bee`); a package not listed here names its own
 * binary (e.g. `beekeeper-host`).
 */
const BINARY_OF = {
  "beekeeper-acp": "buzz-acp",
  "beekeeper-agent": "buzz-agent",
  "beekeeper-backend-kubernetes": "buzz-backend-kubernetes",
  "beekeeper-cli": "bee",
  "beekeeper-dev-mcp": "buzz-dev-mcp",
  "beekeeper-session-provider": "buzz-session-provider",
  "beekeeper-shell-host": "buzz-shell-host",
};
const binaryOf = (pkg) => BINARY_OF[pkg] ?? pkg;

/** The one sidecar the base bundle config deliberately does not declare. */
const PROVIDER = "buzz-session-provider";

/**
 * Sidecars the Windows bundle does not carry.
 *
 * `buzz-shell-host` and `beekeeper-host` refuse to run off Unix (their `main.rs`),
 * so shipping them there would ship a binary that exits 1.
 * `buzz-backend-kubernetes` is a different case with the same answer: it has
 * always been gated out of `bundle-sidecars.sh` and has never been declared in
 * `tauri.windows.conf.json`. Adding this check is what surfaced that it was
 * only ever an unwritten convention.
 *
 * Declaring one of these in the Windows bundle would fail Tauri's
 * compile-time `externalBin` check with a message naming Tauri rather than the
 * missing name.
 */
const UNIX_ONLY = [
  "buzz-backend-kubernetes",
  "buzz-shell-host",
  "beekeeper-host",
];

const read = (rel) => readFileSync(join(REPO, rel), "utf8");
const sorted = (names) => [...new Set(names)].sort();

/** `externalBin` entries are `binaries/<name>`; the leading directory goes. */
function externalBinNames(configPath) {
  const config = JSON.parse(read(configPath));
  const list = config?.bundle?.externalBin;
  if (!Array.isArray(list)) {
    fail(`${configPath}: no bundle.externalBin array`);
  }
  return sorted(list.map((entry) => entry.split("/").pop()));
}

const failures = [];
function fail(message) {
  failures.push(message);
}

/**
 * One `just` recipe's body: from its name at column 0 to the next line that
 * starts at column 0. Recipe bodies are indented, so this needs no parser.
 */
function justRecipe(text, name) {
  const lines = text.split("\n");
  const start = lines.findIndex((line) =>
    new RegExp(`^${name}(\\s|:)`).test(line),
  );
  if (start === -1) fail(`justfile: no recipe named ${name}`);
  const body = [];
  for (const line of lines.slice(start + 1)) {
    if (line.trim() !== "" && !/^\s/.test(line)) break;
    body.push(line);
  }
  return body.join("\n");
}

/** Every `-p <package>` in a chunk of shell. */
const cargoPackages = (text) =>
  [...text.matchAll(/-p\s+([\w-]+)/g)].map((match) => match[1]);

/** The names a `for bin in a b c; do` loop iterates. */
const forLoopNames = (text) =>
  [...text.matchAll(/for\s+bin\s+in\s+([^;]+);/g)].flatMap((match) =>
    match[1]
      .split(/\s+/)
      .filter((word) => /^[\w-]+$/.test(word) && !word.startsWith("$")),
  );

/** A bash array literal's elements: `NAME=( a b c )`. */
function bashArray(text, name) {
  const match = text.match(new RegExp(`${name}=\\(([^)]*)\\)`));
  if (!match) fail(`scripts/app-from.sh: no ${name} array`);
  return (match?.[1] ?? "")
    .split(/\s+/)
    .filter((word) => /^[\w-]+$/.test(word));
}

function compare(label, actual, expected) {
  const missing = expected.filter((name) => !actual.includes(name));
  const extra = actual.filter((name) => !expected.includes(name));
  if (missing.length === 0 && extra.length === 0) return true;
  fail(
    `${label}\n    missing: ${missing.join(", ") || "(none)"}\n    unexpected: ${
      extra.join(", ") || "(none)"
    }`,
  );
  return false;
}

const declared = externalBinNames("desktop/src-tauri/tauri.conf.json");
const expected = sorted([...declared, PROVIDER]);

// 1. The tracked delta config that bundles carrying the provider use.
compare(
  "desktop/src-tauri/tauri.local-prod.conf.json externalBin != tauri.conf.json externalBin + the provider:",
  externalBinNames("desktop/src-tauri/tauri.local-prod.conf.json"),
  expected,
);

// 2. `just desktop-standalone` — builds all eight, copies the seven the dev
//    run needs beside the exe.
// `Justfile`, capitalised — the tracked name. macOS would resolve either, and
// Linux CI would resolve only this one.
const justfile = read("Justfile");
const standalone = justRecipe(justfile, "desktop-standalone");
compare(
  "just desktop-standalone builds/copies a different sidecar set than the bundle declares:",
  sorted([
    ...cargoPackages(standalone).map(binaryOf),
    ...forLoopNames(standalone),
  ]),
  expected,
);

// 3. `scripts/app-from.sh` — the bundle path; it must build *and* copy all
//    eight, since a missing one only shows up as a broken installed app.
const appFrom = read("scripts/app-from.sh");
const appFromPackages = sorted(
  bashArray(appFrom, "SIDECAR_PACKAGES").map(binaryOf),
);
const appFromBinaries = sorted(bashArray(appFrom, "SIDECAR_BINARIES"));
compare(
  "scripts/app-from.sh SIDECAR_PACKAGES does not cover the declared sidecars:",
  appFromPackages,
  expected,
);
compare(
  "scripts/app-from.sh SIDECAR_BINARIES does not cover the declared sidecars:",
  appFromBinaries,
  expected,
);
compare(
  "scripts/app-from.sh builds packages it does not copy (or the reverse):",
  appFromBinaries,
  appFromPackages,
);

// 4. `_ensure-sidecar-stubs` gates compilation: Tauri validates `externalBin`
//    at compile time, so a declared binary with no stub fails the build with a
//    message that names Tauri rather than the missing name.
const stubs = justRecipe(justfile, "_ensure-sidecar-stubs");
const stubbed = sorted([
  ...(stubs.match(/SIDECARS=\(([^)]*)\)/)?.[1] ?? "")
    .split(/\s+/)
    .filter((word) => /^[\w-]+$/.test(word)),
  ...[...stubs.matchAll(/SIDECARS\+=\(([^)]*)\)/g)].flatMap((match) =>
    match[1].split(/\s+/).filter((word) => /^[\w-]+$/.test(word)),
  ),
]);
compare(
  "just _ensure-sidecar-stubs does not stub every declared externalBin:",
  stubbed,
  declared, // the provider is not in the base config, so it needs no stub
);

// 5. `scripts/bundle-sidecars.sh` — the script every CI "Build sidecars" step
//    runs. It hard-fails when a listed binary is missing from `target/`, so
//    its list is the one that decides whether a release builds at all.
const bundleScript = read("scripts/bundle-sidecars.sh");
const bundleUnix = sorted([
  ...(bundleScript.match(/^SIDECARS=\(([^)]*)\)/m)?.[1] ?? "")
    .split(/\s+/)
    .filter((word) => /^[\w-]+$/.test(word)),
  ...[...bundleScript.matchAll(/SIDECARS\+=\(([^)]*)\)/g)].flatMap((match) =>
    match[1].split(/\s+/).filter((word) => /^[\w-]+$/.test(word)),
  ),
]);
compare(
  "scripts/bundle-sidecars.sh copies a different set than the bundle declares:",
  bundleUnix,
  declared,
);

// 6. The Windows bundle. Its `externalBin` is the declared set minus the
//    Unix-only binaries — asserted from both sides, so a Unix-only sidecar
//    cannot be declared there and a portable one cannot be forgotten.
compare(
  "desktop/src-tauri/tauri.windows.conf.json externalBin != the declared set minus the Unix-only sidecars:",
  externalBinNames("desktop/src-tauri/tauri.windows.conf.json"),
  sorted(declared.filter((name) => !UNIX_ONLY.includes(name))),
);

// 7. Every CI step that runs `bundle-sidecars.sh` must first build what that
//    script requires. This is the check whose absence broke every macOS and
//    Linux build: the script's list and the `cargo build` line above it were
//    free to disagree, and did.
const CI_WORKFLOWS = [
  ".github/workflows/linux-canary.yml",
  ".github/workflows/macos-intel-canary.yml",
  ".github/workflows/windows-canary.yml",
];
const packageOf = Object.fromEntries(
  Object.entries(BINARY_OF).map(([pkg, bin]) => [bin, pkg]),
);
/** The cargo packages that produce a set of binary names. */
const packagesFor = (names) =>
  sorted(names.map((name) => packageOf[name] ?? name));

let ciSteps = 0;
for (const workflow of CI_WORKFLOWS) {
  const text = read(workflow);
  const lines = text.split("\n");
  lines.forEach((line, index) => {
    if (!line.includes("bundle-sidecars.sh")) return;
    ciSteps += 1;
    // The `cargo build` that feeds it is the nearest one above, inside the
    // same step. Ten lines is generous for the shapes in these workflows and
    // narrow enough not to reach into a neighbouring step.
    const build = lines
      .slice(Math.max(0, index - 10), index)
      .reverse()
      .find((candidate) => candidate.includes("cargo build --release"));
    const where = `${workflow}:${index + 1}`;
    if (!build) {
      fail(
        `${where}: runs bundle-sidecars.sh with no 'cargo build --release' above it`,
      );
      return;
    }
    // Which platform this step runs on comes from its **job**, not from the
    // build line: a Windows job can take its target from an env var, so its
    // `cargo build` reads identically to the macOS one. The nearest
    // `runs-on:` above is the job's.
    const runsOn = lines
      .slice(0, index)
      .reverse()
      .find((candidate) => /^\s*runs-on:/.test(candidate));
    if (!runsOn) {
      fail(
        `${where}: no 'runs-on:' above it, so its platform cannot be determined`,
      );
      return;
    }
    const windows = /windows/i.test(runsOn);
    const wanted = packagesFor(
      windows ? declared.filter((name) => !UNIX_ONLY.includes(name)) : declared,
    );
    const built = sorted(cargoPackages(build));
    const missing = wanted.filter((pkg) => !built.includes(pkg));
    if (missing.length > 0) {
      fail(
        `${where}: bundle-sidecars.sh will require ${missing.join(", ")}, which the build above it does not produce`,
      );
    }
  });
}
// A guard on the guard: if a workflow stops calling the script, or the call
// moves out of this checker's reach, the loop above would pass by finding
// nothing to check.
if (ciSteps < 3) {
  fail(
    `expected at least 3 CI steps calling bundle-sidecars.sh, found ${ciSteps} — a step was removed or renamed, and this check silently stopped covering it`,
  );
}

if (failures.length > 0) {
  console.error(
    `Sidecar parity failed. Expected set (externalBin + ${PROVIDER}):\n  ${expected.join(
      ", ",
    )}\n`,
  );
  for (const failure of failures) console.error(`  ${failure}`);
  process.exit(1);
}
console.log(
  `Sidecar parity OK — ${expected.length} sidecars: ${expected.join(", ")}`,
);
