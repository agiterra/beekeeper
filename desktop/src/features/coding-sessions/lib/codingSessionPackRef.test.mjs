import assert from "node:assert/strict";
import test from "node:test";

import {
  codingSessionSeatPackLine,
  derivePackRefs,
  isShippedPackRef,
  PACK_REF_SHIPPED_REPO,
  readPackRef,
} from "./codingSessionPackRef.ts";

const OWNER = "a".repeat(64);
const SHA40 = "b".repeat(40);

const REAL_PACK_REF = {
  repo: `30617:${OWNER}:agiterra-packs`,
  sha: SHA40,
  role: "builder",
  path: "personas/roles/builder",
};

const SHIPPED_PACK_REF = {
  repo: PACK_REF_SHIPPED_REPO,
  sha: "0.1.0",
  role: "builder",
  path: "personas/roles/builder",
};

test("readPackRef accepts the exact four-key shape", () => {
  assert.deepEqual(readPackRef(REAL_PACK_REF), REAL_PACK_REF);
});

test("readPackRef refuses null — absence is a wire fact, not this shape", () => {
  assert.equal(readPackRef(null), null);
});

test("readPackRef refuses a value that is not a plain object", () => {
  for (const value of [undefined, "packRef", 1, true, ["a"]]) {
    assert.equal(readPackRef(value), null, `accepted ${JSON.stringify(value)}`);
  }
});

test("readPackRef refuses an extra or missing key", () => {
  assert.equal(
    readPackRef({ ...REAL_PACK_REF, note: "extra" }),
    null,
    "accepted an unrecognised fifth key",
  );
  const { repo: _repo, ...withoutRepo } = REAL_PACK_REF;
  assert.equal(readPackRef(withoutRepo), null, "accepted a missing repo key");
});

test("readPackRef enforces each field's own shape", () => {
  const bad = [
    { ...REAL_PACK_REF, repo: "not-a-coordinate" },
    { ...REAL_PACK_REF, repo: `30621:${OWNER}:agiterra-packs` }, // wrong kind prefix
    { ...REAL_PACK_REF, sha: "deadbeef" }, // not 40 hex
    { ...REAL_PACK_REF, sha: SHA40.toUpperCase() }, // must be lowercase
    { ...REAL_PACK_REF, role: "Not A Slug" },
    { ...REAL_PACK_REF, role: "" },
    { ...REAL_PACK_REF, path: "" },
  ];
  for (const value of bad) {
    assert.equal(
      readPackRef(value),
      null,
      `accepted a malformed packRef: ${JSON.stringify(value)}`,
    );
  }
});

test("codingSessionSeatPackLine names the role and the sha's first 8 hex", () => {
  assert.equal(
    codingSessionSeatPackLine(REAL_PACK_REF),
    "pack builder@bbbbbbbb",
  );
});

test("codingSessionSeatPackLine discloses absence as a fact, never nothing", () => {
  assert.equal(codingSessionSeatPackLine(null), "no pack staged");
});

// The setup-lives-inside-the-app addendum (2026-09-03): a project with no
// 30624 source falls back to the app's own bundled packs, disclosed the same
// way as a real one — never folded into "no pack staged".
test("readPackRef accepts the shipped-defaults shape: app:shipped repo, a version sha", () => {
  assert.deepEqual(readPackRef(SHIPPED_PACK_REF), SHIPPED_PACK_REF);
});

test("readPackRef refuses app:shipped paired with a commit sha, and a real coordinate paired with a version", () => {
  assert.equal(
    readPackRef({ ...SHIPPED_PACK_REF, sha: SHA40 }),
    null,
    "app:shipped must carry a version, not a commit",
  );
  assert.equal(
    readPackRef({ ...REAL_PACK_REF, sha: "0.1.0" }),
    null,
    "a real coordinate must carry a commit, not a version",
  );
});

test("isShippedPackRef names exactly the app:shipped repo", () => {
  assert.equal(isShippedPackRef(SHIPPED_PACK_REF), true);
  assert.equal(isShippedPackRef(REAL_PACK_REF), false);
});

test("codingSessionSeatPackLine names shipped defaults by version, not a sha", () => {
  assert.equal(
    codingSessionSeatPackLine(SHIPPED_PACK_REF),
    "pack builder (shipped defaults v0.1.0)",
  );
});

/** A minimal generation, defaulting to no packRef. */
function generation(overrides = {}) {
  return { packRef: null, ...overrides };
}

test("a seat's active generation packRef wins when it has one", () => {
  const packs = derivePackRefs([
    {
      executionKey: "builder-1",
      activeGeneration: generation({ packRef: REAL_PACK_REF }),
      priorGenerations: [
        generation({ packRef: { ...REAL_PACK_REF, sha: "c".repeat(40) } }),
      ],
    },
  ]);
  assert.deepEqual(packs.get("builder-1"), REAL_PACK_REF);
});

test("a fresh resume with no packRef yet falls back to the newest prior generation's", () => {
  const older = { ...REAL_PACK_REF, sha: "c".repeat(40) };
  const packs = derivePackRefs([
    {
      executionKey: "builder-1",
      activeGeneration: generation(),
      priorGenerations: [generation({ packRef: older }), generation()],
    },
  ]);
  assert.deepEqual(packs.get("builder-1"), older);
});

test("a seat with no packRef anywhere maps to null", () => {
  const packs = derivePackRefs([
    {
      executionKey: "builder-1",
      activeGeneration: generation(),
      priorGenerations: [generation(), generation()],
    },
  ]);
  assert.equal(packs.get("builder-1"), null);
});

test("every execution gets its own entry", () => {
  const packs = derivePackRefs([
    {
      executionKey: "a",
      activeGeneration: generation({ packRef: REAL_PACK_REF }),
      priorGenerations: [],
    },
    { executionKey: "b", activeGeneration: generation(), priorGenerations: [] },
  ]);
  assert.equal(packs.size, 2);
  assert.deepEqual(packs.get("a"), REAL_PACK_REF);
  assert.equal(packs.get("b"), null);
});
