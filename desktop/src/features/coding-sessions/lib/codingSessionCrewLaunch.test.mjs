import assert from "node:assert/strict";
import test from "node:test";

import {
  CODING_SESSION_CREW_LAUNCH_CHANNEL_STEP,
  CODING_SESSION_CREW_LAUNCH_FAMILY_STEP,
  CODING_SESSION_CREW_LAUNCH_GENESIS_STEP,
  CODING_SESSION_CREW_LAUNCH_GRANT_STEP,
  CODING_SESSION_CREW_LAUNCH_TURN_STEP,
  codingSessionCrewLaunchSeatStepId,
  launchCodingSessionCrew,
  leadSeat,
  planCodingSessionCrewLaunch,
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

/** A runtime that really does offer all three seats' models. */
const PROVIDER = {
  label: "claude-agent-acp",
  allowedModels: ["claude-opus-5[1m]", "gpt-5.6-sol", "grok-4"],
};

/**
 * A team whose primary seat is a builder: with no `lead` role in the roster,
 * the primary is the seat the launch creates. Used by the checks below, which
 * are about the seat that is actually created (D14) — not about the roster the
 * lead may hire from.
 */
const PRIMARY_BUILDER = { ...BUILDER, personaId: "p-lead" };

const INPUT = {
  channelId: "chan-1",
  goal: "Close ledger item 53.",
  seats: [LEAD, BUILDER, VERIFIER],
  primaryPersonaId: "p-lead",
  provider: PROVIDER,
};

test("a created seat whose model the selected provider cannot run is refused before anything is published", async () => {
  // The bug this pins: a create goes to the one selected providerInstanceRef,
  // and the provider's apply_model does not refuse an unknown model — it logs
  // "using its default" and creates the session. So a seat declared openai on
  // a Claude-only runtime passed the vendor check and then ran on Anthropic.
  // Under D14 only the lead is created, so this is the seat it must hold for.
  const deps = recordingDeps();
  const result = await launchCodingSessionCrew(
    {
      ...INPUT,
      seats: [PRIMARY_BUILDER, VERIFIER],
      provider: { label: "claude-agent-acp", allowedModels: ["claude-opus-5"] },
    },
    deps,
  );
  assert.equal(result.ok, false);
  assert.equal(result.failedStep, CODING_SESSION_CREW_LAUNCH_FAMILY_STEP);
  assert.match(result.failureReason, /gpt-5\.6-sol/);
  assert.match(result.failureReason, /claude-agent-acp/);
  assert.deepEqual(deps.log, [], "nothing may be signed");
  assert.equal(result.genesisRef, null);
  assert.equal(result.sessionRef, null);
});

test("the `default` alias is a choice the runtime makes, not a refusal, for a one-seat launch", async () => {
  // It used to be a hard refusal, and the reason was the verifier rule: a
  // runtime without live model discovery publishes allowedModels: ["default"],
  // every seat is created on it, and apply_model then runs whatever the one
  // adapter chose — so a verifier "on openai" ran beside its builder. Under
  // D14 no verifier is created by a launch, so there is no cross-seat rule
  // left for the alias to fool, and refusing here would block every runtime
  // that cannot list its models. The roster still says the vendor is
  // undecided rather than claiming one.
  const deps = recordingDeps();
  const result = await launchCodingSessionCrew(
    {
      ...INPUT,
      seats: [{ ...PRIMARY_BUILDER, model: "default", vendor: null }, VERIFIER],
      provider: { label: "claude-agent-acp", allowedModels: ["default"] },
    },
    deps,
  );
  assert.equal(result.ok, true);
  assert.equal(result.seats.length, 1);
});

test("a seat with no model at all cannot be vendor-checked, so it is refused", async () => {
  const deps = recordingDeps();
  const result = await launchCodingSessionCrew(
    { ...INPUT, seats: [{ ...PRIMARY_BUILDER, model: null }] },
    deps,
  );
  assert.equal(result.ok, false);
  assert.equal(result.failedStep, CODING_SESSION_CREW_LAUNCH_FAMILY_STEP);
  assert.match(result.failureReason, /no model/);
  assert.deepEqual(deps.log, []);
});

test("an empty provider catalog is a refusal, not a pass", async () => {
  const deps = recordingDeps();
  const result = await launchCodingSessionCrew(
    {
      ...INPUT,
      seats: [PRIMARY_BUILDER],
      provider: { label: "goose", allowedModels: [] },
    },
    deps,
  );
  assert.equal(result.ok, false);
  assert.equal(result.failedStep, CODING_SESSION_CREW_LAUNCH_FAMILY_STEP);
  assert.match(result.failureReason, /cannot/);
  assert.deepEqual(deps.log, []);
});

test("a bracketed catalog id still runs the base model it names", async () => {
  // `claude-opus-5[1m]` in the catalog offers `claude-opus-5`: the bracket is
  // a context window, not a different vendor.
  const deps = recordingDeps();
  const result = await launchCodingSessionCrew(INPUT, deps);
  assert.equal(result.ok, true);
});

test("a lead's model is not the vendor rule's business", async () => {
  // Only verifier and builder seats decide the family rule; a lead on a model
  // this runtime does not publish is the model picker's problem, not a
  // silently-wrong-vendor problem.
  const deps = recordingDeps();
  const result = await launchCodingSessionCrew(
    {
      ...INPUT,
      seats: [{ ...LEAD, model: "some-local-weight" }, BUILDER, VERIFIER],
      provider: {
        label: "claude-agent-acp",
        allowedModels: ["gpt-5.6-sol", "grok-4"],
      },
    },
    deps,
  );
  assert.equal(result.ok, true);
});

test("a same-vendor verifier no longer refuses a launch that never creates it", async () => {
  // The verifier/builder family rule (D8) is not repealed — it moves. Neither
  // seat is created by a launch any more, so refusing here would be refusing a
  // fact that is not yet true; the rule belongs at hire time, on the seat
  // actually being created.
  const deps = recordingDeps();
  const result = await launchCodingSessionCrew(
    {
      ...INPUT,
      seats: [LEAD, BUILDER, { ...VERIFIER, model: "gpt-5.6-sol" }],
    },
    deps,
  );
  assert.equal(result.ok, true);
  assert.equal(result.seats.length, 1);
  assert.deepEqual(
    result.hireableSeats.map((seat) => seat.role),
    ["builder", "verifier"],
  );
});

test("a created seat on a vendor the selected runtime cannot run is refused", async () => {
  // The check that survives a one-seat launch: the runtime the create is
  // published against runs exactly one vendor, and a seat whose own vendor is
  // another one would run on a vendor nothing on screen names (item 79c is the
  // copy for this; this is the refusal behind it).
  const deps = recordingDeps();
  const result = await launchCodingSessionCrew(
    {
      ...INPUT,
      seats: [PRIMARY_BUILDER],
      provider: {
        label: "claude-agent-acp",
        instanceRef: "claude-primary",
        allowedModels: ["gpt-5.6-sol"],
      },
    },
    deps,
  );
  assert.equal(result.ok, false);
  assert.equal(result.failedStep, CODING_SESSION_CREW_LAUNCH_FAMILY_STEP);
  assert.match(result.failureReason, /anthropic/);
  assert.deepEqual(deps.log, [], "nothing may be signed");
});

test("a refused lead create names its step, and the session it founded stays", async () => {
  const deps = recordingDeps({
    awaitSeatReceipt: async () => {
      throw new Error("ACTOR_UNAVAILABLE");
    },
  });
  const result = await launchCodingSessionCrew(INPUT, deps);
  assert.equal(result.ok, false);
  assert.equal(result.failedStep, codingSessionCrewLaunchSeatStepId(0));
  assert.match(result.failureReason, /lead seat was not created/);
  assert.match(result.failureReason, /ACTOR_UNAVAILABLE/);
  assert.deepEqual(result.seats, []);
  assert.ok(!deps.log.includes(`grant:${LEAD.actor.slice(0, 4)}`));
  assert.ok(!deps.log.includes("turn"));
  // The genesis is real and is reported, rather than being rolled back into a
  // launch that claims to have left nothing behind.
  assert.equal(result.genesisRef, "genesis-1");
  const byId = new Map(result.steps.map((s) => [s.id, s.state]));
  assert.equal(byId.get(CODING_SESSION_CREW_LAUNCH_GENESIS_STEP), "done");
  assert.equal(byId.get(codingSessionCrewLaunchSeatStepId(0)), "failed");
  assert.equal(byId.get(CODING_SESSION_CREW_LAUNCH_GRANT_STEP), "pending");
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
  assert.equal(result.seats.length, 1);
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

test("a seat staged without a role pack says so, and is not called seated craft", async () => {
  const deps = recordingDeps({
    publishSeatCreate: async ({ seat, index }) => {
      deps.log.push(`publish:${seat.role}`);
      return { commandId: `cmd-${index}`, packStaged: false };
    },
  });
  const result = await launchCodingSessionCrew(INPUT, deps);
  assert.equal(result.ok, true);
  assert.deepEqual(result.seatsWithoutRolePack, ["Fable"]);
  const byId = new Map(result.steps.map((entry) => [entry.id, entry]));
  const leadStep = byId.get(codingSessionCrewLaunchSeatStepId(0));
  assert.equal(leadStep.state, "done");
  assert.match(leadStep.detail, /no role skills/);
});

test("a seat that did carry a pack says nothing extra", async () => {
  const deps = recordingDeps({
    publishSeatCreate: async ({ index }) => ({
      commandId: `cmd-${index}`,
      packStaged: true,
    }),
  });
  const result = await launchCodingSessionCrew(INPUT, deps);
  assert.deepEqual(result.seatsWithoutRolePack, []);
  const byId = new Map(result.steps.map((entry) => [entry.id, entry]));
  assert.equal(byId.get(codingSessionCrewLaunchSeatStepId(0)).detail, null);
});

test("a project with no sessions channel yet mints one before the genesis, and seats into it", async () => {
  // The front-door bug (item 79): from a project whose sessions channel has
  // never been published, `channelId` is null and only the one-session path
  // minted it — so the Team tab could not launch at all, and said nothing.
  const seen = [];
  const deps = recordingDeps({
    ensureChannel: async () => {
      seen.push("channel");
      return "chan-minted";
    },
    publishGenesis: async ({ channelId }) => {
      seen.push(`genesis:${channelId}`);
      return { eventId: "genesis-1" };
    },
    publishSeatCreate: async ({ seat, index, channelId }) => {
      seen.push(`publish:${seat.role}:${channelId}`);
      return { commandId: `cmd-${index}` };
    },
    awaitSeatReceipt: async ({ commandId, channelId }) => {
      seen.push(`receipt:${channelId}`);
      return target(`sess-${commandId}`);
    },
    grantOperator: async ({ channelId }) => {
      seen.push(`grant:${channelId}`);
    },
    sendFirstTurn: async ({ channelId }) => {
      seen.push(`turn:${channelId}`);
    },
  });

  const result = await launchCodingSessionCrew(
    { ...INPUT, channelId: null, seats: [LEAD] },
    deps,
  );

  assert.equal(result.ok, true);
  assert.equal(result.channelId, "chan-minted");
  assert.deepEqual(seen, [
    "channel",
    "genesis:chan-minted",
    "publish:lead:chan-minted",
    "receipt:chan-minted",
    "grant:chan-minted",
    "turn:chan-minted",
  ]);
  const channelStep = result.steps.find(
    (entry) => entry.id === CODING_SESSION_CREW_LAUNCH_CHANNEL_STEP,
  );
  assert.equal(channelStep.state, "done");
});

test("a launch with a channel already published never asks for another one", async () => {
  const deps = recordingDeps({
    ensureChannel: async () => {
      throw new Error("a known channel must not be re-minted");
    },
    publishGenesis: async ({ channelId }) => {
      deps.log.push(`genesis:${channelId}`);
      return { eventId: "genesis-1" };
    },
  });
  const result = await launchCodingSessionCrew(INPUT, deps);
  assert.equal(result.ok, true);
  assert.equal(result.channelId, "chan-1");
  assert.equal(deps.log[0], "genesis:chan-1");
  assert.equal(
    result.steps.some(
      (entry) => entry.id === CODING_SESSION_CREW_LAUNCH_CHANNEL_STEP,
    ),
    false,
    "a channel that exists is not a step anybody walks",
  );
});

test("no channel and nothing that can mint one fails before the genesis", async () => {
  const deps = recordingDeps();
  const result = await launchCodingSessionCrew(
    { ...INPUT, channelId: null },
    deps,
  );
  assert.equal(result.ok, false);
  assert.equal(result.failedStep, CODING_SESSION_CREW_LAUNCH_CHANNEL_STEP);
  assert.match(result.failureReason, /channel/i);
  assert.deepEqual(deps.log, [], "nothing may be signed");
  assert.equal(result.channelId, null);
});

/**
 * D14 — launching a team seats the LEAD ONLY.
 *
 * The roster is what the lead may hire, not what the launch creates. Two
 * things fall out of that and are pinned here: exactly one create goes out,
 * and a seat the launch never creates can no longer refuse the launch over
 * the one-provider limitation (item 79c).
 */
test("a launch publishes exactly one seated create — the lead's", async () => {
  const deps = recordingDeps();
  const result = await launchCodingSessionCrew(INPUT, deps);
  assert.equal(result.ok, true);
  assert.deepEqual(deps.log, [
    "genesis",
    "publish:lead",
    "receipt:lead",
    `grant:${LEAD.actor.slice(0, 4)}`,
    "turn",
  ]);
  assert.equal(result.seats.length, 1);
  assert.equal(result.seats[0].seat.role, "lead");
  assert.deepEqual(
    result.hireableSeats.map((seat) => seat.role),
    ["builder", "verifier"],
  );
});

test("the plan names one create step and lists the rest as hireable", () => {
  const steps = planCodingSessionCrewLaunch(INPUT);
  const creates = steps.filter((step) => step.id.startsWith("create:"));
  assert.equal(creates.length, 1);
  assert.match(creates[0].label, /Fable/);
});

test("a seat the launch never creates cannot refuse the launch", async () => {
  // Item 79(c): a Codex architect on a Claude runtime disabled Launch, because
  // every seat was created against the one selected provider. Under D14 only
  // the lead is created, so the architect's model is the lead's problem to
  // solve at hire time — not a reason nobody can launch at all.
  const deps = recordingDeps();
  const result = await launchCodingSessionCrew(
    {
      ...INPUT,
      seats: [LEAD, { ...BUILDER, model: "gpt-5.6-sol" }],
      provider: { label: "claude-agent-acp", allowedModels: ["claude-opus-5"] },
    },
    deps,
  );
  assert.equal(result.ok, true);
  assert.equal(result.seats.length, 1);
});

test("the lead's first turn carries the goal and the roster it may hire from", async () => {
  const deps = recordingDeps();
  await launchCodingSessionCrew(INPUT, deps);
  assert.ok(deps.sentText.startsWith("Close ledger item 53.\n\n"));
  assert.match(deps.sentText, /You are seated. Nobody else is/);
  assert.match(deps.sentText, /- builder: Codey \(openai · gpt-5\.6-sol\)/);
  assert.match(deps.sentText, /bee sessions hire/);
});
