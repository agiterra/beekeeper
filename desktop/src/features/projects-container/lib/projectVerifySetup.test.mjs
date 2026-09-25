import assert from "node:assert/strict";
import test from "node:test";

import {
  DEFAULT_VERIFY_COMMAND_TEXT,
  decodeProjectVerifySetup,
  parseVerifyCommand,
  verifyConsentCase,
  verifyConsentCaseSentence,
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

test("consent is offered only when every setup step resolved, and says which did not", () => {
  const done = decodeProjectVerifySetup({
    workflowId: "wf",
    channelId: "chan",
    definitionHash: "c".repeat(64),
    command: ["python3", "-m", "unittest"],
    publishEventId: "e1",
    channelReused: false,
    checkout: "d".repeat(40),
    grantEventId: "g1",
    grantError: null,
    error: null,
  });
  assert.equal(verifyConsentUnavailable(done, null), null);
  assert.match(
    verifyConsentUnavailable(
      {
        ...done,
        workflowId: null,
        error: "the verify action was not published: x",
      },
      null,
    ),
    /not published: x/,
  );
  assert.match(verifyConsentUnavailable(null, "no host"), /no host/);
  assert.match(verifyConsentUnavailable(null, null), /not published/);
  assert.throws(() => decodeProjectVerifySetup({ workflowId: 1 }));
});

test("setup no longer reports a run or trigger — a response naming neither still decodes", () => {
  // Before ledger 252's control-run-6 fix, decodeProjectVerifySetup required
  // `runId` and `triggerEventId` to be present (even as `null`) or it threw.
  // Setup now starts no run and publishes no trigger, so the host response
  // never carries either field at all.
  const decoded = decodeProjectVerifySetup({
    workflowId: "wf",
    channelId: "chan",
    definitionHash: "c".repeat(64),
    command: [],
    publishEventId: "e1",
    channelReused: false,
    checkout: "d".repeat(40),
    grantEventId: null,
    grantError: null,
    error: null,
  });
  assert.equal(decoded.workflowId, "wf");
  assert.equal(decoded.definitionHash, "c".repeat(64));
});

/**
 * Brian's 2026-09-25 ruling: creation is consent, so setup's own grant
 * hides the card the instant it exists — the card is left only for a host
 * that holds none (`"not-created"`), or one whose grant no longer matches
 * the workflow's current hash (`"changed"`).
 */
test("verifyConsentCase hides once setup's own grant exists, and names the surviving case otherwise", () => {
  const granted = {
    workflowId: "wf",
    channelId: "chan",
    definitionHash: "c".repeat(64),
    command: [],
    publishEventId: "e1",
    channelReused: false,
    checkout: "d".repeat(40),
    grantEventId: "g1",
    grantError: null,
    error: null,
  };
  assert.equal(verifyConsentCase(granted), null);

  const ungranted = { ...granted, grantEventId: null };
  assert.equal(verifyConsentCase(ungranted), "not-created");
  assert.equal(verifyConsentCase(null), "not-created");

  // A live hash independent of what setup itself recorded — e.g. the
  // Actions tab's own autorun read — can tell "changed" apart from
  // "not-created" even though setup's own result looks identical either way.
  assert.equal(
    verifyConsentCase(ungranted, "d".repeat(64)),
    "changed",
    "a live hash different from verify.definitionHash is the changed case",
  );
  assert.equal(
    verifyConsentCase(ungranted, "c".repeat(64)),
    "not-created",
    "a live hash equal to verify.definitionHash is still not-created",
  );

  assert.match(verifyConsentCaseSentence("changed"), /changed/);
  assert.match(verifyConsentCaseSentence("not-created"), /no standing grant/);
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
