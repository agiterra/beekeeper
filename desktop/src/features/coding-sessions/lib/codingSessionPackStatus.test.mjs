import assert from "node:assert/strict";
import test from "node:test";

import { decodeCodingSessionPackStatusResult } from "./codingSessionPackStatus.ts";

const REPO = `30617:${"a".repeat(64)}:agiterra-packs`;
const SHA = "b".repeat(40);

/** `SeatPackPreview` as `actor_seats.rs` serializes it, project rung. */
const FROM_PROJECT = {
  packStaged: true,
  origin: "project",
  role: "builder",
  packDir:
    "/Users/x/Library/Application Support/io.agiterra.beekeeper.app/packs/aaaaaaaa-agiterra-packs/personas/roles/builder",
  personaId: "builder",
  packRef: {
    repo: REPO,
    sha: SHA,
    role: "builder",
    path: "personas/roles/builder",
  },
  refusal: null,
  reason: null,
};

test("a project-sourced preview names the repository, the commit, and the tree", () => {
  assert.deepEqual(
    decodeCodingSessionPackStatusResult(FROM_PROJECT, "builder"),
    {
      schema: "buzz-coding-session-pack-status/v1",
      implementation: "buzz-core",
      hasSource: true,
      repo: REPO,
      sha: SHA,
      path: "personas/roles",
      role: "builder",
      rolePath: "personas/roles/builder",
      roleFound: true,
      overlayFromCheckout: false,
      note: `Stages personas/roles/builder from this project's packs repository, at ${SHA.slice(0, 8)}.`,
    },
  );
});

test("the shipped-defaults rung is not passed off as a repository", () => {
  // `app:shipped` is not a coordinate anyone can go and look at, so the
  // reader must not put it in `repo` — the screen would send a person
  // hunting for a repository that does not exist.
  const shipped = decodeCodingSessionPackStatusResult(
    {
      ...FROM_PROJECT,
      origin: "shipped",
      packRef: { ...FROM_PROJECT.packRef, repo: "app:shipped", sha: "0.5.16" },
    },
    "builder",
  );
  assert.equal(shipped.hasSource, false);
  assert.equal(shipped.repo, null);
  assert.equal(shipped.sha, null);
  assert.equal(shipped.rolePath, null);
  assert.equal(shipped.roleFound, true);
  assert.match(shipped.note, /this build of Beekeeper ships/);
});

test("a checkout overlay is disclosed as an override, not as the project's source", () => {
  const overlaid = decodeCodingSessionPackStatusResult(
    { ...FROM_PROJECT, origin: "checkout", packRef: null },
    "builder",
  );
  assert.equal(overlaid.overlayFromCheckout, true);
  assert.equal(overlaid.hasSource, false);
  assert.match(overlaid.note, /overrides the project's packs repository/);
});

test("no pack is said plainly rather than left blank", () => {
  const none = decodeCodingSessionPackStatusResult(
    {
      ...FROM_PROJECT,
      packStaged: false,
      origin: "none",
      packRef: null,
      packDir: null,
      personaId: null,
    },
    "builder",
  );
  assert.equal(none.roleFound, false);
  assert.match(none.note, /run on its persona prompt alone/);
});

test("a refusal is shown in the host's own words, with its reason", () => {
  const refused = decodeCodingSessionPackStatusResult(
    {
      ...FROM_PROJECT,
      packStaged: false,
      origin: "none",
      packRef: null,
      refusal: "This project stages its agents from a packs repository…",
      reason: "the packs repository could not be fetched",
    },
    "builder",
  );
  assert.equal(
    refused.note,
    "This project stages its agents from a packs repository… (the packs repository could not be fetched)",
  );
});

test("the role the caller asked about is used when the host echoes none", () => {
  const preview = decodeCodingSessionPackStatusResult(
    { ...FROM_PROJECT, role: null },
    "architect",
  );
  assert.equal(preview.role, "architect");
});

test("a malformed boundary answer is refused, not partially read", () => {
  for (const bad of [
    null,
    { ...FROM_PROJECT, packStaged: "true" },
    { ...FROM_PROJECT, origin: "somewhere-else" },
    { ...FROM_PROJECT, packRef: { repo: REPO, sha: SHA } },
    (() => {
      const { refusal: _refusal, ...withoutRefusal } = FROM_PROJECT;
      return withoutRefusal;
    })(),
  ]) {
    assert.throws(
      () => decodeCodingSessionPackStatusResult(bad, "builder"),
      `accepted ${JSON.stringify(bad)}`,
    );
  }
});
