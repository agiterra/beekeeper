import assert from "node:assert/strict";
import test from "node:test";

import { describeActionRun } from "./actionRunLabel.ts";

const HOST = "953d0000000000000000000000000000000000000000000000000000000000ab";
const APPROVER =
  "deadbeef00000000000000000000000000000000000000000000000000000001";
const NOW = 1_800_000_000;
const clock = { nowSeconds: NOW, formatTime: (s) => `t${s}` };

function run(overrides = {}) {
  return {
    id: "run-1",
    workflowId: "wf-1",
    status: "waiting_host",
    currentStep: 0,
    executionTrace: [],
    startedAt: NOW - 100,
    completedAt: null,
    errorCode: null,
    errorMessage: null,
    createdAt: NOW - 100,
    ...overrides,
  };
}

function approval(overrides = {}) {
  return {
    approvalRef: "ab".repeat(32),
    workflowId: "wf-1",
    runId: "run-1",
    stepId: "build",
    stepIndex: 0,
    approverSpec: APPROVER,
    status: "pending",
    approverPubkey: null,
    note: null,
    expiresAt: new Date((NOW + 3_600) * 1_000).toISOString(),
    createdAt: NOW - 90,
    ...overrides,
  };
}

function hostStep(overrides = {}) {
  return {
    runId: "run-1",
    stepId: "build",
    workflowId: "wf-1",
    stepIndex: 0,
    status: "requested",
    requestedEventId: "11".repeat(32),
    expiresAt: new Date((NOW + 3_600) * 1_000).toISOString(),
    claimedBy: null,
    claimedAt: null,
    claimEventId: null,
    resultEventId: null,
    exitedEventId: null,
    exitCode: null,
    disposition: null,
    timedOut: null,
    durationMs: null,
    headSha: null,
    dirty: null,
    artifactRef: null,
    routedAgent: null,
    routedCommandId: null,
    routedHiredRole: null,
    artifacts: [],
    exitedAt: null,
    createdAt: new Date((NOW - 80) * 1_000).toISOString(),
    ...overrides,
  };
}

test("a live pending approval reads 'waiting approval (since …)' and carries the approval", () => {
  const pending = approval();
  const row = describeActionRun(
    run({ status: "waiting_approval" }),
    [pending],
    [],
    clock,
  );
  assert.equal(row.label, `waiting approval (since t${NOW - 90})`);
  assert.equal(row.tone, "pending");
  assert.equal(row.pendingApproval, pending);
});

test("an expired pending approval no longer offers Approve / Deny", () => {
  const stale = approval({
    expiresAt: new Date((NOW - 1) * 1_000).toISOString(),
  });
  const row = describeActionRun(
    run({ status: "waiting_approval" }),
    [stale],
    [],
    clock,
  );
  assert.equal(row.pendingApproval, null);
  assert.equal(row.label, "waiting approval");
});

test("a granted approval with no host record yet reads 'approved by <short>'", () => {
  const row = describeActionRun(
    run({ status: "running" }),
    [approval({ status: "granted", approverPubkey: APPROVER })],
    [],
    clock,
  );
  assert.equal(row.label, "approved by deadbeef…0001");
  assert.equal(row.tone, "ok");
});

test("a requested host step reads 'requested on host (since …)'", () => {
  const row = describeActionRun(run(), [], [hostStep()], clock);
  assert.equal(row.label, `requested on host (since t${NOW - 80})`);
  assert.equal(row.tone, "pending");
});

test("a claimed host step never reads 'running'", () => {
  const row = describeActionRun(
    run(),
    [],
    [hostStep({ status: "claimed", claimedBy: HOST })],
    clock,
  );
  assert.equal(row.label, "claimed by 953d0000…00ab, no result yet");
  assert.doesNotMatch(row.label, /running/);
});

