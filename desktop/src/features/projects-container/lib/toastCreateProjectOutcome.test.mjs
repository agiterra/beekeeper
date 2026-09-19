import assert from "node:assert/strict";
import { before, test } from "node:test";

import { JSDOM } from "jsdom";

/**
 * The create toast prints what the host actually did with the project's
 * folder and roster (plan D2/D3, 2026-09-19): where the code was cloned (or
 * reused), how many agents were put on the roster, and the host's reason
 * when either did not happen. It never invents a path or a count.
 *
 * `sonner` keeps every toast it was asked to show in `toast.getHistory()`,
 * so the sentence is read back from the real toast API rather than a stub.
 */

const OWNER = "a".repeat(64);

const dom = new JSDOM("<!doctype html><html><body></body></html>", {
  url: "http://localhost",
});

before(() => {
  Object.assign(globalThis, {
    document: dom.window.document,
    HTMLElement: dom.window.HTMLElement,
    window: dom.window,
  });
});

const PROJECT = {
  id: "p",
  dtag: "rpg-test",
  owner: OWNER,
  name: "RPG Test",
  description: "",
  createdAt: 1,
  address: `30621:${OWNER}:rpg-test`,
  repoAddrs: [],
  agentAddrs: [],
  channelIds: [],
  visibility: "private",
  members: [],
  icon: null,
  color: null,
};

/** What `project_agents_init` answers with (`agents_repo.rs`), complete. */
const COMPLETE = {
  projectRef: PROJECT.address,
  codeRepoRef: `30617:${OWNER}:rpg-test`,
  codeRepoId: "rpg-test",
  codeAnnouncementEventId: "c".repeat(64),
  codeRepoExisted: false,
  agentsRepoRef: `30617:${OWNER}:rpg-test-beekeeper-agents`,
  agentsRepoId: "rpg-test-beekeeper-agents",
  agentsCloneUrl: `https://hive.example/git/${OWNER}/rpg-test-beekeeper-agents`,
  agentsAnnouncementEventId: "d".repeat(64),
  agentsRepoExisted: false,
  branch: "main",
  roles: ["lead"],
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
  agentsInstalled: [
    { role: "lead", name: "Lead", pubkey: "2".repeat(64), refreshed: false },
  ],
  agentsError: null,
  codeSeedCommitSha: "9".repeat(40),
  codeSeedSkipped: false,
  codeSeedError: null,
  checkoutPath: "/Users/andy/Code/rpg-test",
  checkoutCloned: true,
  checkoutError: null,
  rosterAdded: ["2".repeat(64), "3".repeat(64)],
  rosterError: null,
};

async function lastToast() {
  const { toast } = await import("sonner");
  const history = toast.getHistory();
  return history[history.length - 1];
}

test("the create toast prints the clone path and the roster count", async () => {
  const { toastCreateProjectOutcome } = await import(
    "./toastCreateProjectOutcome.ts"
  );
  toastCreateProjectOutcome({
    project: PROJECT,
    repositories: COMPLETE,
    repositoriesError: null,
  });
  const shown = await lastToast();
  assert.equal(shown.type, "success");
  assert.match(
    String(shown.title),
    /Code cloned to \/Users\/andy\/Code\/rpg-test and recorded as this project's folder\./,
  );
  assert.match(String(shown.title), /2 agents added to the project roster\./);
});

test("a reused checkout, a missing folder and a roster failure are each said in the host's words", async () => {
  const { toastCreateProjectOutcome } = await import(
    "./toastCreateProjectOutcome.ts"
  );
  toastCreateProjectOutcome({
    project: PROJECT,
    repositories: {
      ...COMPLETE,
      checkoutCloned: false,
      rosterAdded: ["2".repeat(64)],
    },
    repositoriesError: null,
  });
  let shown = await lastToast();
  assert.match(
    String(shown.title),
    /Code already checked out at \/Users\/andy\/Code\/rpg-test; recorded as this project's folder\. 1 agent added to the project roster\./,
  );

  toastCreateProjectOutcome({
    project: PROJECT,
    repositories: {
      ...COMPLETE,
      complete: false,
      gap: "code repository rpg-test not cloned: no route to the relay",
      checkoutPath: null,
      checkoutCloned: false,
      checkoutError: "no route to the relay",
      rosterAdded: [],
      rosterError: "restricted: only the project owner may write the roster",
    },
    repositoriesError: null,
  });
  shown = await lastToast();
  assert.equal(shown.type, "warning");
  assert.match(
    String(shown.title),
    /No folder was recorded for this project: no route to the relay\. 0 agents added to the project roster\. Roster error: restricted: only the project owner may write the roster\./,
  );
});
