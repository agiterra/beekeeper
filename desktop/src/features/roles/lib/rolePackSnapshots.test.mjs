import assert from "node:assert/strict";
import test from "node:test";

import {
  buildRolePackSnapshots,
  readRolePackCoordinate,
  revisionShas,
} from "./rolePackSnapshots.ts";

const PROJECT = "30621:a".repeat(1);
const REPO = "30617:owner:packs";
const OTHER_REPO = "30617:other:packs";
const SHA = "a".repeat(40);
const EARLIER_SHA = "b".repeat(40);
const LATER_SHA = "c".repeat(40);

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
      status: "running",
      packRef: coordinate(),
      ...overrides,
    },
  };
}

function revisions(overrides = {}) {
  return {
    repo: REPO,
    currentSha: SHA,
    comparedAt: 1_700_000_000_500,
    reason: null,
    relations: [],
    ...overrides,
  };
}

function build(overrides = {}) {
  return buildRolePackSnapshots({
    projectRef: PROJECT,
    resolvedAt: null,
    resolvedPacks: [pack()],
    catalogEntries: [entry()],
    sourceRepo: REPO,
    sourceKnown: true,
    revisions: null,
    revisionsError: null,
    nowSeconds: 1_700_000_300,
    ...overrides,
  });
}

const SHIPPED = {
  repo: "app:shipped",
  sha: "0.4.2-block",
  role: "builder",
  path: "personas/roles/builder",
};

test("with no project source, a shipped claim matching this machine's bundle is current", () => {
  const snapshots = build({
    sourceRepo: null,
    resolvedPacks: [pack({ origin: "shipped", packRef: SHIPPED })],
    catalogEntries: [entry({ packRef: SHIPPED })],
  });
  assert.equal(snapshots.reported[0].relation, "current");
  assert.equal(snapshots.reported[0].adoption, "none");
});

test("with no project source, a shipped claim from another app version says so", () => {
  const snapshots = build({
    sourceRepo: null,
    resolvedPacks: [pack({ origin: "shipped", packRef: SHIPPED })],
    catalogEntries: [entry({ packRef: { ...SHIPPED, sha: "0.4.1-block" } })],
  });
  assert.equal(snapshots.reported[0].relation, "shipped-differs");
  assert.equal(snapshots.reported[0].adoption, "keeps-until-next-generation");
});

test("with no project source, a shipped claim for a role this machine does not bundle is unknown here", () => {
  const snapshots = build({
    sourceRepo: null,
    resolvedPacks: [],
    catalogEntries: [entry({ packRef: SHIPPED })],
  });
  assert.equal(snapshots.reported[0].relation, "unknown-here");
});

test("a repository claim is unknown here, never different, while the source read failed", () => {
  const snapshots = build({ sourceRepo: null, sourceKnown: false });
  assert.equal(snapshots.reported[0].relation, "unknown-here");
});

test("a repository claim is a different source once the project is known to name none", () => {
  const snapshots = build({ sourceRepo: null, sourceKnown: true });
  assert.equal(snapshots.reported[0].relation, "different-source");
});

test("marks a reported sha earlier than HEAD, disclosing the behind count", () => {
  const snapshots = build({
    revisions: revisions({
      relations: [{ sha: SHA, relation: "earlier", behind: 4, ahead: null }],
    }),
  });
  const row = snapshots.reported[0];
  assert.equal(row.relation, "earlier");
  assert.equal(row.behind, 4);
  assert.equal(row.ahead, null);
});

test("marks a reported sha later than HEAD, disclosing the ahead count", () => {
  const snapshots = build({
    revisions: revisions({
      relations: [{ sha: SHA, relation: "later", behind: null, ahead: 2 }],
    }),
  });
  const row = snapshots.reported[0];
  assert.equal(row.relation, "later");
  assert.equal(row.ahead, 2);
  assert.equal(row.behind, null);
});

test("falls back to unknown-here when no comparison has ever run", () => {
  const snapshots = build({ revisions: null });
  assert.equal(snapshots.reported[0].relation, "unknown-here");
  assert.equal(snapshots.comparison, null);
});

test("falls back to unknown-here and drops stale data once the comparison errors", () => {
  const snapshots = build({
    revisions: revisions({
      relations: [{ sha: SHA, relation: "current", behind: null, ahead: null }],
    }),
    revisionsError: "git exited 1",
  });
  // A row must never read "current" off a stale answer the failed call
  // couldn't reproduce.
  assert.equal(snapshots.reported[0].relation, "unknown-here");
  assert.equal(snapshots.comparison, null);
});

