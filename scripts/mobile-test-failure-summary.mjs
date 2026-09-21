// Turns `flutter test --reporter expanded` output into a short "FAILED:"
// list with file:line, for scripts/pre-push-floor.sh's mobile step.
//
// Why this exists (item 208). `flutter test`'s default reporter renders as
// "compact" with carriage-return redraws whenever a pseudo-terminal is
// attached, so a failing test's name scrolls away in agent/CI capture and
// only the exit code survives. `--reporter expanded` (wired in the justfile's
// `mobile-test` recipe) prints one line per test plus the failure block
// line-oriented, no `\r` redraws — this module reads that text and pulls out
// just the failing test names and their `test/…dart:line:col` location, the
// way `run_cargo_test` already counts passed tests out of `cargo test`
// output for the same reason: a human or an agent scanning the floor's
// output should not have to re-run the suite locally to learn what broke.
//
// Run: node --test scripts/mobile-test-failure-summary.test.mjs

const FAILURE_START = /^\d\d:\d\d(?:\s[+-]\d+)*:\s(.+?)\s\[E\]$/;
const TEST_EVENT_LINE = /^\d\d:\d\d/;
// A `package:foo/bar.dart 12:3  someFrame` stack line is matcher/widget_tester
// plumbing; the negative lookahead skips those and keeps the first frame that
// names a project file instead — the actual call site of the assertion.
const LOCATION_LINE = /^(?!package:)(\S+\.dart)\s+(\d+):(\d+)\b/;

/**
 * @param {string} text `flutter test --reporter expanded` stdout+stderr.
 * @returns {{name: string, location: string | null}[]}
 */
export function extractFailures(text) {
  const lines = text.split(/\r?\n/);
  const failures = [];
  let current = null;

  for (const line of lines) {
    const start = line.match(FAILURE_START);
    if (start) {
      if (current) failures.push(current);
      current = { name: start[1], location: null };
      continue;
    }
    if (!current) continue;
    // Any other `NN:NN …` line (the next test, or the trailing "Some tests
    // failed." summary) closes the block this failure's stack trace lives in.
    if (TEST_EVENT_LINE.test(line) && !start) {
      failures.push(current);
      current = null;
      continue;
    }
    if (!current.location) {
      const loc = line.trim().match(LOCATION_LINE);
      if (loc) current.location = `${loc[1]}:${loc[2]}:${loc[3]}`;
    }
  }
  if (current) failures.push(current);
  return failures;
}

/** @param {{name: string, location: string | null}[]} failures */
export function formatSummary(failures) {
  return failures.map(
    (f) => `FAILED: ${f.name}${f.location ? ` (${f.location})` : ""}`,
  );
}

async function main() {
  const { readFileSync } = await import("node:fs");
  const path = process.argv[2];
  const text = path ? readFileSync(path, "utf8") : readFileSync(0, "utf8");
  const lines = formatSummary(extractFailures(text));
  for (const line of lines) console.log(line);
}

if (import.meta.url === `file://${process.argv[1]}`) {
  main();
}
