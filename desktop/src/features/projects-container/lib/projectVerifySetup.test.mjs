import assert from "node:assert/strict";
import test from "node:test";

import {
  DEFAULT_VERIFY_COMMAND_TEXT,
  decodeProjectVerifySetup,
  findApprovalRequestForRun,
  parseVerifyCommand,
  verifyConsentUnavailable,
} from "./projectVerifySetup.ts";

/**
 * Ledger 248: the creation form's verify command is argv, never a shell
 * line; it defaults to the kettle gate and an empty field means the default.
 */
test("the verify command field parses to argv, defaulting to the kettle gate", () => {
  assert.deepEqual(parseVerifyCommand(DEFAULT_VERIFY_COMMAND_TEXT), [
    "python3",
    "-m",
    "unittest",
    "discover",
    "-s",
    "tests",
  ]);
  assert.deepEqual(parseVerifyCommand("   "), null);
  assert.deepEqual(parseVerifyCommand(`sh -c "just ci && echo ok"`), [
    "sh",
    "-c",
    "just ci && echo ok",
  ]);
  assert.deepEqual(parseVerifyCommand("pytest 'tests/a b'"), [
    "pytest",
    "tests/a b",
  ]);
});

const RUN = "5d6f1f7e-1c1b-4a53-9a8e-3a6f2d2b7c10";
function request(runId, ref) {
  return {
    kind: 46010,
    tags: [
      ["d", ref],
      ["h", "chan"],
    ],
    content: JSON.stringify({
      schema: "buzz-approval-request/v1",
      runId,
      workflowId: "wf",
      stepId: "verify",
      synthetic: true,
    }),
  };
}

test("the consent card is the 46010 of setup's own run, never another run's", () => {
  const other = request("00000000-0000-4000-8000-000000000000", "a".repeat(64));
  const mine = request(RUN, "b".repeat(64));
  assert.equal(findApprovalRequestForRun([other, mine], RUN), mine);
  assert.equal(findApprovalRequestForRun([other], RUN), null);
  assert.equal(findApprovalRequestForRun([mine], null), null);
});

test("consent is offered only when every setup step resolved, and says which did not", () => {
  const done = decodeProjectVerifySetup({
    workflowId: "wf",
    channelId: "chan",
    definitionHash: "c".repeat(64),
    command: ["python3", "-m", "unittest"],
    publishEventId: "e1",
    channelReused: false,
    checkout: "d".repeat(40),
    runId: RUN,
    triggerEventId: "e2",
    error: null,
  });
  assert.equal(verifyConsentUnavailable(done, null), null);
  assert.match(
    verifyConsentUnavailable(
      { ...done, runId: null, error: "the verify run was not started: x" },
      null,
    ),
    /not started: x/,
  );
  assert.match(verifyConsentUnavailable(null, "no host"), /no host/);
  assert.match(verifyConsentUnavailable(null, null), /not published/);
  assert.throws(() => decodeProjectVerifySetup({ workflowId: 1 }));
});

test("setup publishes nothing unless both seeds reached the relay", async () => {
  const { setupSeededVerify } = await import("../useCreateProjectContainer.ts");
  const project = { address: `30621:${"1".repeat(64)}:kettle` };
  const pushed = {
    seededActionsYml: "schema: x",
    pushed: true,
    codeSeedCommitSha: "d".repeat(40),
    codeSeedError: null,
  };
  for (const gap of [
    { seededActionsYml: null },
    { pushed: false },
    { codeSeedCommitSha: null },
    { codeSeedError: "push refused" },
  ]) {
    assert.deepEqual(await setupSeededVerify(project, { ...pushed, ...gap }), {
      verify: null,
      verifyError: null,
    });
  }
});
