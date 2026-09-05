#!/usr/bin/env node
/**
 * The sidecar set is declared in four places. This asserts they are one set.
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
 * Run: node desktop/scripts/check-sidecar-parity.mjs  (also `pnpm check:sidecars`)
 */

import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const REPO = join(dirname(fileURLToPath(import.meta.url)), "..", "..");

/**
 * Cargo package -> the binary it produces, where they differ. `buzz-cli`
 * builds `bee`; every other sidecar package names its own binary.
 */
const BINARY_OF = { "buzz-cli": "bee" };
const binaryOf = (pkg) => BINARY_OF[pkg] ?? pkg;

/** The one sidecar the base bundle config deliberately does not declare. */
const PROVIDER = "buzz-session-provider";

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
