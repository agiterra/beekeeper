/**
 * The close dialog's worktree section. The rules under test are the ones that
 * decide whether a person's uncommitted work survives: a held row never offers
 * to be swept, and an unmeasurable directory never reports as zero.
 */
import assert from "node:assert/strict";
import { test } from "node:test";

import {
  formatReclaimableBytes,
  offersPruneControl,
  pruneHeldConfirmCopy,
  summarizeWorktreeClosure,
} from "./codingSessionWorktreeReclaim.ts";

const SESSION = "11111111-1111-4111-8111-111111111111";

function row(overrides = {}) {
  return {
    key: `${SESSION}/builder-1`,
    sessionRef: SESSION,
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

test("a clean row renders the host's sentence and offers no prune button", () => {
  const summary = summarizeWorktreeClosure([row()]);
  assert.equal(summary.lines.length, 1);
  assert.equal(
    summary.lines[0].detail,
    "/src/proj.worktrees/lane: clean, will be removed",
  );
  assert.equal(summary.lines[0].offersPrune, false);
  assert.equal(summary.heldCount, 0);
});

test("a held row says held: N uncommitted files and is the only one with a prune button", () => {
  const held = row({
    disposition: "held",
    dirtyFiles: 3,
    detail: "held: 3 uncommitted files",
  });
  const summary = summarizeWorktreeClosure([row(), held]);

  assert.deepEqual(
    summary.lines.map((line) => line.offersPrune),
    [false, true],
  );
  assert.equal(summary.heldCount, 1);
  assert.equal(summary.heldFiles, 3);
  assert.equal(summary.lines[1].detail, "held: 3 uncommitted files");
  assert.equal(offersPruneControl(held), true);
});

test("no other disposition ever offers a prune button", () => {
  for (const disposition of [
    "prunable",
    "not-settled",
    "tip-not-on-relay",
    "execution-live",
    "unrecorded",
    "protected",
    "within-grace",
  ]) {
    assert.equal(
      offersPruneControl(row({ disposition, dirtyFiles: 5 })),
      false,
      `${disposition} must not offer to remove a directory`,
    );
  }
});

test("a held row whose directory is already gone offers nothing", () => {
  assert.equal(
    offersPruneControl(
      row({ disposition: "held", dirtyFiles: 2, exists: false }),
    ),
    false,
  );
});

test("unrecorded and unconfirmed-tip rows carry a reason and no action", () => {
  const summary = summarizeWorktreeClosure([
    row({
      disposition: "unrecorded",
      detail:
        "/src/proj.worktrees/old: this host never recorded cutting it, so it never removes it",
      reclaimableNow: false,
    }),
    row({
      disposition: "tip-not-on-relay",
      tipOnRelayKnown: false,
      detail:
        "/src/proj.worktrees/lane: this host could not confirm the branch is on the relay, so nothing is removed",
      reclaimableNow: false,
    }),
  ]);
  assert.deepEqual(
    summary.lines.map((line) => line.offersPrune),
    [false, false],
  );
  assert.deepEqual(
    summary.lines.map((line) => line.offersReclaim),
    [false, false],
  );
  assert.match(summary.lines[1].detail, /could not confirm/);
});

test("build output is offered on a held row too, because no commit lives there", () => {
  const summary = summarizeWorktreeClosure([
    row({
      disposition: "held",
      dirtyFiles: 4,
      detail: "held: 4 uncommitted files",
    }),
  ]);
  assert.equal(summary.lines[0].offersPrune, true);
  assert.equal(summary.lines[0].offersReclaim, true);
  assert.equal(summary.reclaimableLabel, "18.4 GB");
});

test("one unmeasurable directory makes the total unknown, never zero", () => {
  const summary = summarizeWorktreeClosure([
    row(),
    row({
      key: `${SESSION}/builder-2`,
      reclaimableBytes: null,
      reclaimableLabel: "unknown",
    }),
  ]);
  assert.equal(summary.reclaimableBytes, null);
  assert.equal(summary.reclaimableLabel, "unknown");
});

test("measurable rows add up to one number with one decimal", () => {
  const summary = summarizeWorktreeClosure([
    row({ reclaimableBytes: 18_400_000_000 }),
    row({ key: `${SESSION}/builder-2`, reclaimableBytes: 4_100_000_000 }),
  ]);
  assert.equal(summary.reclaimableBytes, 22_500_000_000);
  assert.equal(summary.reclaimableLabel, "22.5 GB");
});

test("formatReclaimableBytes matches the Rust renderer, including unknown", () => {
  assert.equal(formatReclaimableBytes(null), "unknown");
  assert.equal(formatReclaimableBytes(0), "0.0 GB");
  assert.equal(formatReclaimableBytes(298_000_000_000), "298.0 GB");
});

test("the prune confirm names the path and the count", () => {
  const [line] = summarizeWorktreeClosure([
    row({
      disposition: "held",
      dirtyFiles: 3,
      detail: "held: 3 uncommitted files",
    }),
  ]).lines;
  const copy = pruneHeldConfirmCopy(line);
  assert.match(copy, /\/src\/proj\.worktrees\/lane/);
  assert.match(copy, /3 uncommitted files/);
});

test("the confirm says one file, not 1 files", () => {
  const [line] = summarizeWorktreeClosure([
    row({
      disposition: "held",
      dirtyFiles: 1,
      detail: "held: 1 uncommitted files",
    }),
  ]).lines;
  assert.match(
    pruneHeldConfirmCopy(line),
    /1 uncommitted file\?|1 uncommitted file,/,
  );
});

test("an empty session contributes no lines and claims nothing", () => {
  const summary = summarizeWorktreeClosure([]);
  assert.deepEqual(summary.lines, []);
  assert.equal(summary.heldCount, 0);
  assert.equal(summary.offersReclaim, false);
  assert.equal(summary.reclaimableBytes, 0);
});
