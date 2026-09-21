import assert from "node:assert/strict";
import { test } from "node:test";

import {
  projectSetupIncomplete,
  projectSetupSteps,
} from "./projectSetupOutcome.ts";

const OWNER = "a".repeat(64);
const REF = `30621:${OWNER}:kettle-smoke`;

/** The host's report for a create that did everything. */
const COMPLETE = {
  projectRef: REF,
  codeRepoRef: `30617:${OWNER}:kettle-smoke`,
  codeRepoId: "kettle-smoke",
  codeAnnouncementEventId: "c".repeat(64),
  codeRepoExisted: false,
  agentsRepoRef: `30617:${OWNER}:kettle-smoke-beekeeper-agents`,
  agentsRepoId: "kettle-smoke-beekeeper-agents",
  agentsCloneUrl: "https://hive.example/git/x",
  agentsAnnouncementEventId: "d".repeat(64),
  agentsRepoExisted: false,
  branch: "main",
  roles: ["lead", "builder"],
  seedCommitSha: "e".repeat(40),
  seedError: null,
  seedSkipped: false,
  pushed: true,
  pushError: null,
  pushRecordEventId: null,
  sourceEventId: "f".repeat(64),
  sourceExisted: false,
  publicationError: null,
  commitIdentityName: "Brian",
  commitIdentityEmail: "b@example.com",
  agentsAnnouncementWithdrawnEventId: null,
  agentsAnnouncementWithdrawalError: null,
  complete: true,
  gap: null,
  agentsInstalled: [
    { role: "lead", name: "Lead 2", pubkey: "1".repeat(64), refreshed: false },
  ],
  agentsError: null,
  codeSeedCommitSha: "a".repeat(40),
  codeSeedSkipped: false,
  codeSeedError: null,
  checkoutPath: "/Users/brian/Projects/Kettle Smoke/kettle-smoke",
  checkoutCloned: true,
  checkoutError: null,
  rosterAdded: ["1".repeat(64)],
  rosterError: null,
};

const outcome = (result, error = null) => ({
  projectRef: REF,
  projectName: "Kettle Smoke",
  at: "2026-09-20T15:00:00Z",
  result,
  error,
});

// Ledger 207(5): every step creation runs is on the card, readable.
test("a complete create lists all eight steps, none failed", () => {
  const steps = projectSetupSteps(outcome(COMPLETE));
  assert.equal(steps.length, 8);
  assert.deepEqual(
    steps.filter((step) => step.failed),
    [],
  );
  assert.equal(projectSetupIncomplete(outcome(COMPLETE)), false);
  const checkout = steps.find((step) => step.id === "checkout");
  assert.match(checkout.detail, /Kettle Smoke\/kettle-smoke/);
});

test("failures sort first and keep the host's own words", () => {
  const steps = projectSetupSteps(
    outcome({
      ...COMPLETE,
      complete: false,
      gap: "the agents repository was not seeded",
      checkoutPath: null,
      checkoutCloned: false,
      checkoutError: "clone refused: could not read Username",
      rosterError: "relay error 403",
    }),
  );
  assert.deepEqual(
    steps.slice(0, 2).map((step) => step.id),
    ["checkout", "roster"],
  );
  assert.match(steps[0].detail, /could not read Username/);
  assert.match(steps[1].detail, /relay error 403/);
  assert.equal(projectSetupIncomplete(outcome(COMPLETE, null)), false);
});

test("a create whose command threw is one failed step with its reason", () => {
  const steps = projectSetupSteps(outcome(null, "no host"));
  assert.deepEqual(
    steps.map((step) => [step.id, step.failed]),
    [["repositories", true]],
  );
  assert.match(steps[0].detail, /no host/);
  assert.equal(projectSetupIncomplete(outcome(null, "no host")), true);
});

test("an unreported step reads as not done, never as done", () => {
  const steps = projectSetupSteps(
    outcome({
      ...COMPLETE,
      complete: false,
      gap: "source not set",
      sourceEventId: null,
      sourceExisted: false,
      publicationError: null,
    }),
  );
  const source = steps.find((step) => step.id === "source");
  assert.equal(source.failed, true);
  assert.equal(source.detail, "not set");
});
