import assert from "node:assert/strict";
import test from "node:test";

import {
  matchBoundDefinition,
  readBoundDefinition,
} from "./resolveBoundDefinition.ts";
import { buildHostStepApprovalView } from "./hostStepApproval.ts";

const HASH_A = "aa".repeat(32);
const HASH_B = "bb".repeat(32);
const SHA = "e6".repeat(20);

/** A definition body, and the hash of *these* bytes. Never a loose pair. */
function published(command, hash) {
  return {
    definition: {
      steps: [{ id: "verify", action: "run_on_host", command }],
    },
    definitionHash: hash,
  };
}

test("R1: a body and a hash from two reads cannot authorize anything", async () => {
  // Astra's interleaving: run A waits; B is current; the body read returns
  // B; A is republished; the hash read returns A. Two reads joined by a
  // comparison agree, and the owner is shown B while the relay grants A.
  const read = await readBoundDefinition("wf-1", {
    // One read, so the fixture cannot even express the interleaving: the
    // hash is of the bytes returned beside it.
    readDefinition: async () => published(["echo", "B"], HASH_B),
  });
  const resolution = matchBoundDefinition({
    runHash: HASH_A,
    read,
    stepId: "verify",
  });
  assert.equal(resolution.kind, "not-current");

  const view = buildHostStepApprovalView({
    request: {
      approvalRef: "ab".repeat(32),
      runId: "11111111-2222-4333-8444-555555555555",
      workflowName: "verify",
      stepId: "verify",
      stepIndex: 0,
      approverSpec: `project-owner:30621:${"3d".repeat(32)}:kettle`,
      message: null,
      expiresAt: null,
    },
    runDefinitionHash: HASH_A,
    runRead: true,
    definition: resolution,
    checkout: { state: "commit", sha: SHA },
  });
  assert.equal(view.command.value, null, "command B is never displayed");
  assert.equal(view.grantAvailable, false, "Approve stays unavailable");
});

test("R1: the hash the surface trusts is of the bytes it displays", async () => {
  const read = await readBoundDefinition("wf-1", {
    readDefinition: async () => published(["just", "ci"], HASH_A),
  });
  const resolution = matchBoundDefinition({
    runHash: HASH_A,
    read,
    stepId: "verify",
  });
  assert.equal(resolution.kind, "resolved");
  assert.deepEqual(resolution.command, {
    form: "argv",
    argv: ["just", "ci"],
  });
  // The resolved hash is the read's, which is the hash of the returned body.
  assert.equal(resolution.hash, HASH_A);
});

test("R1: a failed single read resolves nothing and grants nothing", async () => {
  const read = await readBoundDefinition("wf-1", {
    readDefinition: async () => {
      throw new Error("relay said 403");
    },
  });
  const resolution = matchBoundDefinition({
    runHash: HASH_A,
    read,
    stepId: "verify",
  });
  assert.equal(resolution.kind, "unread");
  assert.match(resolution.reason, /403/);
});

test("R1: a read that returns no hash is unread, never a match", async () => {
  const read = await readBoundDefinition("wf-1", {
    readDefinition: async () => ({
      definition: { steps: [] },
      definitionHash: null,
    }),
  });
  assert.equal(
    matchBoundDefinition({ runHash: HASH_A, read, stepId: "verify" }).kind,
    "unread",
  );
});
