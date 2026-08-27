import assert from "node:assert/strict";
import test from "node:test";

import {
  CODING_SESSION_CREW_LAUNCH_FAMILY_STEP,
  CODING_SESSION_CREW_LAUNCH_GENESIS_STEP,
  CODING_SESSION_CREW_LAUNCH_GRANT_STEP,
  CODING_SESSION_CREW_LAUNCH_TURN_STEP,
  codingSessionCrewLaunchSeatStepId,
  launchCodingSessionCrew,
  leadSeat,
} from "./codingSessionCrewLaunch.ts";

const LEAD = {
  personaId: "p-lead",
  role: "lead",
  actor: "a".repeat(64),
  actorLabel: "Fable",
  model: "claude-opus-5",
  vendor: null,
};
const BUILDER = {
  personaId: "p-build",
  role: "builder",
  actor: "b".repeat(64),
  actorLabel: "Codey",
  model: "gpt-5.6-sol",
  vendor: null,
};
const VERIFIER = {
  personaId: "p-verify",
  role: "verifier",
  actor: "c".repeat(64),
  actorLabel: "Grokker",
  model: "grok-4",
  vendor: null,
};

function target(sessionId) {
  return {
    driver: "claude-agent-acp",
    instanceId: "inst-1",
    sessionId,
    generation: 1,
  };
}

function recordingDeps(overrides = {}) {
  const log = [];
  const deps = {
    log,
    newSessionRef: () => "11111111-1111-4111-8111-111111111111",
    publishGenesis: async () => {
      log.push("genesis");
      return { eventId: "genesis-1" };
    },
    publishSeatCreate: async ({ seat, index }) => {
      log.push(`publish:${seat.role}`);
      return { commandId: `cmd-${index}` };
    },
    awaitSeatReceipt: async ({ commandId, seat }) => {
      log.push(`receipt:${seat.role}`);
      return target(`sess-${commandId}`);
    },
    grantOperator: async ({ granteePubkey }) => {
      log.push(`grant:${granteePubkey.slice(0, 4)}`);
    },
    sendFirstTurn: async ({ text }) => {
      log.push("turn");
      deps.sentText = text;
    },
    ...overrides,
  };
  return deps;
}

const INPUT = {
  channelId: "chan-1",
  goal: "Close ledger item 53.",
  seats: [LEAD, BUILDER, VERIFIER],
  primaryPersonaId: "p-lead",
};

test("a launch is receipt-gated: no seat is published before the last one's receipt", async () => {
  const deps = recordingDeps();
  const result = await launchCodingSessionCrew(INPUT, deps);
  assert.equal(result.ok, true);
  assert.deepEqual(deps.log, [
    "genesis",
    "publish:lead",
    "receipt:lead",
    "publish:builder",
    "receipt:builder",
    "publish:verifier",
    "receipt:verifier",
    `grant:${LEAD.actor.slice(0, 4)}`,
    "turn",
  ]);
  assert.equal(result.seats.length, 3);
  assert.equal(result.genesisRef, "genesis-1");
});

test("the first turn carries the goal and the roster", async () => {
  const deps = recordingDeps();
  await launchCodingSessionCrew(INPUT, deps);
  assert.ok(deps.sentText.startsWith("Close ledger item 53.\n\n[Crew]"));
  assert.match(
    deps.sentText,
    /- lead: Fable \(anthropic · claude-opus-5\) — you/,
  );
  assert.match(deps.sentText, /- builder: Codey \(openai · gpt-5\.6-sol\)/);
  assert.match(deps.sentText, /- verifier: Grokker \(xai · grok-4\)/);
});

test("a same-vendor verifier is refused before anything is published", async () => {
  const deps = recordingDeps();
  const result = await launchCodingSessionCrew(
    {
      ...INPUT,
      seats: [LEAD, BUILDER, { ...VERIFIER, model: "gpt-5.6-sol" }],
    },
    deps,
  );
  assert.equal(result.ok, false);
  assert.equal(result.failedStep, CODING_SESSION_CREW_LAUNCH_FAMILY_STEP);
  assert.match(result.failureReason, /openai/);
  // The refusal is hard: nothing signed, no genesis, no seats.
  assert.deepEqual(deps.log, []);
  assert.equal(result.genesisRef, null);
  assert.equal(result.sessionRef, null);
  assert.deepEqual(result.seats, []);
});

