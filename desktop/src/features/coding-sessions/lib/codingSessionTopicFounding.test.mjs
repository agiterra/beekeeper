/**
 * Founding a team topic: the order, what is fatal, and what is only reported.
 *
 * Every relay interaction is injected, so the sequence asserted here is the
 * function's own — not a re-enactment — and nothing here touches a relay, a
 * keyring or this computer's disk.
 */
import assert from "node:assert/strict";
import test from "node:test";

import {
  CODING_SESSION_FOUNDING_CHANNEL_STEP,
  CODING_SESSION_FOUNDING_GENESIS_STEP,
  CODING_SESSION_FOUNDING_GOAL_STEP,
  CODING_SESSION_FOUNDING_NAME_STEP,
  foundCodingSessionTopic,
  planCodingSessionTopicFounding,
} from "./codingSessionTopicFounding.ts";

const CHANNEL_ID = "3d2a7b18-9b7a-4a41-9a86-6a52a1c0b7e1";
const MINTED_CHANNEL_ID = "8f1c2a90-2b7e-4f0d-9a3b-1c4d5e6f7a80";
const SESSION_REF = "11111111-1111-4111-8111-111111111111";

const INPUT = {
  channelId: CHANNEL_ID,
  goal: "Close ledger item 103.",
  title: "Ledger 103",
  projectRef: "30621:owner:project",
  repoRef: "30617:owner:repo",
};

/** Fake publishes that record their order and can be told to refuse. */
function fakeDeps(overrides = {}) {
  const calls = [];
  const emitted = [];
  const deps = {
    newSessionRef: () => SESSION_REF,
    publishGenesis: async (input) => {
      calls.push(["genesis", input]);
      return { eventId: "genesis-event" };
    },
    publishGoal: async (input) => {
      calls.push(["goal", input]);
    },
    publishName: async (input) => {
      calls.push(["name", input]);
    },
    onSteps: (steps) => emitted.push(steps),
    ...overrides,
  };
  return { deps, calls, emitted };
}

test("the plan is channel? → genesis → goal? → name?", () => {
  assert.deepEqual(
    planCodingSessionTopicFounding({
      channelId: CHANNEL_ID,
      goal: "g",
      title: "x",
    }).map((step) => step.id),
    [
      CODING_SESSION_FOUNDING_GENESIS_STEP,
      CODING_SESSION_FOUNDING_GOAL_STEP,
      CODING_SESSION_FOUNDING_NAME_STEP,
    ],
  );
  assert.deepEqual(
    planCodingSessionTopicFounding({
      channelId: null,
      goal: "g",
      title: "  ",
    }).map((step) => step.id),
    [
      CODING_SESSION_FOUNDING_CHANNEL_STEP,
      CODING_SESSION_FOUNDING_GENESIS_STEP,
      CODING_SESSION_FOUNDING_GOAL_STEP,
    ],
  );
  // A click founds with nothing but a genesis: no goal step, no name step.
  assert.deepEqual(
    planCodingSessionTopicFounding({
      channelId: CHANNEL_ID,
      goal: "",
      title: null,
    }).map((step) => step.id),
    [CODING_SESSION_FOUNDING_GENESIS_STEP],
  );
  for (const step of planCodingSessionTopicFounding({
    channelId: null,
    goal: "",
    title: null,
  })) {
    assert.equal(step.state, "pending");
    assert.equal(step.detail, null);
  }
});

test("a founding publishes the genesis, then the goal, then the name, all under one session ref", async () => {
  const { deps, calls, emitted } = fakeDeps({
    ensureChannel: async () => assert.fail("a known channel is not re-minted"),
  });
  const result = await foundCodingSessionTopic(INPUT, deps);
  assert.deepEqual(
    calls.map(([name]) => name),
    ["genesis", "goal", "name"],
  );
  for (const [, input] of calls) {
    assert.equal(input.channelId, CHANNEL_ID);
    assert.equal(input.sessionRef, SESSION_REF);
  }
  assert.equal(calls[1][1].content, INPUT.goal);
  assert.equal(calls[2][1].content, "Ledger 103");
  assert.equal(result.ok, true);
  assert.equal(result.channelId, CHANNEL_ID);
  assert.equal(result.sessionRef, SESSION_REF);
  assert.equal(result.genesisRef, "genesis-event");
  assert.deepEqual(result.goal, { published: true, reason: null });
  assert.deepEqual(result.name, { published: true, reason: null });
  assert.equal(result.projectRef, INPUT.projectRef);
  assert.equal(result.repoRef, INPUT.repoRef);
  assert.equal(result.failedStep, null);
  assert.equal(result.failureReason, null);
  assert.deepEqual(
    result.steps.map((step) => [step.id, step.state]),
    [
      ["genesis", "done"],
      ["goal", "done"],
      ["name", "done"],
    ],
  );
  // The step list is emitted as it walks — pending, running, done — and each
  // emission is a copy, so a screen holding an earlier one is not rewritten
  // underneath.
  assert.ok(emitted.length >= 7);
  assert.equal(
    emitted[0].every((step) => step.state === "pending"),
    true,
  );
  assert.equal(emitted[1][0].state, "running");
  assert.notEqual(emitted[0], emitted[1]);
  assert.notEqual(emitted[0][0], emitted[1][0]);
});

