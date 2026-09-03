import assert from "node:assert/strict";
import { before, test } from "node:test";

import {
  installSeatWipHooks,
  resolveWorktreeSourceSelection,
  SEAT_KEY_IS_NOT_ON_DISK,
  seatKeyfilePath,
} from "./codingSessionWorktreeSource.ts";

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

// The seat-hook install site. F2: `install_coding_session_seat_hooks` had no
// live caller, so no seat got a hook, so no seat pushed a wip ref. What is
// guarded here is that a hire really does reach the installer — and that the
// one thing this host cannot supply, the seat's key file, is named rather than
// guessed at.

const ipcCalls = [];
const tauriInternals = {
  invoke(command, args) {
    ipcCalls.push({ command, args });
    return Promise.resolve({
      wipRef: "refs/heads/wip/refuter/0f1e2d3c",
      hooksDir: "/tmp/seat/.git/hooks",
      hooksWritten: ["post-commit", "prepare-commit-msg"],
      configScope: "worktree",
      dispatch: "worktree",
      changed: true,
      signing: "unsigned",
    });
  },
  transformCallback: () => Math.random(),
};

before(() => {
  globalThis.window = { __TAURI_INTERNALS__: tauriInternals };
  globalThis.__TAURI_INTERNALS__ = tauriInternals;
});

test("this host names no key file for a seat, and says why in words", () => {
  const source = seatKeyfilePath("a".repeat(64));
  assert.equal(source.kind, "none");
  assert.equal(source.code, SEAT_KEY_IS_NOT_ON_DISK);
  assert.match(source.why, /NOSTR_PRIVATE_KEY/);
});

test("a hire installs the hooks, naming the worktree and the seat", async () => {
  ipcCalls.length = 0;
  const outcome = await installSeatWipHooks(
    { path: "/tmp/seat", branch: "lane/refuter" },
    {
      actor: "a".repeat(64),
      role: "refuter",
      sessionRef: "session-1",
      genesisRef: "genesis-1",
      channelId: "c0ffee",
    },
    "0".repeat(64),
  );

  assert.equal(ipcCalls.length, 1);
  assert.equal(ipcCalls[0].command, "install_coding_session_seat_hooks");
  assert.deepEqual(ipcCalls[0].args.request, {
    worktreePath: "/tmp/seat",
    seatRole: "refuter",
    seatPubkey: "a".repeat(64),
    keyfilePath: null,
    signerProgram: "git-sign-nostr",
    assignmentId: "0".repeat(64),
    sessionRef: "session-1",
    genesisRef: "genesis-1",
    channelId: "c0ffee",
    branch: "lane/refuter",
  });
  assert.equal(outcome.kind, "installed");
  assert.equal(outcome.installed.wipRef, "refs/heads/wip/refuter/0f1e2d3c");
  assert.equal(outcome.signing.kind, "none");
  assert.equal(outcome.signing.code, SEAT_KEY_IS_NOT_ON_DISK);
});

test("an install that throws is a named failure, never a silent null", async () => {
  ipcCalls.length = 0;
  const broken = {
    invoke: () => Promise.reject(new Error("no such worktree")),
    transformCallback: () => Math.random(),
  };
  globalThis.window = { __TAURI_INTERNALS__: broken };
  globalThis.__TAURI_INTERNALS__ = broken;
  try {
    const outcome = await installSeatWipHooks(
      { path: "/tmp/seat", branch: "lane/refuter" },
      {
        actor: "a".repeat(64),
        role: "refuter",
        sessionRef: "session-1",
        genesisRef: "genesis-1",
        channelId: "c0ffee",
      },
      null,
    );
    assert.equal(outcome.kind, "failed");
    assert.match(outcome.why, /no such worktree/);
  } finally {
    globalThis.window = { __TAURI_INTERNALS__: tauriInternals };
    globalThis.__TAURI_INTERNALS__ = tauriInternals;
  }
});
