import assert from "node:assert/strict";
import test from "node:test";

import {
  decodeProjectPacksInitResult,
  defaultPacksRepoId,
  packsRepoIdError,
} from "./projectPacksInit.ts";

const RESULT = {
  repoRef: `30617:${"a".repeat(64)}:agiterra-packs`,
  sourceEventId: "b".repeat(64),
  seedCommitSha: "c".repeat(40),
  seedError: null,
  commitIdentityName: "Beekeeper aaaaaaaa",
  commitIdentityEmail: "aaaaaaaa@beekeeper.local",
  pushRecordEventId: "d".repeat(64),
  announcementWithdrawnEventId: null,
  announcementWithdrawalError: null,
};

/** What `project_packs_init` really answers with (`packs_repo.rs`). */
const HOST_RESULT = {
  ...RESULT,
  repoId: "agiterra-packs",
  cloneUrl: `https://hive.example/git/${"a".repeat(64)}/agiterra-packs`,
  announcementEventId: "e".repeat(64),
  branch: "main",
  roles: ["architect", "builder"],
  pushed: true,
  pushError: null,
  publicationError: null,
};

test("decodeProjectPacksInitResult accepts every key the panel prints", () => {
  assert.deepEqual(decodeProjectPacksInitResult(RESULT), RESULT);
});

test("decodeProjectPacksInitResult reads the host's larger answer, keeping only what it prints", () => {
  // `packs_repo.rs` returns more keys than this reader keeps. A reader that
  // refused the response because it did not recognise `pushError` would
  // break the button every time the host learned to report one more fact.
  assert.deepEqual(decodeProjectPacksInitResult(HOST_RESULT), RESULT);
});

test("decodeProjectPacksInitResult keeps the ids the host may honestly not have", () => {
  // A push that did not land withholds the 30624; a relay that has not yet
  // published the 30618 has no push record. Both are `null`, never invented.
  const withheld = {
    ...HOST_RESULT,
    sourceEventId: null,
    pushRecordEventId: null,
    pushed: false,
    pushError: "the relay refused the push",
  };
  assert.deepEqual(decodeProjectPacksInitResult(withheld), {
    ...RESULT,
    sourceEventId: null,
    pushRecordEventId: null,
  });
});

test("decodeProjectPacksInitResult reads a seed failure: no commit, the reason, and a withdrawal", () => {
  // LANE-L31 (Finding 66): the seed/push failure path is a normal result now
  // (the announcement had already landed), not a thrown error carrying git's
  // raw stderr — see the module doc.
  const seedFailed = {
    ...HOST_RESULT,
    sourceEventId: null,
    seedCommitSha: null,
    seedError:
      "Author identity unknown … fatal: unable to auto-detect email address",
    pushRecordEventId: null,
    pushed: false,
    announcementWithdrawnEventId: "f".repeat(64),
  };
  assert.deepEqual(decodeProjectPacksInitResult(seedFailed), {
    ...RESULT,
    sourceEventId: null,
    seedCommitSha: null,
    seedError:
      "Author identity unknown … fatal: unable to auto-detect email address",
    pushRecordEventId: null,
    announcementWithdrawnEventId: "f".repeat(64),
  });
});

test("decodeProjectPacksInitResult refuses a missing key", () => {
  const { seedCommitSha: _seedCommitSha, ...withoutSeed } = RESULT;
  assert.throws(() => decodeProjectPacksInitResult(withoutSeed));
});

test("decodeProjectPacksInitResult refuses a wrong-typed field", () => {
  assert.throws(() =>
    decodeProjectPacksInitResult({ ...RESULT, sourceEventId: 123 }),
  );
});

test("decodeProjectPacksInitResult refuses a repoRef that is not a string", () => {
  assert.throws(() =>
    decodeProjectPacksInitResult({ ...RESULT, repoRef: null }),
  );
});

// --- LANE-L30: default id + validation, mirroring packs_repo.rs ---

test("defaultPacksRepoId lowercases, sanitizes, and appends -packs", () => {
  assert.equal(defaultPacksRepoId("agiterra"), "agiterra-packs");
  assert.equal(defaultPacksRepoId("  My Project  "), "my-project-packs");
  assert.equal(defaultPacksRepoId("A_b.c"), "a_b.c-packs");
});

test("defaultPacksRepoId survives the length bound with the suffix intact", () => {
  const id = defaultPacksRepoId("a".repeat(120));
  assert.ok(id.endsWith("-packs"), id);
  assert.ok(id.length <= 64, `${id.length} chars`);
});

test("packsRepoIdError accepts the default shape and any valid lowercase slug", () => {
  assert.equal(packsRepoIdError("agiterra-packs"), null);
  assert.equal(packsRepoIdError("a"), null);
  assert.equal(packsRepoIdError("repo_v2.0"), null);
  assert.equal(packsRepoIdError("a".repeat(64)), null);
});

test("packsRepoIdError refuses empty, over-length, and reserved-shape ids", () => {
  assert.match(packsRepoIdError(""), /cannot be empty/);
  assert.match(packsRepoIdError("a".repeat(65)), /64 characters or fewer/);
  assert.match(packsRepoIdError(".hidden"), /must not start with '\.'/);
  assert.match(packsRepoIdError("-leading-dash"), /must not start with '-'/);
  assert.match(packsRepoIdError("foo..bar"), /must not contain '\.\.'/);
});

test("packsRepoIdError refuses uppercase and any character outside the class", () => {
  assert.match(packsRepoIdError("Agiterra-Packs"), /lowercase letters/);
  assert.match(packsRepoIdError("my repo"), /lowercase letters/);
  assert.match(packsRepoIdError("foo/bar"), /lowercase letters/);
  assert.match(packsRepoIdError("a@b"), /lowercase letters/);
});
