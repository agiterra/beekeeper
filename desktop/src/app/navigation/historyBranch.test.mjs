import assert from "node:assert/strict";
import test from "node:test";

import { resolveHistoryBranch } from "./historyBranch.ts";

test("a push onto a used index truncates what was ahead", () => {
  // Went back to 2, then navigated somewhere new: index 3 already had a key,
  // and it is not this one.
  assert.deepEqual(
    resolveHistoryBranch({
      previousIndex: 2,
      index: 3,
      storedKey: "old",
      key: "new",
      maxIndex: 5,
    }),
    { maxIndex: 3, truncateAbove: true },
  );
});

test("a push onto a fresh index keeps the high-water mark honest", () => {
  assert.deepEqual(
    resolveHistoryBranch({
      previousIndex: 2,
      index: 3,
      storedKey: undefined,
      key: "new",
      maxIndex: 2,
    }),
    { maxIndex: 3, truncateAbove: false },
  );
});

test("going forward into history we still hold keeps everything ahead", () => {
  assert.deepEqual(
    resolveHistoryBranch({
      previousIndex: 3,
      index: 4,
      storedKey: "known",
      key: "known",
      maxIndex: 6,
    }),
    { maxIndex: 6, truncateAbove: false },
  );
});

// This is the bug the module exists for: picking a person in the New message
// screen rewrites `/messages/new` into the DM with `{ replace: true }`, which
// mints a new key at the same index. Reading that as a new branch greyed out
// the forward arrow over history that was still there.
test("a replace in place keeps the forward stack", () => {
  assert.deepEqual(
    resolveHistoryBranch({
      previousIndex: 3,
      index: 3,
      storedKey: "before-replace",
      key: "after-replace",
      maxIndex: 6,
    }),
    { maxIndex: 6, truncateAbove: false },
  );
});

test("going back destroys nothing", () => {
  assert.deepEqual(
    resolveHistoryBranch({
      previousIndex: 4,
      index: 2,
      storedKey: "known",
      key: "known",
      maxIndex: 6,
    }),
    { maxIndex: 6, truncateAbove: false },
  );
});

test("the first observation records where we are and claims nothing ahead", () => {
  assert.deepEqual(
    resolveHistoryBranch({
      previousIndex: null,
      index: 3,
      storedKey: undefined,
      key: "first",
      maxIndex: 3,
    }),
    { maxIndex: 3, truncateAbove: false },
  );
});

test("a reload onto a deeper index than we knew raises the mark", () => {
  assert.deepEqual(
    resolveHistoryBranch({
      previousIndex: null,
      index: 7,
      storedKey: undefined,
      key: "restored",
      maxIndex: 0,
    }),
    { maxIndex: 7, truncateAbove: false },
  );
});
