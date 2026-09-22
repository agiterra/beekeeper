// Contract tests for scripts/mobile-test-failure-summary.mjs.
//
// Run: node --test scripts/mobile-test-failure-summary.test.mjs

import assert from "node:assert/strict";
import test from "node:test";

import { extractFailures, formatSummary } from "./mobile-test-failure-summary.mjs";

// Captured verbatim from a real `flutter test --reporter expanded` run
// against a two-test file (one passing, one failing) — see item 208's
// investigation notes in plans/SESSION_STATE.md.
const SAMPLE = `00:00 +0: loading /repo/mobile/test/sample_test.dart
00:00 +0: a passing test
00:00 +1: a failing test
00:00 +1 -1: a failing test [E]
  Expected: <2>
    Actual: <1>

  package:matcher                                     expect
  package:flutter_test/src/widget_tester.dart 473:18  expect
  test/sample_test.dart 8:5                   main.<fn>

00:00 +1 -1: Some tests failed.
`;

test("pulls the failing test's name and its own file:line, not a library frame", () => {
  const failures = extractFailures(SAMPLE);
  assert.equal(failures.length, 1);
  assert.equal(failures[0].name, "a failing test");
  assert.equal(failures[0].location, "test/sample_test.dart:8:5");
});

test("formats as a short FAILED: line", () => {
  const lines = formatSummary(extractFailures(SAMPLE));
  assert.deepEqual(lines, [
    "FAILED: a failing test (test/sample_test.dart:8:5)",
  ]);
});

test("an all-passing run reports no failures", () => {
  const passing = `00:00 +0: loading /repo/mobile/test/sample_test.dart
00:00 +0: a passing test
00:00 +1: All tests passed!
`;
  assert.deepEqual(extractFailures(passing), []);
  assert.deepEqual(formatSummary(extractFailures(passing)), []);
});

test("multiple failures in one run are each reported", () => {
  const multi = `00:00 +0: loading /repo/mobile/test/sample_test.dart
00:00 +0 -1: first failing test [E]
  Expected: <2>
    Actual: <1>

  package:matcher                                     expect
  test/sample_test.dart 8:5                   main.<fn>

00:00 +0 -2: second failing test [E]
  Expected: <4>
    Actual: <3>

  package:matcher                                     expect
  test/other_test.dart 20:9                   main.<fn>

00:00 +0 -2: Some tests failed.
`;
  const lines = formatSummary(extractFailures(multi));
  assert.deepEqual(lines, [
    "FAILED: first failing test (test/sample_test.dart:8:5)",
    "FAILED: second failing test (test/other_test.dart:20:9)",
  ]);
});

test("a failure with no recognisable stack location still names the test", () => {
  const noLocation = `00:00 +0 -1: a weird failure [E]
  something threw

00:00 +0 -1: Some tests failed.
`;
  assert.deepEqual(formatSummary(extractFailures(noLocation)), [
    "FAILED: a weird failure",
  ]);
});
