#!/usr/bin/env node
/**
 * Guard against Playwright config/disk drift.
 *
 * `playwright.config.ts` enumerates every spec by name in each project's
 * `testMatch`. Playwright neither warns about a glob that matches no file nor
 * about a spec file that no project claims, so both directions rot in silence:
 * at the time this check was written the config listed two specs that had been
 * deleted (`workspace-rail`, `tokens`) and two specs sat on disk in no project
 * at all, unreachable by `pnpm test:e2e` — five passing tests nobody could run.
 *
 * Two rules, both cheap:
 *
 *   1. Every `testMatch` entry must match at least one file on disk.
 *   2. Every `*.spec.ts` under `tests/e2e/` must be claimed by some project.
 *
 * `*.perf.ts` files are deliberately exempt from rule 2: they are manual
 * benchmarks with multi-minute timeouts, opted in one at a time, not gates.
 *
 * Both `playwright.config.ts` and `playwright.release-smoke.config.ts` count as
 * registration — they draw from the same `tests/e2e` directory.
 */

import { readdirSync, readFileSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const here = dirname(fileURLToPath(import.meta.url));
const desktop = resolve(here, "..");
// Both configs share the one `tests/e2e` directory, so a spec is "registered"
// if either claims it. Omitting the release-smoke config here would report its
// three specs as unreachable when they are simply run by a different recipe
// (`just desktop-release-smoke`).
const configPaths = [
  resolve(desktop, "playwright.config.ts"),
  resolve(desktop, "playwright.release-smoke.config.ts"),
];
const e2eDir = resolve(desktop, "tests/e2e");

const config = configPaths.map((p) => readFileSync(p, "utf8")).join("\n");

// Every `"**/<name>"` string inside a testMatch array. The config only ever
// writes them in that one shape, so a regex is enough and keeps this check
// free of a TypeScript loader.
const entries = [...config.matchAll(/"\*\*\/([^"]+)"/g)].map((m) => m[1]);
if (entries.length === 0) {
  console.error(
    "check-e2e-registration: no testMatch entries found — has the config format changed?",
  );
  process.exit(1);
}

const onDisk = readdirSync(e2eDir).filter((f) => f.endsWith(".ts"));
const registered = new Set(entries);

const dead = entries.filter((e) => !onDisk.includes(e));
const unclaimed = onDisk.filter(
  (f) => f.endsWith(".spec.ts") && !registered.has(f),
);

let failed = false;

if (dead.length > 0) {
  failed = true;
  console.error(
    `\ncheck-e2e-registration: ${dead.length} testMatch entr${dead.length === 1 ? "y matches" : "ies match"} no file on disk.`,
  );
  console.error(
    "Delete them from playwright.config.ts, or restore the spec:\n",
  );
  for (const d of dead) console.error(`  ${d}`);
}

if (unclaimed.length > 0) {
  failed = true;
  console.error(
    `\ncheck-e2e-registration: ${unclaimed.length} spec file${unclaimed.length === 1 ? "" : "s"} registered in no project — \`pnpm test:e2e\` cannot reach ${unclaimed.length === 1 ? "it" : "them"}.`,
  );
  console.error(
    "Add to a project's testMatch in playwright.config.ts, or delete the file:\n",
  );
  for (const u of unclaimed) console.error(`  tests/e2e/${u}`);
}

if (failed) {
  console.error("");
  process.exit(1);
}

console.error(
  `check-e2e-registration: ok — ${entries.length} entries, ${onDisk.filter((f) => f.endsWith(".spec.ts")).length} specs, no drift.`,
);
