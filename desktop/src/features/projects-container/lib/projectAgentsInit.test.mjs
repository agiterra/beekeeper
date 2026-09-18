import assert from "node:assert/strict";
import test from "node:test";

import {
  decodeProjectAgentsInitResult,
  defaultAgentsRepoId,
  describeAgentsSetup,
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
  publicationError: null,
  commitIdentityName: "Beekeeper aaaaaaaa",
  commitIdentityEmail: "aaaaaaaa@beekeeper.local",
  agentsAnnouncementWithdrawnEventId: null,
  agentsAnnouncementWithdrawalError: null,
  complete: true,
  gap: null,
};

test("decodeProjectAgentsInitResult accepts the host's answer and refuses a malformed one", () => {
  assert.deepEqual(decodeProjectAgentsInitResult(COMPLETE), COMPLETE);
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
    "Repositories ready: demo (code) and demo-beekeeper-agents (seeded as Beekeeper aaaaaaaa, commit eeeeeeee); the project's roles come from demo-beekeeper-agents on main.",
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
    "Not finished: agents repository demo-beekeeper-agents not seeded: no route to the relay. Finish setup from Project settings → Packs.",
  );
});
