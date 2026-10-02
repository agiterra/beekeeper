import assert from "node:assert/strict";
import { test } from "node:test";

import {
  nextPinCreatedAt,
  pinTargetKindOf,
  rankAt,
} from "@/features/agents-repo/lib/artifactPinMutations";

const row = (target, rank, pinned = true) => ({
  target,
  targetKind: "file",
  pinned,
  rank,
  by: "a".repeat(64),
  updatedAt: 1,
});

test("a file and a folder are told apart by their own shapes", () => {
  assert.deepEqual(pinTargetKindOf("docs/notes/a.md"), {
    ok: true,
    kind: "file",
  });
  assert.deepEqual(pinTargetKindOf("plans/CURRENT_STATE.md"), {
    ok: true,
    kind: "file",
  });
  assert.deepEqual(pinTargetKindOf("docs/mockups"), {
    ok: true,
    kind: "folder",
  });
});

test("a keep is never the pinned thing — its folder is", () => {
  const refused = pinTargetKindOf("docs/mockups/.gitkeep");
  assert.equal(refused.ok, false);
  assert.match(refused.error, /pin the folder/);
});

test("the one shape both grammars admit refuses instead of guessing", () => {
  // A folder name may contain a dot, so this is a legal folder *and* a
  // refused file. Guessing "folder" would pin a row that names nothing and
  // never tell the person who typed a filename why.
  const refused = pinTargetKindOf("docs/notes.txt");
  assert.equal(refused.ok, false);
  assert.match(refused.error, /say which you mean/);
  assert.match(refused.error, /As a file:/);
  // Dotless is unambiguous.
  assert.deepEqual(pinTargetKindOf("docs/v2"), { ok: true, kind: "folder" });
});

test("a first pin takes the first rank and later ones append", () => {
  const first = rankAt([], "docs/a.md");
  assert.equal(first, "a0");
  const appended = rankAt([row("docs/a.md", "a0")], "docs/b.md");
  assert.ok(appended > "a0", appended);
});

test("an index places a pin between its new neighbours", () => {
  const three = [
    row("docs/a.md", "a0"),
    row("docs/b.md", "a1"),
    row("docs/c.md", "a2"),
  ];
  const top = rankAt(three, "docs/d.md", 0);
  assert.ok(top < "a0", top);
  const middle = rankAt(three, "docs/d.md", 1);
  assert.ok(middle > "a0" && middle < "a1", middle);
  // Past the end is the end: a caller asking for "last" should not have to
  // count the rows first.
  const last = rankAt(three, "docs/d.md", 99);
  assert.ok(last > "a2", last);
});

test("moving a pin never measures against its own row", () => {
  // `b` moving to the top must land before `a`. If its own row counted as a
  // neighbour the rank would be computed between `a` and `b` and the row
  // would not move at all — which looks exactly like a working drag.
  const three = [
    row("docs/a.md", "a0"),
    row("docs/b.md", "a1"),
    row("docs/c.md", "a2"),
  ];
  assert.ok(rankAt(three, "docs/b.md", 0) < "a0");
  assert.ok(rankAt(three, "docs/b.md", 2) > "a2");
});

test("an unpinned row is not a neighbour", () => {
  const mixed = [
    row("docs/a.md", "a0"),
    row("docs/gone.md", "a1", false),
    row("docs/c.md", "a2"),
  ];
  const middle = rankAt(mixed, "docs/d.md", 1);
  assert.ok(middle > "a0" && middle < "a2", middle);
});

test("a write is stamped past the latest op on its target, never behind now", () => {
  assert.equal(nextPinCreatedAt(0, 1_000), 1_000);
  // A peer stamped the target ahead of this clock: the write still wins the
  // field rather than losing to a skew.
  assert.equal(nextPinCreatedAt(1_500, 1_000), 1_501);
});
