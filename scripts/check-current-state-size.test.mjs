import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import test from "node:test";
import { fileURLToPath } from "node:url";
import {
  evaluateCurrentState,
  MAP_PATH,
  MAX_BYTES,
  MAX_LINES,
  measure,
} from "./check-current-state-size.mjs";

const repoRoot = resolve(dirname(fileURLToPath(import.meta.url)), "..");

test("lines count the way wc -l does, bytes as UTF-8", () => {
  assert.deepEqual(measure(""), { lines: 0, bytes: 0 });
  assert.deepEqual(measure("a\nb\n"), { lines: 2, bytes: 4 });
  assert.deepEqual(measure("a\nb"), { lines: 2, bytes: 3 });
  // One em dash is three UTF-8 bytes; a character count would under-report it.
  assert.equal(measure("—\n").bytes, 4);
});

test("a file under both ceilings passes", () => {
  const body = `${"line\n".repeat(10)}`;
  assert.deepEqual(
    evaluateCurrentState(body, { maxLines: 10, maxBytes: 50 }).violations,
    [],
  );
});

test("a line ceiling alone would admit enormous paragraphs; bytes catch them", () => {
  const body = `${"x".repeat(5_000)}\n`.repeat(5);
  const result = evaluateCurrentState(body, {
    maxLines: 300,
    maxBytes: 24_000,
  });
  assert.equal(result.lines, 5);
  assert.deepEqual(result.violations, ["25005 bytes (max 24000)"]);
});

test("many short lines trip the line ceiling even when bytes are fine", () => {
  const body = "x\n".repeat(301);
  const result = evaluateCurrentState(body, {
    maxLines: 300,
    maxBytes: 24_000,
  });
  assert.deepEqual(result.violations, ["301 lines (max 300)"]);
});

test("both measures are reported when both are exceeded", () => {
  const body = `${"x".repeat(100)}\n`.repeat(301);
  const result = evaluateCurrentState(body, {
    maxLines: 300,
    maxBytes: 24_000,
  });
  assert.equal(result.violations.length, 2);
});

test("the shipped ceilings are the agreed ones", () => {
  assert.equal(MAX_LINES, 300);
  assert.equal(MAX_BYTES, 24_000);
});

test("the checked-in map is under its ceiling", () => {
  const content = readFileSync(join(repoRoot, MAP_PATH), "utf8");
  const result = evaluateCurrentState(content);
  assert.deepEqual(
    result.violations,
    [],
    `${MAP_PATH} is ${result.lines} lines and ${result.bytes} bytes`,
  );
});
