import assert from "node:assert/strict";
import test from "node:test";

import {
  decodeProjectAgentsInitResult,
  defaultAgentsRepoId,
  describeAgentsSetup,
  describeCheckoutOutcome,
  describeCodeSeedOutcome,
  describeMigrationOutcome,
  describeRosterOutcome,
  foldMigrationNotes,
  isProjectAgentsRepoSource,
} from "./projectAgentsInit.ts";

const OWNER = "a".repeat(64);

/** What `project_agents_init` answers with (`agents_repo.rs`). */
const COMPLETE = {
  projectRef: `30621:${OWNER}:demo`,
  codeRepoRef: `30617:${OWNER}:demo`,
  codeRepoId: "demo",
  codeAnnouncementEventId: "c".repeat(64),
  codeRepoExisted: false,
  agentsRepoRef: `30617:${OWNER}:demo-beekeeper-agents`,
  agentsRepoId: "demo-beekeeper-agents",
  agentsCloneUrl: `https://hive.example/git/${OWNER}/demo-beekeeper-agents`,
  agentsAnnouncementEventId: "d".repeat(64),
  agentsRepoExisted: false,
  branch: "main",
  roles: ["builder", "lead"],
  seedCommitSha: "e".repeat(40),
  seedError: null,
  seedSkipped: false,
  pushed: true,
  pushError: null,
  pushRecordEventId: "f".repeat(64),
  sourceEventId: "1".repeat(64),
  sourceExisted: false,
  migratedFrom: null,
  migratedRoles: [],
  migrationNotes: [],
  sourceConflict: false,
  codeRepoAdopted: false,
  publicationError: null,
  commitIdentityName: "Beekeeper aaaaaaaa",
  commitIdentityEmail: "aaaaaaaa@beekeeper.local",
  agentsAnnouncementWithdrawnEventId: null,
  agentsAnnouncementWithdrawalError: null,
  complete: true,
  gap: null,
  agentsInstalled: [
    {
      role: "builder",
      name: "Builder",
      pubkey: "1".repeat(64),
      refreshed: false,
    },
    { role: "lead", name: "Lead", pubkey: "2".repeat(64), refreshed: false },
  ],
  agentsError: null,
  codeSeedCommitSha: "9".repeat(40),
  codeSeedSkipped: false,
  codeSeedError: null,
  checkoutPath: "/Users/x/Code/demo",
  checkoutCloned: true,
  checkoutError: null,
  rosterAdded: ["1".repeat(64), "2".repeat(64)],
  rosterError: null,
};

test("decodeProjectAgentsInitResult accepts the host's answer and refuses a malformed one", () => {
  assert.deepEqual(decodeProjectAgentsInitResult(COMPLETE), COMPLETE);
  // The checkout and roster facts are part of the shape, not optional
  // extras: a host that omits them is a host this reader cannot vouch for.
  for (const field of ["checkoutPath", "checkoutCloned", "rosterAdded"]) {
    const { [field]: _dropped, ...without } = COMPLETE;
    assert.throws(
      () => decodeProjectAgentsInitResult(without),
      /malformed/,
      field,
    );
  }
  assert.throws(
    () => decodeProjectAgentsInitResult({ ...COMPLETE, rosterAdded: [1] }),
    /malformed/,
  );
  assert.throws(
    () => decodeProjectAgentsInitResult({ ...COMPLETE, complete: "yes" }),
    /malformed/,
  );
  assert.throws(
    () => decodeProjectAgentsInitResult({ ...COMPLETE, roles: [1] }),
    /malformed/,
  );
  assert.throws(() => decodeProjectAgentsInitResult(null), /malformed/);
});

test("defaultAgentsRepoId keeps the suffix whole inside 64 characters, as the host does", () => {
  assert.equal(defaultAgentsRepoId("tank-loop"), "tank-loop-beekeeper-agents");
  assert.equal(
    defaultAgentsRepoId("  Tank Loop!  "),
    "tank-loop-beekeeper-agents",
  );
  const long = defaultAgentsRepoId("x".repeat(80));
  assert.equal(long.length, 64);
  assert.ok(long.endsWith("-beekeeper-agents"));
});

test("describeAgentsSetup prints the host's verdict, not a happier one", () => {
  assert.equal(
    describeAgentsSetup(COMPLETE),
    "Repositories ready: demo (code) and demo-beekeeper-agents (seeded as Beekeeper aaaaaaaa, commit eeeeeeee); the project's roles come from demo-beekeeper-agents on main. 2 default agents installed: Builder, Lead. Code cloned to /Users/x/Code/demo and recorded as this project's folder. 2 agents added to the project roster.",
  );
  assert.match(
    describeAgentsSetup({
      ...COMPLETE,
      agentsInstalled: [],
      agentsError: "no packs cache",
    }),
    /No default agents were installed: no packs cache\./,
  );
  assert.throws(
    () =>
      decodeProjectAgentsInitResult({
        ...COMPLETE,
        agentsInstalled: [{ role: "lead" }],
      }),
    /malformed/,
  );
  assert.match(
    describeAgentsSetup({
      ...COMPLETE,
      seedSkipped: true,
      seedCommitSha: null,
    }),
    /already seeded/,
  );
  const gap = {
    ...COMPLETE,
    pushed: false,
    pushError: "no route to the relay",
    sourceEventId: null,
    complete: false,
    gap: "agents repository demo-beekeeper-agents not seeded: no route to the relay",
  };
  assert.equal(
    describeAgentsSetup(gap),
    "Not finished: agents repository demo-beekeeper-agents not seeded: no route to the relay. Finish setup from Project settings → Packs. Code cloned to /Users/x/Code/demo and recorded as this project's folder. 2 agents added to the project roster.",
  );
});