test("an undeclared vendor is refused before anything is published", async () => {
  const deps = recordingDeps();
  const result = await launchCodingSessionCrew(
    {
      ...INPUT,
      seats: [LEAD, BUILDER, { ...VERIFIER, model: "qwen3-coder" }],
    },
    deps,
  );
  assert.equal(result.ok, false);
  assert.equal(result.failedStep, CODING_SESSION_CREW_LAUNCH_FAMILY_STEP);
  assert.match(result.failureReason, /Declare the model vendor/);
  assert.deepEqual(deps.log, []);
});

test("a failed seat names its step and leaves the earlier seats live", async () => {
  const deps = recordingDeps({
    awaitSeatReceipt: async ({ seat }) => {
      if (seat.role === "builder") {
        throw new Error("ACTOR_UNAVAILABLE");
      }
      return target(`sess-${seat.role}`);
    },
  });
  const result = await launchCodingSessionCrew(INPUT, deps);
  assert.equal(result.ok, false);
  assert.equal(result.failedStep, codingSessionCrewLaunchSeatStepId(1));
  assert.match(result.failureReason, /builder seat was not created/);
  assert.match(result.failureReason, /ACTOR_UNAVAILABLE/);
  // Seat one survived; seat three was never published.
  assert.equal(result.seats.length, 1);
  assert.equal(result.seats[0].seat.role, "lead");
  assert.ok(!deps.log.includes("publish:verifier"));
  assert.ok(!deps.log.includes("grant:" + LEAD.actor.slice(0, 4)));
  // Every step is still reported, so the untouched ones read as untouched.
  const byId = new Map(result.steps.map((s) => [s.id, s.state]));
  assert.equal(byId.get(CODING_SESSION_CREW_LAUNCH_GENESIS_STEP), "done");
  assert.equal(byId.get(codingSessionCrewLaunchSeatStepId(0)), "done");
  assert.equal(byId.get(codingSessionCrewLaunchSeatStepId(1)), "failed");
  assert.equal(byId.get(codingSessionCrewLaunchSeatStepId(2)), "pending");
  assert.equal(byId.get(CODING_SESSION_CREW_LAUNCH_TURN_STEP), "pending");
});

test("a failed grant is named rather than swallowed", async () => {
  const deps = recordingDeps({
    grantOperator: async () => {
      throw new Error("head conflict");
    },
  });
  const result = await launchCodingSessionCrew(INPUT, deps);
  assert.equal(result.ok, false);
  assert.equal(result.failedStep, CODING_SESSION_CREW_LAUNCH_GRANT_STEP);
  assert.match(result.failureReason, /cannot steer its siblings/);
  assert.equal(result.seats.length, 3);
  assert.ok(!deps.log.includes("turn"));
});

test("a failed genesis stops before any seat is published", async () => {
  const deps = recordingDeps({
    publishGenesis: async () => {
      throw new Error("relay refused");
    },
  });
  const result = await launchCodingSessionCrew(INPUT, deps);
  assert.equal(result.ok, false);
  assert.equal(result.failedStep, CODING_SESSION_CREW_LAUNCH_GENESIS_STEP);
  assert.deepEqual(result.seats, []);
  assert.deepEqual(deps.log, []);
});

test("the lead seat holds operator authority; the primary stands in without one", () => {
  assert.equal(leadSeat([LEAD, BUILDER], "p-build"), LEAD);
  assert.equal(leadSeat([BUILDER, VERIFIER], "p-verify"), VERIFIER);
  assert.equal(leadSeat([], "p-lead"), null);
});

test("progress is reported step by step", async () => {
  const seen = [];
  const deps = recordingDeps({
    onSteps: (steps) => {
      seen.push(steps.map((step) => `${step.id}:${step.state}`).join("|"));
    },
  });
  await launchCodingSessionCrew(INPUT, deps);
  assert.ok(seen.length > 5);
  assert.ok(
    seen.at(-1).includes(`${CODING_SESSION_CREW_LAUNCH_TURN_STEP}:done`),
  );
});
