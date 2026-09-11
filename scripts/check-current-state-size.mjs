#!/usr/bin/env node
/**
 * Hard ceiling on `docs/CURRENT_STATE.md`, the current-state map every agent
 * reads first and whole.
 *
 * The map is useful only while it is small. Its predecessor as the first read,
 * `docs/SESSION_STATE.md`, carried two written promises that it would shrink
 * and grew from 152 lines to 12,000+ in three weeks anyway (+3,140 / −216
 * lines over its last sixty commits). A prose ceiling does not hold; this one
 * is mechanical.
 *
 * Two measures, because a line limit alone admits enormous paragraphs. Bytes
 * are UTF-8. And it is a wall, not a ratchet: the repository's file-size
 * ratchet lets a file that is already oversize keep its size, which is exactly
 * the failure this guard exists to prevent.
 */

import { readFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

export const MAP_PATH = "docs/CURRENT_STATE.md";
export const MAX_LINES = 300;
export const MAX_BYTES = 24_000;

/** Lines the way `wc -l` counts them; bytes as UTF-8. */
export function measure(content) {
  if (content.length === 0) {
    return { lines: 0, bytes: 0 };
  }
  const lines = content.split("\n").length - (content.endsWith("\n") ? 1 : 0);
  return { lines, bytes: Buffer.byteLength(content, "utf8") };
}

/**
 * Pure verdict for one file body. `violations` is empty when the file is under
 * both ceilings; otherwise one sentence per exceeded measure.
 */
export function evaluateCurrentState(content, limits = {}) {
  const maxLines = limits.maxLines ?? MAX_LINES;
  const maxBytes = limits.maxBytes ?? MAX_BYTES;
  const { lines, bytes } = measure(content);
  const violations = [];
  if (lines > maxLines) {
    violations.push(`${lines} lines (max ${maxLines})`);
  }
  if (bytes > maxBytes) {
    violations.push(`${bytes} bytes (max ${maxBytes})`);
  }
  return { lines, bytes, maxLines, maxBytes, violations };
}

function main() {
  const repoRoot = resolve(dirname(fileURLToPath(import.meta.url)), "..");
  const content = readFileSync(join(repoRoot, MAP_PATH), "utf8");
  const result = evaluateCurrentState(content);
  if (result.violations.length > 0) {
    console.error(
      `${MAP_PATH} is over its ceiling: ${result.violations.join(", ")}. ` +
        "It is the map every agent reads whole. Move detail into the plan it " +
        "links, a numbered ledger item, or docs/history/; do not raise the limit.",
    );
    process.exit(1);
  }
  console.log(
    `${MAP_PATH}: ${result.lines}/${result.maxLines} lines, ${result.bytes}/${result.maxBytes} bytes`,
  );
}

if (
  process.argv[1] &&
  resolve(process.argv[1]) === fileURLToPath(import.meta.url)
) {
  main();
}
