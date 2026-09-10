import assert from "node:assert/strict";
import test from "node:test";

import {
  clearCodingSessionFoundedDraft,
  codingSessionFoundedDraftLegacyStorageKey,
  codingSessionFoundedDraftStorageKey,
  readCodingSessionFoundedDraft,
  writeCodingSessionFoundedDraft,
} from "./codingSessionFoundedDraft.ts";

const SESSION_REF = "11111111-1111-4111-8111-111111111111";
const OTHER_REF = "22222222-2222-4222-8222-222222222222";

const DRAFT = {
  name: "Ledger 103",
  workdir: "/Users/andy/Code/beekeeper",
  useWorktree: true,
  worktreeName: "ledger-103",
  worktreeSource: "main",
  rememberWorkspace: true,
  repoRef: "30617:owner:repo",
  workspaceSourcePath: null,
  workspaceSourceBranch: null,
  workspaceSourceBranchSource: null,
};

const REUSE_DRAFT = {
  name: null,
  workdir: "/Users/x/Code/repo-wt-a",
  useWorktree: false,
  worktreeName: null,
  worktreeSource: null,
  rememberWorkspace: false,
  repoRef: "30617:owner:repo",
  workspaceSourcePath: "/Users/x/Code/repo-wt-a",
  workspaceSourceBranch: "wt-a",
  workspaceSourceBranchSource: "recorded",
};

const V1_DRAFT = {
  workdir: "/Users/andy/Code/beekeeper",
  useWorktree: true,
  worktreeName: "ledger-103",
  worktreeSource: "main",
};

function v1Record(sessionRef, draft = V1_DRAFT) {
  return JSON.stringify({
    schema: "buzz-coding-session-founded-draft/v1",
    sessionRef,
    draft,
  });
}

function memoryStorage() {
  const values = new Map();
  return {
    getItem: (key) => values.get(key) ?? null,
    setItem: (key, value) => values.set(key, value),
    removeItem: (key) => values.delete(key),
    values,
  };
}

test("a v2 draft round-trips per session ref and clears explicitly", () => {
  const storage = memoryStorage();
  assert.equal(
    writeCodingSessionFoundedDraft(SESSION_REF, DRAFT, storage),
    true,
  );
  assert.equal(
    writeCodingSessionFoundedDraft(OTHER_REF, REUSE_DRAFT, storage),
    true,
  );
  assert.deepEqual(readCodingSessionFoundedDraft(SESSION_REF, storage), DRAFT);
  assert.deepEqual(
    readCodingSessionFoundedDraft(OTHER_REF, storage),
    REUSE_DRAFT,
  );
  clearCodingSessionFoundedDraft(SESSION_REF, storage);
  assert.equal(readCodingSessionFoundedDraft(SESSION_REF, storage), null);
  assert.notEqual(readCodingSessionFoundedDraft(OTHER_REF, storage), null);
  // Nothing written for a ref reads as nothing, not as an empty draft.
  assert.equal(readCodingSessionFoundedDraft("never-founded", storage), null);
});

test("the written record carries exactly the ten v2 fields, whatever else was passed", () => {
  const storage = memoryStorage();
  writeCodingSessionFoundedDraft(
    SESSION_REF,
    { ...DRAFT, extra: "a field the founded page never asked for" },
    storage,
  );
  const stored = JSON.parse(
    storage.getItem(codingSessionFoundedDraftStorageKey(SESSION_REF)),
  );
  assert.deepEqual(Object.keys(stored.draft).sort(), [
    "name",
    "rememberWorkspace",
    "repoRef",
    "useWorktree",
    "workdir",
    "workspaceSourceBranch",
    "workspaceSourceBranchSource",
    "workspaceSourcePath",
    "worktreeName",
    "worktreeSource",
  ]);
  assert.equal(stored.sessionRef, SESSION_REF);
  assert.match(stored.schema, /founded-draft\/v2$/);
  assert.match(
    codingSessionFoundedDraftStorageKey(SESSION_REF),
    /\.v2:11111111/,
  );
  // A live branch source is never written, even when a caller hands one over.
  writeCodingSessionFoundedDraft(
    OTHER_REF,
    { ...REUSE_DRAFT, workspaceSourceBranchSource: "live" },
    storage,
  );
  assert.equal(
    readCodingSessionFoundedDraft(OTHER_REF, storage)
      .workspaceSourceBranchSource,
    null,
  );
});