test("an exited host step names the code, host, sha and dirtiness", () => {
  const exited = hostStep({
    status: "exited",
    claimedBy: HOST,
    disposition: "exited",
    exitCode: 1,
    headSha: "0123456789abcdef0123456789abcdef01234567",
    dirty: true,
  });
  const row = describeActionRun(run(), [], [exited], clock);
  assert.equal(row.label, "exited 1 on 953d0000…00ab at 0123456, dirty");
  assert.equal(row.tone, "bad");

  const clean = describeActionRun(
    run({ status: "completed" }),
    [approval({ status: "granted", approverPubkey: APPROVER })],
    [{ ...exited, exitCode: 0, dirty: false }],
    clock,
  );
  assert.equal(clean.label, "completed · exited 0 on 953d0000…00ab at 0123456");
  assert.equal(clean.tone, "ok");
});

test("timed out, lost and refused steps read exactly that", () => {
  const timedOut = describeActionRun(
    run({ status: "failed", errorCode: "host_step_failed" }),
    [],
    [
      hostStep({
        status: "exited",
        claimedBy: HOST,
        disposition: "timed_out",
        exitCode: 124,
        timedOut: true,
      }),
    ],
    clock,
  );
  assert.equal(
    timedOut.label,
    "failed: host_step_failed · timed out on 953d0000…00ab",
  );

  const lost = describeActionRun(
    run(),
    [],
    [hostStep({ status: "lost", claimedBy: HOST })],
    clock,
  );
  assert.equal(lost.label, "lost on host restart");

  const refused = describeActionRun(
    run({
      executionTrace: [
        {
          stepId: "build",
          status: "completed",
          output: {
            disposition: "refused",
            refusal_code: "ACTION_DEFINITION_DRIFT",
          },
          startedAt: null,
          completedAt: null,
          error: null,
        },
      ],
    }),
    [],
    [hostStep({ status: "exited", claimedBy: HOST, disposition: "refused" })],
    clock,
  );
  assert.equal(refused.label, "refused: ACTION_DEFINITION_DRIFT");
  assert.equal(refused.tone, "bad");
});

test("the latest host step wins when a run has several", () => {
  const first = hostStep({
    stepIndex: 0,
    stepId: "build",
    status: "exited",
    claimedBy: HOST,
    disposition: "exited",
    exitCode: 0,
  });
  const second = hostStep({
    stepIndex: 2,
    stepId: "deploy",
    status: "claimed",
    claimedBy: HOST,
  });
  const row = describeActionRun(run(), [], [second, first], clock);
  assert.equal(row.label, "claimed by 953d0000…00ab, no result yet");
});

test("a run with no approval or host record shows only its own status", () => {
  assert.equal(
    describeActionRun(run({ status: "pending" }), [], [], clock).label,
    "queued",
  );
  assert.equal(
    describeActionRun(run({ status: "completed" }), [], [], clock).label,
    "completed",
  );
  assert.equal(
    describeActionRun(run({ status: "failed", errorCode: "E1" }), [], [], clock)
      .label,
    "failed: E1",
  );
  assert.equal(
    describeActionRun(run({ status: "cancelled" }), [], [], clock).tone,
    "muted",
  );
});

test("a routed wake step reads as routed to the agent with its turn", () => {
  const routed = describeActionRun(
    run({ status: "running" }),
    [],
    [
      hostStep({
        status: "exited",
        claimedBy: HOST,
        disposition: "exited",
        exitCode: 0,
        routedAgent: "Levain",
        routedCommandId: `action-route-${"f".repeat(64)}`,
      }),
    ],
    clock,
  );
  assert.equal(routed.label, "routed to Levain · turn action-route-ffffffff");
  assert.equal(routed.tone, "ok");
});

test("a hire step reads as hired into the agent's session", () => {
  const hired = describeActionRun(
    run({ status: "running" }),
    [],
    [
      hostStep({
        status: "exited",
        claimedBy: HOST,
        disposition: "exited",
        exitCode: 0,
        routedAgent: "Keystone",
        routedHiredRole: "runner",
        routedCommandId: `action-hire-${"e".repeat(64)}`,
      }),
    ],
    clock,
  );
  assert.equal(
    hired.label,
    "hired a runner into Keystone's session · hire action-hire-eeeeeeee",
  );
  assert.equal(hired.tone, "ok");
});
