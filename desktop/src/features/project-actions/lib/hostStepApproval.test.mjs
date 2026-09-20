import assert from "node:assert/strict";
import test from "node:test";

import {
  approvalAuthority,
  buildHostStepApprovalView,
  readHostStepApprovalRequest,
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
    command: null,
    definitionRead: true,
    boundCommit: null,
  });
  assert.equal(view.definitionHash.value, null);
  assert.match(view.definitionHash.reason, /no definition binding/);
  assert.equal(view.command.value, null);
  assert.match(view.command.reason, /names no command for step verify/);
  assert.equal(view.boundCommit.value, null);
  assert.match(view.boundCommit.reason, /no record of this run names a commit/);
});

test("an unread run and an unbound run are different sentences", () => {
  const request = readHostStepApprovalRequest(event());
  const unread = buildHostStepApprovalView({
    request,
    runDefinitionHash: null,
    runRead: false,
    command: null,
    definitionRead: false,
    boundCommit: null,
  });
  assert.match(unread.definitionHash.reason, /has not been read yet/);
  assert.match(unread.command.reason, /has not been read yet/);
});

test("the run's own binding is shown, and the command verbatim", () => {
  const view = buildHostStepApprovalView({
    request: readHostStepApprovalRequest(event()),
    runDefinitionHash: "cd".repeat(32),
    runRead: true,
    command: "just ci",
    definitionRead: true,
    boundCommit: "e6".repeat(20),
  });
  assert.equal(view.definitionHash.value, "cd".repeat(32));
  assert.equal(view.command.value, "just ci");
  assert.equal(view.boundCommit.value, "e6".repeat(20));
  assert.equal(view.actionName, "verify");
});

test("only the project owner is offered an answer", () => {
  const owner = approvalAuthority({
    viewerPubkey: OWNER.toUpperCase(),
    projectOwner: OWNER,
    approverSpec: "owner",
  });
  assert.equal(owner.canApprove, true);

  const other = approvalAuthority({
    viewerPubkey: "11".repeat(32),
    projectOwner: OWNER,
    approverSpec: "owner",
  });
  assert.equal(other.canApprove, false);
  assert.match(other.sentence, /never delegated/);

  for (const missing of [
    { viewerPubkey: null, projectOwner: OWNER },
    { viewerPubkey: OWNER, projectOwner: null },
  ]) {
    const answer = approvalAuthority({ ...missing, approverSpec: null });
    assert.equal(answer.canApprove, false);
    assert.match(answer.sentence, /unknown/);
  }
});
