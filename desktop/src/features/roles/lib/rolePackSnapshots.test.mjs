import assert from "node:assert/strict";
import test from "node:test";

import {
  buildRolePackSnapshots,
  readRolePackCoordinate,
  revisionShas,
} from "./rolePackSnapshots.ts";
import { rolePackProvenanceKey } from "./rolePackProvenance.ts";

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

/** One `ProjectPackRevisionEntry` — the wire shape, `note` included. */
function relationEntry(overrides = {}) {
  return {
    sha: SHA,
    relation: "current",
    behind: null,
    ahead: null,
    note: null,
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
    provenance: null,
    provenanceError: null,
    nowSeconds: 1_700_000_300,
    ...overrides,
  });
}

/** The `RolePackProvenanceRow` the default `entry()` fixture projects to —
 * `entry()` names no `commandTarget`, so `targetKey` is null. */
const DEFAULT_PROVENANCE_ROW = {
  channelId: "project-channel",
  targetKey: null,
  metadataEventId: null,
  signerPubkey: "b".repeat(64),
  sessionRef: null,
};

function provenanceResult(overrides = {}) {
  return {
    dispositions: new Map(),
    notes: [],
    ...overrides,
  };
}

function disposition(overrides = {}) {
  return {
    state: "proof-unavailable",
    reason: "no accepted lifecycle proof for this generation",
    founderPubkey: null,
    commandSignerPubkey: null,
    commandEventId: null,
    receiptEventId: null,
    genesisEventId: null,
    ...overrides,
  };
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
      relations: [relationEntry({ relation: "earlier", behind: 4 })],
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
      relations: [relationEntry({ relation: "later", ahead: 2 })],
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
      relations: [relationEntry({ relation: "current" })],
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
      relations: [relationEntry({ relation: "current" })],
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
      relations: [relationEntry({ relation: "earlier", behind: 1 })],
    }),
  });
  assert.equal(snapshots.reported[0].adoption, "keeps-until-next-generation");
});

for (const status of ["completed", "stopped", "failed"]) {
  test(`omits the adoption sentence for a ${status} row`, () => {
    const snapshots = build({
      catalogEntries: [entry({ status })],
      revisions: revisions({
        relations: [relationEntry({ relation: "earlier", behind: 1 })],
      }),
    });
    assert.equal(snapshots.reported[0].adoption, "none");
  });
}

