import assert from "node:assert/strict";
import test from "node:test";

import { resolveWorktreeSourceSelection } from "./codingSessionWorktreeSource.ts";

// What the source picker lands on decides which branch every new session
// descends from. The bug this guards against: a checkout parked on an old
// topic branch becoming the silent ancestor of a "new" session.

test("no branches means nothing to select", () => {
  assert.equal(
    resolveWorktreeSourceSelection({ branches: null, current: null }),
    null,
  );
  assert.equal(
    resolveWorktreeSourceSelection({
      branches: { branches: [], defaultBranch: null, headBranch: null },
      current: "main",
    }),
    null,
  );
});

test("the trunk wins by default, even when the checkout is parked elsewhere", () => {
  assert.equal(
    resolveWorktreeSourceSelection({
      branches: {
        branches: ["old-topic", "main"],
        defaultBranch: "main",
        headBranch: "old-topic",
      },
      current: null,
    }),
    "main",
  );
});

test("a choice that still exists is kept", () => {
  assert.equal(
    resolveWorktreeSourceSelection({
      branches: {
        branches: ["main", "feature-x"],
        defaultBranch: "main",
        headBranch: "main",
      },
      current: "feature-x",
    }),
    "feature-x",
  );
});

test("a choice that vanished falls back to the default", () => {
  assert.equal(
    resolveWorktreeSourceSelection({
      branches: {
        branches: ["main", "feature-x"],
        defaultBranch: "main",
        headBranch: "main",
      },
      current: "deleted-branch",
    }),
    "main",
  );
});

test("without a trunk, the checkout's own branch stands in", () => {
  assert.equal(
    resolveWorktreeSourceSelection({
      branches: {
        branches: ["newest", "trunk"],
        defaultBranch: null,
        headBranch: "trunk",
      },
      current: null,
    }),
    "trunk",
  );
});

test("detached without a trunk, the most recent branch stands in", () => {
  assert.equal(
    resolveWorktreeSourceSelection({
      branches: {
        branches: ["newest", "older"],
        defaultBranch: null,
        headBranch: null,
      },
      current: null,
    }),
    "newest",
  );
});
