#!/usr/bin/env node
/**
 * Every `just` recipe that starts a relay runs the first-party plugins.
 *
 * Planning is a plugin, so a relay started without `BEEKEEPER_PLUGINS_FILE`
 * serves no Planning at all. `just relay` set it; `just dev`, `relay-web`,
 * `admin` and `relay-release` did not, so the desktop's own dev loop ran a
 * relay with no Planning while the relay image and `just relay` had it.
 *
 * A recipe that runs `cargo run -p beekeeper-relay` (not an `--example`) or a
 * built `target/<profile>/beekeeper-relay` must depend on `plugins-build` and
 * call `beekeeper_default_plugins_file` (scripts/lib/plugins.sh) after it
 * loads `.env`, so an operator's own registry still wins. Recipes that start
 * test relays through scripts set the registry themselves and are not
 * matched here.
 *
 * Usage: node scripts/check-relay-recipes-plugins.mjs [--justfile <path>]
 */

import { execFileSync } from "node:child_process";

const args = process.argv.slice(2);
const justArgs = ["--dump", "--dump-format", "json"];
const at = args.indexOf("--justfile");
if (at !== -1) justArgs.unshift("--justfile", args[at + 1]);

const dump = JSON.parse(execFileSync("just", justArgs, { encoding: "utf8" }));

// A recipe body is a list of lines, each a list of text fragments and
// interpolations; only the text matters here.
const lineText = (line) =>
  line.map((part) => (typeof part === "string" ? part : "{{}}")).join("");

const startsRelay = (text) =>
  (/\bcargo run\b.*-p beekeeper-relay\b/.test(text) &&
    !/--example\b/.test(text)) ||
  /target\/(debug|release)\/beekeeper-relay\b/.test(text);

const problems = [];
const matched = [];
for (const [name, recipe] of Object.entries(dump.recipes)) {
  const lines = recipe.body
    .map(lineText)
    .filter((l) => !l.trim().startsWith("#"));
  const relayAt = lines.findIndex(startsRelay);
  if (relayAt === -1) continue;
  matched.push(name);
  if (!recipe.dependencies.some((d) => d.recipe === "plugins-build")) {
    problems.push(
      `${name}: starts a relay but does not depend on plugins-build`,
    );
  }
  const helperAt = lines.findIndex((l) =>
    l.includes("beekeeper_default_plugins_file"),
  );
  const envAt = lines.findIndex((l) => /^\s*source \.env\b/.test(l));
  if (helperAt === -1 || helperAt > relayAt) {
    problems.push(
      `${name}: starts a relay without calling beekeeper_default_plugins_file first`,
    );
  } else if (envAt !== -1 && helperAt < envAt) {
    problems.push(
      `${name}: sets the plugin default before loading .env, so .env cannot override it`,
    );
  }
}

// The matcher itself can rot: five recipes start a relay today.
if (matched.length < 5) {
  problems.push(
    `only ${matched.length} relay-starting recipe(s) matched (${matched.join(", ")}); expected at least 5 — has the matcher gone stale?`,
  );
}

if (problems.length) {
  for (const p of problems) console.error(`relay-plugins-check: ${p}`);
  process.exit(1);
}
console.log(
  `relay-plugins-check: ${matched.length} relay recipes run the first-party plugins (${matched.join(", ")})`,
);