test("marks a shipped coordinate different-source even when its sha matches HEAD", () => {
  const snapshots = build({
    catalogEntries: [
      entry({
        packRef: {
          repo: "app:shipped",
          sha: "0.4.2-block",
          role: "builder",
          path: "roles/builder",
        },
      }),
    ],
    revisions: revisions({
      relations: [{ sha: SHA, relation: "current", behind: null, ahead: null }],
    }),
  });
  assert.equal(snapshots.reported[0].relation, "different-source");
});

test("marks a coordinate naming a foreign project repo different-source", () => {
  const snapshots = build({
    catalogEntries: [entry({ packRef: coordinate({ repo: OTHER_REPO }) })],
  });
  assert.equal(snapshots.reported[0].relation, "different-source");
});

test("marks a partial coordinate incomplete", () => {
  const snapshots = build({
    catalogEntries: [entry({ packRef: { repo: REPO, sha: SHA } })],
  });
  assert.equal(snapshots.reported[0].relation, "incomplete");
  assert.match(
    snapshots.reported[0].reason ?? "",
    /metadata claim has no complete pack coordinate/,
  );
});

test("adds the adoption sentence for a running, non-current row", () => {
  const snapshots = build({
    catalogEntries: [entry({ status: "running" })],
    revisions: revisions({
      relations: [{ sha: SHA, relation: "earlier", behind: 1, ahead: null }],
    }),
  });
  assert.equal(snapshots.reported[0].adoption, "keeps-until-next-generation");
});

for (const status of ["completed", "stopped", "failed"]) {
  test(`omits the adoption sentence for a ${status} row`, () => {
    const snapshots = build({
      catalogEntries: [entry({ status })],
      revisions: revisions({
        relations: [{ sha: SHA, relation: "earlier", behind: 1, ahead: null }],
      }),
    });
    assert.equal(snapshots.reported[0].adoption, "none");
  });
}

test("omits the adoption sentence for a current row even while running", () => {
  const snapshots = build({
    catalogEntries: [entry({ status: "running" })],
    revisions: revisions({
      relations: [{ sha: SHA, relation: "current", behind: null, ahead: null }],
    }),
  });
  assert.equal(snapshots.reported[0].relation, "current");
  assert.equal(snapshots.reported[0].adoption, "none");
});

test("omits the adoption sentence for an incomplete row", () => {
  const snapshots = build({
    catalogEntries: [entry({ packRef: { repo: REPO, sha: SHA } })],
  });
  assert.equal(snapshots.reported[0].adoption, "none");
});

test("does not lend a shared channel's report from another project", () => {
  const snapshots = build({
    resolvedAt: null,
    catalogEntries: [entry(), entry({ projectRef: "30621:other:project" })],
  });

  assert.equal(snapshots.reported.length, 1);
  assert.equal(snapshots.reported[0]?.generationId, "generation-1");
});

test("keeps an incomplete resolved coordinate unknown", () => {
  const snapshots = build({
    resolvedPacks: [pack({ packRef: { repo: REPO, sha: SHA } })],
  });

  assert.equal(snapshots.resolved[0]?.coordinate, null);
  assert.match(
    snapshots.resolved[0]?.reason ?? "",
    /complete portable coordinate/,
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

test("revisionShas dedupes, sorts, and excludes shipped or foreign-repo coordinates", () => {
  const shas = revisionShas(
    [
      entry({ packRef: coordinate({ sha: LATER_SHA }) }),
      entry({ packRef: coordinate({ sha: EARLIER_SHA }) }),
      // Duplicate of the first row — must not appear twice.
      entry({ packRef: coordinate({ sha: LATER_SHA }) }),
      // Shipped fallback — never a git revision to ask about.
      entry({
        packRef: {
          repo: "app:shipped",
          sha: "0.4.2-block",
          role: "builder",
          path: "roles/builder",
        },
      }),
      // Names a different project repo entirely.
      entry({ packRef: coordinate({ repo: OTHER_REPO, sha: SHA }) }),
      // Incomplete coordinate — excluded like any other unreadable claim.
      entry({ packRef: { repo: REPO, sha: SHA } }),
    ],
    REPO,
  );

  assert.deepEqual(shas, [EARLIER_SHA, LATER_SHA].sort());
});

test("revisionShas answers nothing when the project names no source", () => {
  const shas = revisionShas([entry()], null);
  assert.deepEqual(shas, []);
});