test("the checkout, code seed and roster sentences say what the host did, or why not", () => {
  assert.equal(
    describeCheckoutOutcome({
      checkoutPath: "/Users/x/Code/demo",
      checkoutCloned: false,
      checkoutError: null,
    }),
    "Code already checked out at /Users/x/Code/demo; recorded as this project's folder.",
  );
  assert.equal(
    describeCheckoutOutcome({
      checkoutPath: null,
      checkoutCloned: false,
      checkoutError: "/Users/x/Code/demo is a checkout of another repository",
    }),
    "No folder was recorded for this project: /Users/x/Code/demo is a checkout of another repository.",
  );
  assert.equal(
    describeCheckoutOutcome({
      checkoutPath: null,
      checkoutCloned: false,
      checkoutError: null,
    }),
    "No folder was recorded for this project: the host did not say why.",
  );
  assert.equal(
    describeCodeSeedOutcome({
      codeSeedCommitSha: "9".repeat(40),
      codeSeedSkipped: false,
      codeSeedError: null,
    }),
    "seeded, commit 99999999",
  );
  assert.equal(
    describeCodeSeedOutcome({
      codeSeedCommitSha: null,
      codeSeedSkipped: true,
      codeSeedError: null,
    }),
    "already had commits",
  );
  assert.equal(
    describeCodeSeedOutcome({
      codeSeedCommitSha: null,
      codeSeedSkipped: false,
      codeSeedError: "push rejected",
    }),
    "push rejected",
  );
  assert.equal(
    describeRosterOutcome({ rosterAdded: ["1".repeat(64)], rosterError: null }),
    "1 agent added to the project roster.",
  );
  assert.equal(
    describeRosterOutcome({ rosterAdded: [], rosterError: null }),
    "0 agents added to the project roster.",
  );
  assert.equal(
    describeRosterOutcome({
      rosterAdded: [],
      rosterError: "restricted: project write access required",
    }),
    "0 agents added to the project roster. Roster error: restricted: project write access required.",
  );
});

test("isProjectAgentsRepoSource tells the three states apart", () => {
  const own = {
    repo: `30617:${OWNER}:demo-beekeeper-agents`,
    path: ".",
    ref: "refs/heads/main",
  };
  assert.equal(isProjectAgentsRepoSource(own, "demo"), true);
  // No source at all: the project has never been pointed anywhere.
  assert.equal(isProjectAgentsRepoSource(null, "demo"), false);
  // A pack-layout source under a sub-path, which is what a project created
  // before the pivot has.
  assert.equal(
    isProjectAgentsRepoSource(
      {
        repo: `30617:${OWNER}:agiterra-packs`,
        path: "personas/roles",
        ref: "refs/heads/main",
      },
      "bee-keeper",
    ),
    false,
  );
  // The right layout, but another project's repository.
  assert.equal(isProjectAgentsRepoSource({ ...own }, "other"), false);
  // A sha pin: drafts and seats follow a branch, so this is not the state
  // "Finish repository setup" can finish.
  assert.equal(isProjectAgentsRepoSource({ ...own, ref: null }, "demo"), false);
});

test("describeMigrationOutcome says what moved, and says when nothing was re-pointed", () => {
  assert.equal(describeMigrationOutcome(COMPLETE), "");
  const migrated = {
    migratedFrom: `30617:${OWNER}:agiterra-packs`,
    migratedRoles: ["builder", "lead"],
    migrationNotes: ["lead: the frontmatter's skills did not survive."],
    sourceConflict: false,
  };
  const sentence = describeMigrationOutcome(migrated);
  assert.match(sentence, /agiterra-packs/);
  assert.match(sentence, /2 roles converted \(builder, lead\)/);
  assert.match(sentence, /now reads its roles from this repository/);
  assert.match(sentence, /did not survive/);
  const refused = describeMigrationOutcome({
    ...migrated,
    sourceConflict: true,
  });
  assert.match(refused, /NOT re-pointed/);
});

test("foldMigrationNotes says one fact about seven roles, not seven facts", () => {
  const same = "the frontmatter's skills did not survive the layout change";
  const seven = [
    "architect",
    "builder",
    "designer",
    "lead",
    "poker",
    "runner",
    "verifier",
  ].map((role) => `${role}: ${same}`);
  const folded = foldMigrationNotes(seven);
  assert.equal(
    folded,
    ` architect, builder, designer, lead, poker, runner, verifier: ${same}`,
  );
  // The sentence appears once, not once per role.
  assert.equal(folded.split(same).length - 1, 1);

  // Two different reasons stay apart, each naming its own roles.
  const mixed = foldMigrationNotes([
    "lead: reason one",
    "poker: reason two",
    "runner: reason one",
  ]);
  assert.match(mixed, /lead, runner: reason one/);
  assert.match(mixed, /poker: reason two/);

  // Nothing to say stays silent, and a note in no such shape is untouched.
  assert.equal(foldMigrationNotes([]), "");
  assert.equal(
    foldMigrationNotes(["no role prefix here"]),
    " no role prefix here",
  );
});
