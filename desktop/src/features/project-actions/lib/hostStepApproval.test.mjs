import assert from "node:assert/strict";
import test from "node:test";

import {
  buildHostStepApprovalView,
  readHostStepApprovalRequest,
  resolveApprovalAuthority,
} from "./hostStepApproval.ts";

const REF = "ab".repeat(32);
const OWNER = "3d".repeat(32);

function event(overrides = {}) {
  const content = {
    schema: "buzz-approval-request/v1",
    runId: "11111111-2222-4333-8444-555555555555",
    workflowId: "99999999-2222-4333-8444-555555555555",
    workflowName: "verify",
    stepId: "verify",
    stepIndex: 1,
    approverSpec: "owner",
    message: "Run just ci?",
    expiresAt: 1_800_000_000,
    synthetic: true,
    ...(overrides.content ?? {}),
  };
  return {
    kind: overrides.kind ?? 46010,
    tags: overrides.tags ?? [
      ["d", REF],
      ["h", "chan"],
      ["p", OWNER],
    ],
    content: overrides.raw ?? JSON.stringify(content),
  };
}

test("a well-formed request reads into its five facts", () => {
  const request = readHostStepApprovalRequest(event());
  assert.equal(request.approvalRef, REF);
  assert.equal(request.stepId, "verify");
  assert.equal(request.stepIndex, 1);
  assert.equal(request.synthetic, true);
  assert.equal(request.channelId, "chan");
});

test("a shape this reader cannot fully recognise carries no control", () => {
  assert.equal(readHostStepApprovalRequest(event({ kind: 1 })), null);
  assert.equal(readHostStepApprovalRequest(event({ raw: "not json" })), null);
  assert.equal(
    readHostStepApprovalRequest(event({ content: { schema: "other/v1" } })),
    null,
  );
  assert.equal(
    readHostStepApprovalRequest(event({ tags: [["h", "chan"]] })),
    null,
  );
  assert.equal(
    readHostStepApprovalRequest(event({ content: { stepId: "   " } })),
    null,
  );
});

test("every fact it cannot establish is named, never blank", () => {
  const view = buildHostStepApprovalView({
    request: readHostStepApprovalRequest(event()),
    runDefinitionHash: null,
    runRead: true,
    definition: { kind: "hash-unknown" },
    checkout: { state: "not-reported" },
  });
  assert.equal(view.definitionHash.value, null);
  assert.match(view.definitionHash.reason, /no definition binding/);
  assert.equal(view.command.value, null);
  assert.match(view.command.reason, /names no definition/);
  assert.equal(view.boundCommit.value, null);
  assert.match(view.boundCommit.reason, /does not report a run's bound commit/);
  assert.equal(view.grantAvailable, false);
});

test("an unread run and an unbound run are different sentences", () => {
  const request = readHostStepApprovalRequest(event());
  const unread = buildHostStepApprovalView({
    request,
    runDefinitionHash: null,
    runRead: false,
    definition: { kind: "unread", reason: "the read has not settled" },
    checkout: { state: "not-reported" },
  });
  assert.match(unread.definitionHash.reason, /has not been read yet/);
  assert.match(unread.command.reason, /could not be read/);
});

test("the run's own binding is shown, and the command verbatim", () => {
  const view = buildHostStepApprovalView({
    request: readHostStepApprovalRequest(event()),
    runDefinitionHash: "cd".repeat(32),
    runRead: true,
    definition: {
      kind: "resolved",
      hash: "cd".repeat(32),
      command: { form: "shell", text: "just ci" },
    },
    checkout: { state: "commit", sha: "e6".repeat(20) },
  });
  assert.equal(view.definitionHash.value, "cd".repeat(32));
  assert.equal(view.command.value, "just ci");
  assert.equal(view.boundCommit.value, "e6".repeat(20));
  assert.equal(view.actionName, "verify");
  assert.equal(view.grantAvailable, true);
});

test("only a project owner is offered an answer", () => {
  const coordinate = `30621:${OWNER}:kettle`;
  const owner = resolveApprovalAuthority({
    viewerPubkey: OWNER.toUpperCase(),
    approverSpec: `project-owner:${coordinate}`,
    roster: [],
    rosterRead: true,
  });
  assert.equal(owner.canApprove, true);

  const other = resolveApprovalAuthority({
    viewerPubkey: "11".repeat(32),
    approverSpec: `project-owner:${coordinate}`,
    roster: [],
    rosterRead: true,
  });
  assert.equal(other.canApprove, false);
  assert.match(other.sentence, /never delegated/);

  const noIdentity = resolveApprovalAuthority({
    viewerPubkey: null,
    approverSpec: `project-owner:${coordinate}`,
    roster: [],
    rosterRead: true,
  });
  assert.equal(noIdentity.canApprove, false);
  assert.match(noIdentity.sentence, /unknown/);
});
