// Contract tests for scripts/mobile-test-failure-summary.mjs.
//
// Run: node --test scripts/mobile-test-failure-summary.test.mjs

import assert from "node:assert/strict";
import test from "node:test";

import {
  extractFailures,
  extractLoadFailures,
  formatSummary,
} from "./mobile-test-failure-summary.mjs";

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

// Shape seen 2026-10-07 when a foreign client took flutter_tester's harness
// socket: the file never ran, so it is a load failure, not a test failure.
const LOAD_FAILED = `00:00 +0: loading /repo/mobile/test/a_test.dart
00:00 +0 -1: loading /repo/mobile/test/a_test.dart [E]
  Failed to load "/repo/mobile/test/a_test.dart": Unable to connect to flutter_tester process: WebSocketException: Invalid WebSocket upgrade request
00:01 +3 -1: Some tests failed.
`;

test("names each file whose harness never attached, once", () => {
  assert.deepEqual(extractLoadFailures(LOAD_FAILED + LOAD_FAILED), [
    "/repo/mobile/test/a_test.dart",
  ]);
});

test("a real assertion failure is never mistaken for a load failure", () => {
  assert.deepEqual(extractLoadFailures(SAMPLE), []);
});