test("ensureChannel is called exactly when the channel id is null, and before the genesis", async () => {
  let ensured = 0;
  const { deps, calls } = fakeDeps({
    ensureChannel: async () => {
      ensured += 1;
      calls.push(["channel", null]);
      return MINTED_CHANNEL_ID;
    },
  });
  const result = await foundCodingSessionTopic(
    { ...INPUT, channelId: null },
    deps,
  );
  assert.equal(ensured, 1);
  assert.deepEqual(
    calls.map(([name]) => name),
    ["channel", "genesis", "goal", "name"],
  );
  assert.equal(calls[1][1].channelId, MINTED_CHANNEL_ID);
  assert.equal(result.ok, true);
  assert.equal(result.channelId, MINTED_CHANNEL_ID);
  assert.equal(result.steps[0].id, CODING_SESSION_FOUNDING_CHANNEL_STEP);
  assert.equal(result.steps[0].state, "done");

  // …and never when the caller already has one.
  const known = fakeDeps({
    ensureChannel: async () => {
      ensured += 1;
      return MINTED_CHANNEL_ID;
    },
  });
  await foundCodingSessionTopic(INPUT, known.deps);
  assert.equal(ensured, 1);
});

test("no channel and no way to make one stops before anything is signed", async () => {
  const { deps, calls } = fakeDeps();
  const result = await foundCodingSessionTopic(
    { ...INPUT, channelId: null },
    deps,
  );
  assert.equal(result.ok, false);
  assert.equal(result.failedStep, CODING_SESSION_FOUNDING_CHANNEL_STEP);
  assert.match(result.failureReason, /nowhere to be founded/);
  assert.equal(result.sessionRef, null);
  assert.deepEqual(calls, []);
});

test("a channel that could not be prepared is the failed step, in its own words", async () => {
  const { deps, calls } = fakeDeps({
    ensureChannel: async () => {
      throw new Error("relay refused the channel create");
    },
  });
  const result = await foundCodingSessionTopic(
    { ...INPUT, channelId: null },
    deps,
  );
  assert.equal(result.ok, false);
  assert.equal(result.failedStep, CODING_SESSION_FOUNDING_CHANNEL_STEP);
  assert.equal(result.failureReason, "relay refused the channel create");
  assert.equal(result.channelId, null);
  assert.deepEqual(calls, []);
  assert.equal(result.steps[0].state, "failed");
  assert.equal(result.steps[0].detail, "relay refused the channel create");
  assert.equal(result.steps[1].state, "pending");
});

test("the name is skipped when blank, and the goal still goes out", async () => {
  for (const title of [null, "", "   "]) {
    const { deps, calls } = fakeDeps({
      publishName: async () => assert.fail("no name was asked for"),
    });
    const result = await foundCodingSessionTopic({ ...INPUT, title }, deps);
    assert.deepEqual(
      calls.map(([name]) => name),
      ["genesis", "goal"],
    );
    assert.equal(result.ok, true);
    // Not asked for is a different fact from asked for and refused.
    assert.deepEqual(result.name, { published: false, reason: null });
    assert.equal(
      result.steps.some(
        (step) => step.id === CODING_SESSION_FOUNDING_NAME_STEP,
      ),
      false,
    );
  }
});

