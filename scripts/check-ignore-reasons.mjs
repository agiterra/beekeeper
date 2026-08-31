#!/usr/bin/env node
/**
 * Ratchet on bare `#[ignore]` attributes.
 *
 * An ignored test is invisible: it does not run, and nothing in a run summary
 * says why. With a reason string the gap is legible and greppable — you can ask
 * "what would Postgres buy me?" and get an answer. `#[ignore]` on its own only
 * says someone once decided not to run it.
 *
 * The repo already leans on reason strings heavily: 300+ tests say
 * `#[ignore = "requires Postgres"]`, and `Justfile`'s `test-genesis` selects
 * ignored tests by name precisely because they are labelled. This guard stops
 * the unlabelled population growing while the existing ones are worked down.
 *
 * It is a ratchet, not a wall: lower BASELINE when you add reasons. Never raise
 * it — add the reason instead. Same rule as the file-size ratchet.
 */

import { readdirSync, readFileSync, statSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

// Bare `#[ignore]` count at the time this guard landed. Ratchet DOWN only.
const BASELINE = 253;

const repoRoot = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const roots = ["crates", "desktop/src-tauri/src"].map((r) => join(repoRoot, r));

const hits = [];
const walk = (dir) => {
  for (const entry of readdirSync(dir)) {
    if (entry === "target" || entry === "node_modules" || entry === ".git") continue;
    const full = join(dir, entry);
    const st = statSync(full);
    if (st.isDirectory()) {
      walk(full);
    } else if (entry.endsWith(".rs")) {
      const lines = readFileSync(full, "utf8").split("\n");
      lines.forEach((line, i) => {
        if (line.trim() === "#[ignore]") {
          hits.push(`${full.slice(repoRoot.length + 1)}:${i + 1}`);
        }
      });
    }
  }
};
for (const r of roots) walk(r);

const count = hits.length;

if (count > BASELINE) {
  console.error(
    `\ncheck-ignore-reasons: ${count} bare #[ignore] attributes, baseline is ${BASELINE}.`,
  );
  console.error(
    "\nGive the new one a reason — `#[ignore = \"requires Postgres\"]`, `\"requires Redis\"`,",
  );
  console.error(
    "`\"requires running relay\"` — matching the vocabulary already in use. Do not raise",
  );
  console.error("the baseline.\n");
  process.exit(1);
}

if (count < BASELINE) {
  console.error(
    `\ncheck-ignore-reasons: ${count} bare #[ignore] — below the ${BASELINE} baseline. Nice.`,
  );
  console.error(
    `Lower BASELINE in scripts/check-ignore-reasons.mjs to ${count} to hold the ground.\n`,
  );
  process.exit(1);
}

console.error(`check-ignore-reasons: ok — ${count} bare #[ignore], at baseline.`);