test("omits the adoption sentence for a current row even while running", () => {
  const snapshots = build({
    catalogEntries: [entry({ status: "running" })],
    revisions: revisions({
      relations: [relationEntry({ relation: "current" })],
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

test("carries the comparison's per-row note through to the reported snapshot", () => {
  const snapshots = build({
    revisions: revisions({
      relations: [
        relationEntry({
          relation: "unrelated",
          note: "this machine's packs checkout is shallow, so ancestry cannot be ranked",
        }),
      ],
    }),
  });
  assert.equal(
    snapshots.reported[0].note,
    "this machine's packs checkout is shallow, so ancestry cannot be ranked",
  );
});

test("carries a null note when the comparison entry has none", () => {
  const snapshots = build({
    revisions: revisions({
      relations: [relationEntry({ relation: "current" })],
    }),
  });
  assert.equal(snapshots.reported[0].note, null);
});

test("never carries a note for a row the comparison never ranked", () => {
  const snapshots = build({ revisions: null });
  assert.equal(snapshots.reported[0].relation, "unknown-here");
  assert.equal(snapshots.reported[0].note, null);
});

test("never carries a note once the comparison errors and rows fall back to unknown-here", () => {
  const snapshots = build({
    revisions: revisions({
      relations: [relationEntry({ relation: "unrelated", note: "stale note" })],
    }),
    revisionsError: "git exited 1",
  });
  assert.equal(snapshots.reported[0].relation, "unknown-here");
  // The section-level "Revision comparison unavailable" line already
  // discloses the failure; a row must not also surface a stale per-row note.
  assert.equal(snapshots.reported[0].note, null);
});

test("never carries a note for an incomplete, different-source, or shipped-differs row", () => {
  const incomplete = build({
    catalogEntries: [entry({ packRef: { repo: REPO, sha: SHA } })],
  });
  assert.equal(incomplete.reported[0].note, null);

  const differentSource = build({
    catalogEntries: [entry({ packRef: coordinate({ repo: OTHER_REPO }) })],
  });
  assert.equal(differentSource.reported[0].note, null);

  const shippedDiffers = build({
    sourceRepo: null,
    resolvedPacks: [pack({ origin: "shipped", packRef: SHIPPED })],
    catalogEntries: [entry({ packRef: { ...SHIPPED, sha: "0.4.1-block" } })],
  });
  assert.equal(shippedDiffers.reported[0].relation, "shipped-differs");
  assert.equal(shippedDiffers.reported[0].note, null);
});

// ── Provenance lookup ────────────────────────────────────────────────────

test("a row with no disposition and no provenance answer yet is proof-unavailable, unchecked", () => {
  const snapshots = build({ provenance: null, provenanceError: null });
  const row = snapshots.reported[0];
  assert.equal(row.provenance, "proof-unavailable");
  assert.equal(row.provenanceReason, "Provenance has not been checked yet.");
  assert.equal(row.founderPubkey, null);
  assert.deepEqual(snapshots.provenanceNotes, []);
});

test("a row with no disposition and a failed provenance read carries the hook's error", () => {
  const snapshots = build({
    provenance: null,
    provenanceError: "the relay closed the subscription",
  });
  assert.equal(snapshots.reported[0].provenance, "proof-unavailable");
  assert.equal(
    snapshots.reported[0].provenanceReason,
    "the relay closed the subscription",
  );
});

test("a commissioned disposition beside a provenance error is not trusted", () => {
  const key = rolePackProvenanceKey(DEFAULT_PROVENANCE_ROW);
  const snapshots = build({
    provenance: provenanceResult({
      dispositions: new Map([
        [
          key,
          {
            state: "commissioned",
            reason: null,
            founderPubkey: "f".repeat(64),
            commandSignerPubkey: "f".repeat(64),
            commandEventId: "c".repeat(64),
            receiptEventId: "d".repeat(64),
            genesisEventId: "e".repeat(64),
          },
        ],
      ]),
      notes: ["Lifecycle receipts were truncated at 1000 events."],
    }),
    provenanceError: "the relay closed the connection",
  });
  assert.equal(snapshots.reported[0].provenance, "proof-unavailable");
  assert.equal(
    snapshots.reported[0].provenanceReason,
    "the relay closed the connection",
  );
  assert.equal(snapshots.reported[0].founderPubkey, null);
  assert.deepEqual(snapshots.provenanceNotes, []);
});

test("a commissioned disposition carries the founder pubkey and a null reason", () => {
  const founder = "f".repeat(64);
  const key = rolePackProvenanceKey(DEFAULT_PROVENANCE_ROW);
  const snapshots = build({
    provenance: provenanceResult({
      dispositions: new Map([
        [
          key,
          disposition({
            state: "commissioned",
            reason: null,
            founderPubkey: founder,
            commandSignerPubkey: founder,
          }),
        ],
      ]),
    }),
  });
  const row = snapshots.reported[0];
  assert.equal(row.provenance, "commissioned");
  assert.equal(row.provenanceReason, null);
  assert.equal(row.founderPubkey, founder);
});

test("a proof-unavailable disposition carries the fold's own product sentence", () => {
  const key = rolePackProvenanceKey(DEFAULT_PROVENANCE_ROW);
  const snapshots = build({
    provenance: provenanceResult({
      dispositions: new Map([
        [
          key,
          disposition({
            reason:
              "The command that started generation 2 was signed by a key that is not the founder; operator grants are not projected yet.",
          }),
        ],
      ]),
    }),
  });
  assert.equal(snapshots.reported[0].provenance, "proof-unavailable");
  assert.match(
    snapshots.reported[0].provenanceReason ?? "",
    /operator grants are not projected yet/,
  );
});

test("a disputed disposition names the contradiction", () => {
  const key = rolePackProvenanceKey(DEFAULT_PROVENANCE_ROW);
  const snapshots = build({
    provenance: provenanceResult({
      dispositions: new Map([
        [
          key,
          disposition({
            state: "disputed",
            reason: "The 44223 signer is not the accepted provider.",
          }),
        ],
      ]),
    }),
  });
  assert.equal(snapshots.reported[0].provenance, "disputed");
  assert.equal(
    snapshots.reported[0].provenanceReason,
    "The 44223 signer is not the accepted provider.",
  );
});

test("a row whose key is absent from an otherwise-resolved provenance result still falls back honestly", () => {
  const snapshots = build({
    provenance: provenanceResult({ dispositions: new Map() }),
    provenanceError: null,
  });
  assert.equal(snapshots.reported[0].provenance, "proof-unavailable");
  assert.equal(
    snapshots.reported[0].provenanceReason,
    "Provenance has not been checked yet.",
  );
});

test("carries the provenance fold's notes through to the section model", () => {
  const snapshots = build({
    provenance: provenanceResult({
      notes: ["Lifecycle receipts could not be read: timed out."],
    }),
  });
  assert.deepEqual(snapshots.provenanceNotes, [
    "Lifecycle receipts could not be read: timed out.",
  ]);
});

test("provenanceNotes is empty before any provenance answer exists", () => {
  const snapshots = build({ provenance: null });
  assert.deepEqual(snapshots.provenanceNotes, []);
});

// ── session.role passthrough ─────────────────────────────────────────────

test("a row names its own role even with no version coordinate", () => {
  const snapshots = build({
    catalogEntries: [entry({ packRef: undefined, role: "lead" })],
  });
  const row = snapshots.reported[0];
  assert.equal(row.role, "lead");
  assert.equal(row.coordinate, null);
});

test("a row with neither a role nor a coordinate reports role: null", () => {
  const snapshots = build({
    catalogEntries: [entry({ packRef: undefined })],
  });
  const row = snapshots.reported[0];
  assert.equal(row.role, null);
  assert.equal(row.coordinate, null);
});
