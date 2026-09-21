import assert from "node:assert/strict";
import test from "node:test";

import { runErrorSentence } from "./actionRunLabel.ts";
import { checkoutSentence, runProvenanceFacts } from "./actionRunProvenance.ts";
import {
  hostStepCommand,
  isFullCommitSha,
  requiredCheckoutStepIds,
} from "./actionDefinition.ts";

const SHA = "e6".repeat(20);

function run(overrides = {}) {
  return {
    id: "run-1",
    workflowId: "wf-1",
    workflowName: "verify",
    triggerEventId: null,
    triggerAuthor: null,
    definitionHash: null,
    status: "completed",
    currentStep: 0,
    executionTrace: [],
    startedAt: null,
    completedAt: null,
    errorCode: null,
    errorMessage: null,
    createdAt: 1,
    ...overrides,
  };
}

function step(overrides = {}) {
  return {
    runId: "run-1",
    stepId: "verify",
    workflowId: "wf-1",
    stepIndex: 0,
    status: "exited",
    requestedEventId: null,
    expiresAt: "2026-09-20T00:00:00Z",
    claimedBy: null,
    claimedAt: null,
    claimEventId: null,
    resultEventId: null,
    exitedEventId: null,
    exitCode: 0,
    disposition: "exited",
    timedOut: null,
    durationMs: 19,
    headSha: SHA,
    dirty: false,
    artifactRef: null,
    routedAgent: null,
    routedCommandId: null,
    routedHiredRole: null,
    artifacts: [],
    exitedAt: null,
    createdAt: "2026-09-20T00:00:00Z",
    checkout: null,
    ...overrides,
  };
}

test("an absent trigger author and definition binding are named, not blank", () => {
  const facts = runProvenanceFacts(run(), [step()], null);
  const by = Object.fromEntries(facts.map((f) => [f.label, f]));
  assert.equal(by["Triggered by"].value, null);
  assert.match(by["Triggered by"].reason, /names no trigger author/);
  assert.equal(by.Definition.value, null);
  assert.match(by.Definition.reason, /before runs were bound to a definition/);
});

test("a pre-184 result says it does not record its checkout", () => {
  const answer = checkoutSentence(step({ checkout: null }));
  assert.equal(answer.value, null);
  assert.match(answer.reason, /predates the checkout record/);
});

test("a bound checkout names mode, sha, head before and dirtiness", () => {
  const answer = checkoutSentence(
    step({
      checkout: {
        mode: `commit ${SHA}`,
        sha: SHA,
        headShaBefore: "ab".repeat(20),
        dirtyBefore: false,
      },
    }),
  );
  assert.match(answer.value, new RegExp(`commit ${SHA}`));
  assert.match(answer.value, /clean before/);
  assert.equal(answer.reason, null);
});

test("an unreadable host-step listing never reads as 'no host step'", () => {
  const facts = runProvenanceFacts(run(), [], "relay said 403");
  const hostStep = facts.find((f) => f.label === "Host step");
  assert.match(hostStep.reason, /could not be read: relay said 403/);
  assert.equal(
    facts.some((f) => f.label === "Exit code"),
    false,
  );

  const none = runProvenanceFacts(run(), [], null);
  assert.match(
    none.find((f) => f.label === "Host step").reason,
    /recorded no host step/,
  );
});

test("the two definition-binding stops read as words, not codes", () => {
  assert.match(
    runErrorSentence("definition_changed", null),
    /definition changed after the run was created/,
  );
  assert.match(
    runErrorSentence("definition_unknown", null),
    /names no definition/,
  );
  assert.equal(runErrorSentence(null, "boom"), "failed: boom");
});

test("a required checkout is read off the step, and the command verbatim", () => {
  const definition = {
    steps: [
      {
        id: "verify",
        action: "run_on_host",
        command: ["just", "ci"],
        checkout: "required",
      },
      { id: "note", action: "send_message", text: "hi" },
      { id: "loose", action: "run_on_host", command: ["ls"] },
    ],
  };
  assert.deepEqual(requiredCheckoutStepIds(definition), ["verify"]);
  assert.deepEqual(hostStepCommand(definition, "verify"), {
    form: "argv",
    argv: ["just", "ci"],
  });
  assert.equal(hostStepCommand(definition, "note"), null);
  assert.equal(hostStepCommand(definition, "missing"), null);
  assert.equal(requiredCheckoutStepIds({}).length, 0);
});

test("only a full 40-hex sha may be bound", () => {
  assert.equal(isFullCommitSha(SHA), true);
  assert.equal(isFullCommitSha(` ${SHA.toUpperCase()} `), true);
  assert.equal(isFullCommitSha("e6".repeat(19)), false);
  assert.equal(isFullCommitSha("zz".repeat(20)), false);
});