test("a goal that failed after the genesis is a founded session that says so", async () => {
  const { deps, calls } = fakeDeps({
    publishGoal: async (input) => {
      calls.push(["goal", input]);
      throw new Error("rate-limited: quota exceeded");
    },
  });
  const result = await foundCodingSessionTopic(INPUT, deps);
  // The umbrella exists: the caller lands on it, and the goal pill is the
  // place to set what did not go out.
  assert.equal(result.ok, true);
  assert.equal(result.sessionRef, SESSION_REF);
  assert.equal(result.genesisRef, "genesis-event");
  assert.deepEqual(result.goal, {
    published: false,
    reason: "rate-limited: quota exceeded",
  });
  assert.equal(result.failedStep, null);
  // The name is still attempted — one refused record does not withhold another.
  assert.deepEqual(
    calls.map(([name]) => name),
    ["genesis", "goal", "name"],
  );
  assert.deepEqual(result.name, { published: true, reason: null });
  assert.deepEqual(
    result.steps.map((step) => [step.id, step.state, step.detail]),
    [
      ["genesis", "done", null],
      ["goal", "failed", "rate-limited: quota exceeded"],
      ["name", "done", null],
    ],
  );
});

test("a name that failed after the genesis is reported the same way", async () => {
  const { deps } = fakeDeps({
    publishName: async () => {
      throw new Error("name too long");
    },
  });
  const result = await foundCodingSessionTopic(INPUT, deps);
  assert.equal(result.ok, true);
  assert.deepEqual(result.goal, { published: true, reason: null });
  assert.deepEqual(result.name, { published: false, reason: "name too long" });
  assert.equal(result.failedStep, null);
});

test("a publish that fails with no words gets the step's own fallback sentence", async () => {
  const { deps } = fakeDeps({
    publishGoal: async () => {
      throw new Error("   ");
    },
  });
  const result = await foundCodingSessionTopic(INPUT, deps);
  assert.equal(result.goal.published, false);
  assert.equal(result.goal.reason, "the goal publish did not go out");
});

test("a genesis that did not publish is nothing: ok false, no session ref, nothing after it", async () => {
  const { deps, calls } = fakeDeps({
    publishGenesis: async (input) => {
      calls.push(["genesis", input]);
      throw new Error("auth-required: not a member");
    },
  });
  const result = await foundCodingSessionTopic(INPUT, deps);
  assert.equal(result.ok, false);
  assert.equal(result.sessionRef, null);
  assert.equal(result.genesisRef, null);
  assert.equal(result.failedStep, CODING_SESSION_FOUNDING_GENESIS_STEP);
  assert.equal(result.failureReason, "auth-required: not a member");
  assert.deepEqual(
    calls.map(([name]) => name),
    ["genesis"],
  );
  assert.deepEqual(result.goal, { published: false, reason: null });
  assert.deepEqual(result.name, { published: false, reason: null });
  assert.deepEqual(
    result.steps.map((step) => [step.id, step.state]),
    [
      ["genesis", "failed"],
      ["goal", "pending"],
      ["name", "pending"],
    ],
  );
});

test("the founding resolves rather than rejecting, whatever a dependency throws", async () => {
  const { deps } = fakeDeps({
    publishGenesis: async () => {
      // Not an Error: the reason is still a sentence.
      throw "wire down";
    },
  });
  const result = await foundCodingSessionTopic(INPUT, deps);
  assert.equal(result.ok, false);
  assert.equal(result.failureReason, "wire down");
});

test("a blank goal skips the goal step in the plan and in the run, and is ok", async () => {
  for (const goal of ["", "   ", "\n"]) {
    const { deps, calls, emitted } = fakeDeps({
      publishGoal: async () => assert.fail("no goal was asked for"),
    });
    const result = await foundCodingSessionTopic(
      { ...INPUT, goal, title: null },
      deps,
    );
    assert.deepEqual(
      calls.map(([name]) => name),
      ["genesis"],
    );
    assert.equal(result.ok, true);
    assert.equal(result.sessionRef, SESSION_REF);
    // Not asked for is a different fact from asked for and refused.
    assert.deepEqual(result.goal, { published: false, reason: null });
    assert.deepEqual(result.name, { published: false, reason: null });
    assert.equal(result.failedStep, null);
    assert.deepEqual(
      result.steps.map((step) => [step.id, step.state]),
      [["genesis", "done"]],
    );
    // No emission ever carried a permanently pending goal step.
    for (const steps of emitted) {
      assert.equal(
        steps.some((step) => step.id === CODING_SESSION_FOUNDING_GOAL_STEP),
        false,
      );
    }
  }
});

test("a blank goal with a name still publishes the name", async () => {
  const { deps, calls } = fakeDeps();
  const result = await foundCodingSessionTopic({ ...INPUT, goal: "" }, deps);
  assert.deepEqual(
    calls.map(([name]) => name),
    ["genesis", "name"],
  );
  assert.deepEqual(result.goal, { published: false, reason: null });
  assert.deepEqual(result.name, { published: true, reason: null });
});
