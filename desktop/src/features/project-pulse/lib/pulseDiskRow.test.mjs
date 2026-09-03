/**
 * Pulse's disk row. Every number in the sentence has to be backed by a row
 * that produced it — a "2 held" with nothing named under it is the exact shape
 * of claim this project treats as a bug.
 */
import assert from "node:assert/strict";
import { test } from "node:test";

import { buildPulseDiskRow, formatDiskBytes } from "./pulseDiskRow.ts";

const SESSION_A = "11111111-1111-4111-8111-111111111111";
const SESSION_B = "22222222-2222-4222-8222-222222222222";

function row(overrides = {}) {
  return {
    key: `${SESSION_A}/builder-1`,
    sessionRef: SESSION_A,
    seatLabel: "builder-1",
    path: "/src/proj.worktrees/lane",
    branch: "lane",
    repoRoot: "/src/proj",
    disposition: "prunable",
    dirtyFiles: 0,
    reclaimableBytes: 18_400_000_000,
    reclaimableLabel: "18.4 GB",
    reclaimableNow: true,
    graceRemainingSecs: null,
    exists: true,
    tipOnRelayKnown: true,
    detail: "/src/proj.worktrees/lane: clean, will be removed",
    ...overrides,
  };
}

test("the row counts worktrees, measures gigabytes, and names every held tree", () => {
  const built = buildPulseDiskRow([
    row(),
    row({
      key: `${SESSION_B}/refuter-1`,
      sessionRef: SESSION_B,
      seatLabel: "refuter-1",
      path: "/src/proj.worktrees/lane-two",
      disposition: "held",
      dirtyFiles: 3,
      reclaimableBytes: 4_100_000_000,
      detail: "held: 3 uncommitted files",
    }),
  ]);

  assert.equal(built.line, "disk: 2 worktrees, 22.5 GB reclaimable, 1 held");
  assert.equal(built.worktrees, 2);
  assert.equal(built.reclaimableBytes, 22_500_000_000);
  assert.equal(built.held.length, 1);
  assert.equal(built.held[0].sessionRef, SESSION_B);
  assert.equal(built.held[0].dirtyFiles, 3);
  assert.equal(built.held[0].line, "refuter-1 · 3 uncommitted files");
});

test("every held count in the sentence is backed by a named entry", () => {
  const built = buildPulseDiskRow([
    row({
      disposition: "held",
      dirtyFiles: 2,
      detail: "held: 2 uncommitted files",
    }),
    row({
      key: `${SESSION_A}/builder-2`,
      seatLabel: "builder-2",
      disposition: "held",
      dirtyFiles: 9,
      detail: "held: 9 uncommitted files",
    }),
  ]);
  const stated = Number(built.line.match(/(\d+) held/)[1]);
  assert.equal(stated, built.held.length, "the number and the list must agree");
  assert.equal(
    built.held.every((entry) => entry.line.includes("uncommitted")),
    true,
  );
});

test("one unmeasurable directory makes the total unknown, never zero", () => {
  const built = buildPulseDiskRow([
    row(),
    row({ key: `${SESSION_A}/builder-2`, reclaimableBytes: null }),
  ]);
  assert.equal(built.reclaimableBytes, null);
  assert.equal(built.reclaimableLabel, "unknown");
  assert.match(built.line, /unknown reclaimable/);
});

test("unrecorded trees are counted apart and never folded into the removable total", () => {
  const built = buildPulseDiskRow([
    row(),
    row({
      key: `${SESSION_A}/found-1`,
      seatLabel: "found-1",
      disposition: "unrecorded",
      reclaimableNow: false,
      reclaimableBytes: 40_000_000_000,
      detail:
        "/src/proj.worktrees/old: this host never recorded cutting it, so it never removes it",
    }),
  ]);
  assert.equal(built.unrecorded, 1);
  assert.equal(
    built.reclaimableBytes,
    18_400_000_000,
    "an unrecorded tree's bytes are not offered as reclaimable",
  );
  assert.match(built.line, /1 unrecorded/);
});

test("with nothing recorded the row says so rather than printing zeroes", () => {
  const built = buildPulseDiskRow([]);
  assert.equal(built.empty, true);
  assert.equal(built.line, "disk: no worktrees recorded on this machine");
  assert.deepEqual(built.held, []);
});

test("one worktree and one file read in the singular", () => {
  const built = buildPulseDiskRow([
    row({
      disposition: "held",
      dirtyFiles: 1,
      detail: "held: 1 uncommitted files",
    }),
  ]);
  assert.match(built.line, /^disk: 1 worktree, /);
  assert.equal(built.held[0].line, "builder-1 · 1 uncommitted file");
});

test("formatDiskBytes matches the Rust renderer", () => {
  assert.equal(formatDiskBytes(null), "unknown");
  assert.equal(formatDiskBytes(0), "0.0 GB");
  assert.equal(formatDiskBytes(298_000_000_000), "298.0 GB");
});
