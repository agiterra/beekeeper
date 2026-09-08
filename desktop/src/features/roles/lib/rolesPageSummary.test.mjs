import assert from "node:assert/strict";
import { test } from "node:test";

import { rolesPageSummary } from "@/features/roles/lib/rolesPageSummary";

const SHA = "9f2e1d0c".padEnd(40, "a");

function seat(overrides = {}) {
  return {
    key: "chan-1/gen-1",
    channelId: "chan-1",
    generationId: "gen-1",
    label: "Build the card",
    agentPubkey: "a".repeat(64),
    agentName: "Ada",
    role: "builder",
    projectId: "p1",
    projectName: "Beekeeper",
    status: "running",
    ageSeconds: 30,
    packSha: SHA,
    ...overrides,
  };
}

function agent(overrides = {}) {
  return {
    pubkey: "a".repeat(64),
    name: "Ada",
    hasRolePack: true,
    packRefusedSharedHome: false,
    status: "running",
    avatarUrl: null,
    runtime: "goose",
    model: "sonnet",
    ...overrides,
  };
}

function role(overrides = {}) {
  return { role: "builder", agents: [], seats: [], ...overrides };
}

function view(overrides = {}) {
  return { roles: [], byProject: [], unplaced: [], ...overrides };
}

function snapshots(reported = []) {
  return {
    resolvedAt: null,
    resolved: [],
    reported,
    comparison: null,
    provenanceNotes: [],
  };
}

function report(provenance) {
  return { provenance };
}

test("the role count is the number of cards on the page", () => {
  const summary = rolesPageSummary(
    view({ roles: [role(), role({ role: "lead" })] }),
    snapshots(),
  );
  assert.equal(summary.roles, 2);
});

test("an agent is counted once, however many cards could show it", () => {
  const summary = rolesPageSummary(
    view({
      roles: [
        role({ agents: [agent(), agent({ pubkey: "b".repeat(64) })] }),
        // The same agent again: the count is a set size, not a sum of lists.
        role({ role: "lead", agents: [agent()] }),
      ],
    }),
    snapshots(),
  );
  assert.equal(summary.agents, 2);
});

test("a shared agent is counted apart from the ones set up on this computer", () => {
  const summary = rolesPageSummary(
    view({
      roles: [
        role({
          agents: [
            agent(),
            agent({ pubkey: "b".repeat(64), isManagedHere: false }),
            agent({ pubkey: "c".repeat(64), isManagedHere: true }),
          ],
        }),
      ],
    }),
    snapshots(),
  );
  assert.equal(summary.agents, 3);
  assert.equal(summary.agentsShared, 1);
  // An agent nobody said anything about is local, and the two always sum.
  assert.equal(summary.agentsLocal, 2);
  assert.equal(summary.agentsLocal + summary.agentsShared, summary.agents);
});

test("a seat filed under both a role and a project is one open session", () => {
  const only = seat();
  const summary = rolesPageSummary(
    view({
      roles: [role({ seats: [only] })],
      byProject: [{ projectId: "p1", projectName: "Beekeeper", seats: [only] }],
    }),
    snapshots(),
  );
  assert.equal(summary.openSessions, 1);
});

test("a session whose own status says it is over is not open", () => {
  const summary = rolesPageSummary(
    view({
      roles: [
        role({
          seats: [
            seat(),
            seat({ key: "c/2", status: "completed" }),
            seat({ key: "c/3", status: "stopped" }),
            seat({ key: "c/4", status: "failed" }),
            // Everything else is still open, including the ones that say
            // nothing useful — absence of a live word is not a finish.
            seat({ key: "c/5", status: "waiting_for_input" }),
            seat({ key: "c/6", status: "disconnected" }),
            seat({ key: "c/7", status: "unknown" }),
          ],
        }),
      ],
    }),
    snapshots(),
  );
  assert.equal(summary.openSessions, 4);
});

test("unplaced seats are counted too, and only once", () => {
  const orphan = seat({ key: "c/9", projectId: null, projectName: null });
  const summary = rolesPageSummary(
    view({ roles: [role({ seats: [orphan] })], unplaced: [orphan] }),
    snapshots(),
  );
  assert.equal(summary.openSessions, 1);
});

test("the report counts come from the reports themselves, split by what was checked", () => {
  const summary = rolesPageSummary(
    view(),
    snapshots([
      report("commissioned"),
      report("proof-unavailable"),
      report("proof-unavailable"),
      report("disputed"),
    ]),
  );
  assert.equal(summary.reports, 4);
  assert.equal(summary.unconfirmed, 2);
  assert.equal(summary.disputed, 1);
});

test("an empty project counts to zero rather than to nothing", () => {
  const summary = rolesPageSummary(view(), snapshots());
  assert.deepEqual(summary, {
    roles: 0,
    agents: 0,
    agentsLocal: 0,
    agentsShared: 0,
    openSessions: 0,
    reports: 0,
    unconfirmed: 0,
    disputed: 0,
  });
});
