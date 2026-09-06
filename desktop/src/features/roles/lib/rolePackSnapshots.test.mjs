import assert from "node:assert/strict";
import test from "node:test";

import {
  buildRolePackSnapshots,
  readRolePackCoordinate,
} from "./rolePackSnapshots.ts";

const PROJECT = "30621:a".repeat(1);
const REPO = "30617:owner:packs";
const SHA = "a".repeat(40);

function coordinate(overrides = {}) {
  return {
    repo: REPO,
    sha: SHA,
    role: "builder",
    path: "personas/roles/builder",
    ...overrides,
  };
}

function pack(overrides = {}) {
  return {
    role: "builder",
    displayName: "Builder",
    description: "",
    summary: "",
    version: null,
    origin: "project",
    packDir: "/local/roles/builder",
    packRef: coordinate(),
    skills: [],
    refusal: null,
    ...overrides,
  };
}

function entry(overrides = {}) {
  return {
    channelId: "project-channel",
    session: {
      generationId: "generation-1",
      projectRef: PROJECT,
      title: "Build the snapshot",
      label: "Builder",
      metadataAuthorityPubkey: "b".repeat(64),
      statusAt: 1_700_000_000_000,
      packRef: coordinate(),
      ...overrides,
    },
  };
}

test("matches only the full repo, sha, role, and path coordinate", () => {
  const snapshots = buildRolePackSnapshots({
    projectRef: PROJECT,
    resolvedAt: 1_700_000_000_123,
    resolvedPacks: [pack()],
    catalogEntries: [
      entry(),
      entry({
        generationId: "generation-other-repo",
        packRef: coordinate({ repo: "30617:other:packs" }),
      }),
      entry({
        generationId: "generation-other-path",
        packRef: coordinate({ path: "roles/builder" }),
      }),
    ],
  });

  assert.equal(snapshots.resolvedAt, 1_700_000_000_123);
  assert.deepEqual(
    snapshots.reported.map((snapshot) => snapshot.comparison).sort(),
    ["claims-differ", "claims-differ", "claims-match"],
  );
});

test("keeps a forged member coordinate explicitly unverified", () => {
  const memberPubkey = "f".repeat(64);
  const snapshots = buildRolePackSnapshots({
    projectRef: PROJECT,
    resolvedAt: 1_700_000_000_123,
    resolvedPacks: [pack()],
    // Open ingress can supply this row from any writable channel member. No
    // create or receipt is present, so equality is only the member's claim.
    catalogEntries: [
      entry({
        generationId: "member-invented-generation",
        metadataAuthorityPubkey: memberPubkey,
        packRef: coordinate(),
      }),
    ],
  });

  assert.deepEqual(snapshots.reported, [
    {
      channelId: "project-channel",
      generationId: "member-invented-generation",
      label: "Build the snapshot",
      claimedByPubkey: memberPubkey,
      reportedAt: 1_700_000_000_000,
      coordinate: coordinate(),
      reason: null,
      comparison: "claims-match",
      provenance: "unverified-channel-metadata",
    },
  ]);
});

test("does not lend a shared channel's report from another project", () => {
  const snapshots = buildRolePackSnapshots({
    projectRef: PROJECT,
    resolvedAt: null,
    resolvedPacks: [pack()],
    catalogEntries: [entry(), entry({ projectRef: "30621:other:project" })],
  });

  assert.equal(snapshots.reported.length, 1);
  assert.equal(snapshots.reported[0]?.generationId, "generation-1");
});

test("keeps incomplete resolved and reported coordinates unknown", () => {
  const snapshots = buildRolePackSnapshots({
    projectRef: PROJECT,
    resolvedAt: null,
    resolvedPacks: [pack({ packRef: { repo: REPO, sha: SHA } })],
    catalogEntries: [entry({ packRef: { repo: REPO, sha: SHA } })],
  });

  assert.equal(snapshots.resolved[0]?.coordinate, null);
  assert.match(
    snapshots.resolved[0]?.reason ?? "",
    /complete portable coordinate/,
  );
  assert.equal(snapshots.reported[0]?.comparison, "unknown");
  assert.match(
    snapshots.reported[0]?.reason ?? "",
    /metadata claim has no complete pack coordinate/,
  );
});

test("rejects malformed coordinate values instead of repairing them", () => {
  assert.equal(readRolePackCoordinate(null), null);
  assert.equal(
    readRolePackCoordinate({ repo: REPO, sha: SHA, role: "builder" }),
    null,
  );
  assert.equal(readRolePackCoordinate(coordinate({ path: "" })), null);
});