test("a v1 record is read when no v2 record exists, upgraded with the ordinary defaults", () => {
  const storage = memoryStorage();
  storage.setItem(
    codingSessionFoundedDraftLegacyStorageKey(SESSION_REF),
    v1Record(SESSION_REF),
  );
  assert.deepEqual(readCodingSessionFoundedDraft(SESSION_REF, storage), {
    name: null,
    ...V1_DRAFT,
    rememberWorkspace: true,
    repoRef: null,
    workspaceSourcePath: null,
    workspaceSourceBranch: null,
    workspaceSourceBranchSource: null,
  });
  // A v2 record wins over a v1 one under the same ref.
  writeCodingSessionFoundedDraft(SESSION_REF, REUSE_DRAFT, storage);
  assert.deepEqual(
    readCodingSessionFoundedDraft(SESSION_REF, storage),
    REUSE_DRAFT,
  );
});

test("a v1 record is parsed exactly: four keys, or nothing", () => {
  const storage = memoryStorage();
  const key = codingSessionFoundedDraftLegacyStorageKey(SESSION_REF);
  for (const draft of [
    { ...V1_DRAFT, extra: true },
    { workdir: "/x", useWorktree: true },
    { ...V1_DRAFT, useWorktree: "yes" },
    { ...V1_DRAFT, workdir: 42 },
    // A v2-shaped draft under the v1 key and schema is not a v1 record.
    DRAFT,
  ]) {
    storage.setItem(key, v1Record(SESSION_REF, draft));
    assert.equal(
      readCodingSessionFoundedDraft(SESSION_REF, storage),
      null,
      JSON.stringify(draft),
    );
  }
  // …and a v1 record for another ref is foreign.
  storage.setItem(key, v1Record(OTHER_REF));
  assert.equal(readCodingSessionFoundedDraft(SESSION_REF, storage), null);
});

test("clear removes both the v2 and the v1 record", () => {
  const storage = memoryStorage();
  writeCodingSessionFoundedDraft(SESSION_REF, DRAFT, storage);
  storage.setItem(
    codingSessionFoundedDraftLegacyStorageKey(SESSION_REF),
    v1Record(SESSION_REF),
  );
  clearCodingSessionFoundedDraft(SESSION_REF, storage);
  assert.equal(readCodingSessionFoundedDraft(SESSION_REF, storage), null);
  assert.equal(
    storage.getItem(codingSessionFoundedDraftStorageKey(SESSION_REF)),
    null,
  );
  assert.equal(
    storage.getItem(codingSessionFoundedDraftLegacyStorageKey(SESSION_REF)),
    null,
  );
});

test("the v2 reader fails closed on a foreign ref, a foreign schema, or malformed data", () => {
  const storage = memoryStorage();
  const key = codingSessionFoundedDraftStorageKey(SESSION_REF);
  const record = (overrides) =>
    JSON.stringify({
      schema: "buzz-coding-session-founded-draft/v2",
      sessionRef: SESSION_REF,
      draft: DRAFT,
      ...overrides,
    });
  const cases = [
    "{bad",
    "null",
    "[]",
    record({ schema: "something-else/v2" }),
    record({ schema: "buzz-coding-session-founded-draft/v1" }),
    record({ sessionRef: OTHER_REF }),
    record({ draft: undefined }),
    record({ draft: { ...DRAFT, useWorktree: "yes" } }),
    record({ draft: { ...DRAFT, workdir: 42 } }),
    record({ draft: { ...DRAFT, name: 42 } }),
    record({ draft: { ...DRAFT, rememberWorkspace: "no" } }),
    record({ draft: { ...DRAFT, repoRef: 7 } }),
    record({ draft: { ...DRAFT, workspaceSourceBranchSource: "live" } }),
    record({ draft: V1_DRAFT }),
    record({ draft: { ...DRAFT, extra: true } }),
  ];
  for (const value of cases) {
    storage.setItem(key, value);
    assert.equal(
      readCodingSessionFoundedDraft(SESSION_REF, storage),
      null,
      `must read as nothing: ${value}`,
    );
  }
});

test("unavailable storage reads as nothing and writes as a no-op, without throwing", () => {
  const blocked = {
    getItem() {
      throw new Error("SecurityError");
    },
    setItem() {
      throw new Error("QuotaExceededError");
    },
    removeItem() {
      throw new Error("SecurityError");
    },
  };
  assert.equal(readCodingSessionFoundedDraft(SESSION_REF, blocked), null);
  assert.equal(
    writeCodingSessionFoundedDraft(SESSION_REF, DRAFT, blocked),
    false,
  );
  assert.doesNotThrow(() =>
    clearCodingSessionFoundedDraft(SESSION_REF, blocked),
  );
});

test("no storage at all is the same as blocked storage", () => {
  // In this test process there is no `localStorage` on globalThis, so the
  // default resolution finds nothing — the founded page then shows the
  // project default and says so, rather than guessing.
  assert.equal(typeof globalThis.localStorage, "undefined");
  assert.equal(readCodingSessionFoundedDraft(SESSION_REF), null);
  assert.equal(writeCodingSessionFoundedDraft(SESSION_REF, DRAFT), false);
  assert.doesNotThrow(() => clearCodingSessionFoundedDraft(SESSION_REF));
});
