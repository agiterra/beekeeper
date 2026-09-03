import assert from "node:assert/strict";
import test from "node:test";

import { decodeProjectPacksInitResult } from "./projectPacksInit.ts";

const RESULT = {
  repoRef: `30617:${"a".repeat(64)}:agiterra-packs`,
  sourceEventId: "b".repeat(64),
  seedCommitSha: "c".repeat(40),
  pushRecordEventId: "d".repeat(64),
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

test("decodeProjectPacksInitResult accepts the four keys the panel prints", () => {
  assert.deepEqual(decodeProjectPacksInitResult(RESULT), RESULT);
});

test("decodeProjectPacksInitResult reads the host's larger answer, keeping only what it prints", () => {
  // `packs_repo.rs` returns twelve keys. A reader that refused the response
  // because it did not recognise `pushError` would break the button every
  // time the host learned to report one more fact.
  assert.deepEqual(decodeProjectPacksInitResult(HOST_RESULT), RESULT);
});

test("decodeProjectPacksInitResult keeps the two ids the host may honestly not have", () => {
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
