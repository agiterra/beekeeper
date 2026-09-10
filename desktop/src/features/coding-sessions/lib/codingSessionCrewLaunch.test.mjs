import assert from "node:assert/strict";
import test from "node:test";

import { buildCodingSessionTranscriptGenerationId } from "./codingSessionTranscriptPresentation.ts";
import {
  CODING_SESSION_CREW_LAUNCH_CHANNEL_STEP,
  CODING_SESSION_CREW_LAUNCH_FAMILY_STEP,
  CODING_SESSION_CREW_LAUNCH_GENESIS_STEP,
  CODING_SESSION_CREW_LAUNCH_GRANT_STEP,
  CODING_SESSION_CREW_LAUNCH_TURN_STEP,
  CODING_SESSION_CREW_LAUNCH_WORKTREE_STEP,
  codingSessionCrewLaunchSeatStepId,
  codingSessionCrewLeadDestination,
  codingSessionCrewProjectNote,
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
    grantLeadSeat: async ({ actorPubkey, role }) => {
      log.push(`seat:${actorPubkey.slice(0, 4)}:${role}`);
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
  assert.match(result.failureReason, /governed lead grants/);
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
    `seat:${LEAD.actor.slice(0, 4)}:lead`,
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

/**
 * Item 87(a), found live 2026-08-28 21:21: Brian launched a team from inside
 * the Bee Keeper project and the lead's create carried `projectRef: None`, so
 * the session was founded into a channel nobody was looking at and the
 * project's session list — which groups by projectRef — never showed it.
 */
test("the lead's create carries the project it was launched from", async () => {
  const deps = recordingDeps();
  const created = [];
  deps.publishSeatCreate = async ({ seat, index, projectRef, workdir }) => {
    created.push({ role: seat.role, projectRef, workdir });
    return { commandId: `cmd-${index}` };
  };
  const result = await launchCodingSessionCrew(
    {
      channelId: "chan-1",
      goal: "Ship the front door.",
      seats: [LEAD, BUILDER],
      primaryPersonaId: LEAD.personaId,
      projectRef: "34550:owner:beekeeper",
      provider: PROVIDER,
    },
    deps,
  );
  assert.equal(result.ok, true);
  assert.deepEqual(created, [
    { role: "lead", projectRef: "34550:owner:beekeeper", workdir: null },
  ]);
});

test("a launch with no project signs no project", async () => {
  const deps = recordingDeps();
  let seen;
  deps.publishSeatCreate = async ({ projectRef }) => {
    seen = projectRef;
    return { commandId: "cmd-0" };
  };
  await launchCodingSessionCrew(
    {
      channelId: "chan-1",
      goal: "Ship the front door.",
      seats: [LEAD],
      primaryPersonaId: LEAD.personaId,
      provider: PROVIDER,
    },
    deps,
  );
  assert.equal(seen, null);
});

/** The sentence the tab shows instead of letting a launch land nowhere. */
test("the project note names the project, or says there is none", () => {
  assert.equal(
    codingSessionCrewProjectNote("Bee Keeper"),
    "Launching in Bee Keeper: the session belongs to that project and " +
      "appears in its sessions.",
  );
  assert.match(
    codingSessionCrewProjectNote(null),
    /will not belong to a project/,
  );
  assert.match(codingSessionCrewProjectNote("   "), /will not belong/);
});

/**
 * Item 87(d): the Team tab had no worktree toggle, so the lead ran in the
 * checkout the tab named — the operator's own hot checkout — while every seat
 * it went on to hire got a worktree of its own.
 */
test("the lead's create runs in the worktree the launch cut for it", async () => {
  const deps = recordingDeps();
  const cut = [];
  deps.createLeadWorktree = async (request) => {
    cut.push(request);
    return { path: "/Users/b/Projects/bk/bk-ui-lead" };
  };
  let workdir;
  deps.publishSeatCreate = async (input) => {
    workdir = input.workdir;
    return { commandId: "cmd-0" };
  };
  const result = await launchCodingSessionCrew(
    {
      channelId: "chan-1",
      goal: "Ship the front door.",
      seats: [LEAD],
      primaryPersonaId: LEAD.personaId,
      provider: PROVIDER,
      workdir: "/Users/b/Projects/bk/bk",
      leadWorktree: { name: "ui-lead", source: "main" },
    },
    deps,
  );
  assert.equal(result.ok, true);
  assert.deepEqual(cut, [
    { workdir: "/Users/b/Projects/bk/bk", name: "ui-lead", source: "main" },
  ]);
  assert.equal(workdir, "/Users/b/Projects/bk/bk-ui-lead");
  assert.equal(result.leadWorkdir, "/Users/b/Projects/bk/bk-ui-lead");
  // The step is planned and walked, not done invisibly.
  assert.equal(
    result.steps.find(
      (step) => step.id === CODING_SESSION_CREW_LAUNCH_WORKTREE_STEP,
    )?.state,
    "done",
  );
});

test("without the toggle the lead runs in the checkout, and no worktree step is shown", async () => {
  const deps = recordingDeps();
  deps.createLeadWorktree = async () => {
    throw new Error("nothing asked for a worktree");
  };
  let workdir;
  deps.publishSeatCreate = async (input) => {
    workdir = input.workdir;
    return { commandId: "cmd-0" };
  };
  const result = await launchCodingSessionCrew(
    {
      channelId: "chan-1",
      goal: "Ship the front door.",
      seats: [LEAD],
      primaryPersonaId: LEAD.personaId,
      provider: PROVIDER,
      workdir: "/Users/b/Projects/bk/bk",
      leadWorktree: null,
    },
    deps,
  );
  assert.equal(result.ok, true);
  assert.equal(workdir, "/Users/b/Projects/bk/bk");
  assert.equal(
    result.steps.some(
      (step) => step.id === CODING_SESSION_CREW_LAUNCH_WORKTREE_STEP,
    ),
    false,
  );
});

test("a worktree that cannot be cut stops the launch before anything is signed", async () => {
  const deps = recordingDeps();
  deps.createLeadWorktree = async () => {
    throw new Error("fatal: 'ui-lead' is already checked out");
  };
  const result = await launchCodingSessionCrew(
    {
      channelId: "chan-1",
      goal: "Ship the front door.",
      seats: [LEAD],
      primaryPersonaId: LEAD.personaId,
      provider: PROVIDER,
      workdir: "/Users/b/Projects/bk/bk",
      leadWorktree: { name: "ui-lead", source: null },
    },
    deps,
  );
  assert.equal(result.ok, false);
  assert.equal(result.failedStep, CODING_SESSION_CREW_LAUNCH_WORKTREE_STEP);
  assert.match(result.failureReason, /already checked out/);
  assert.deepEqual(deps.log, []);
});

/**
 * Item 87(b): the launch closed the dialog and navigated nowhere, so a
 * session that really had been founded appeared nowhere the person looked.
 */
test("a successful launch resolves the lead's generation to open", () => {
  const leadTarget = target("sess-lead");
  const destination = codingSessionCrewLeadDestination({
    result: {
      ok: true,
      channelId: "chan-1",
      seats: [{ seat: LEAD, target: leadTarget }],
      hireableSeats: [],
    },
    providerAuthorityPubkey: "f".repeat(64),
  });
  assert.deepEqual(destination, {
    channelId: "chan-1",
    generationId: buildCodingSessionTranscriptGenerationId(
      "chan-1",
      "f".repeat(64),
      leadTarget,
    ),
  });
});

test("a failed launch, or one with no provider named, opens nothing", () => {
  assert.equal(
    codingSessionCrewLeadDestination({
      result: {
        ok: false,
        channelId: "chan-1",
        seats: [{ seat: LEAD, target: target("s") }],
        hireableSeats: [],
      },
      providerAuthorityPubkey: "f".repeat(64),
    }),
    null,
  );
  assert.equal(
    codingSessionCrewLeadDestination({
      result: {
        ok: true,
        channelId: "chan-1",
        seats: [{ seat: LEAD, target: target("s") }],
        hireableSeats: [],
      },
      providerAuthorityPubkey: null,
    }),
    null,
  );
  assert.equal(
    codingSessionCrewLeadDestination({
      result: { ok: true, channelId: "chan-1", seats: [], hireableSeats: [] },
      providerAuthorityPubkey: "f".repeat(64),
    }),
    null,
  );
});

// ── B3.1: the session policy, between the genesis and the first create ────────

test("a policy is published after the genesis and before any seat is created", async () => {
  // Order is the point. A seat should be able to read the posture and the
  // budget it is working under off the wire rather than out of the founder's
  // memory, so the record has to exist before the seat does.
  const deps = recordingDeps({
    publishPolicy: async ({ sessionRef, genesisRef }) => {
      deps.log.push("policy");
      assert.equal(sessionRef, "11111111-1111-4111-8111-111111111111");
      assert.equal(genesisRef, "genesis-1");
      return { eventId: "policy-1" };
    },
  });
  const result = await launchCodingSessionCrew(
    { ...INPUT, policySet: true },
    deps,
  );
  assert.equal(result.ok, true);
  assert.equal(result.policyEventId, "policy-1");
  assert.deepEqual(deps.log.slice(0, 3), ["genesis", "policy", "publish:lead"]);
  const step = result.steps.find((entry) => entry.id === "policy");
  assert.equal(step.state, "done");
});

test("a launch that set no policy publishes none, and shows no step for one", async () => {
  // The withdrawal record is a deliberate act of taking a policy back, never
  // the default shape of a session nobody wrote a policy for.
  const deps = recordingDeps({
    publishPolicy: async () => {
      deps.log.push("policy");
      return { eventId: "policy-1" };
    },
  });
  const result = await launchCodingSessionCrew(INPUT, deps);
  assert.equal(result.ok, true);
  assert.equal(result.policyEventId, null);
  assert.equal(deps.log.includes("policy"), false);
  assert.equal(
    result.steps.some((entry) => entry.id === "policy"),
    false,
  );
});

test("a policy that will not publish stops the launch, by name", async () => {
  // Unlike the goal — which still reaches the lead in its first turn — a
  // policy that fails to publish is a ceiling the founder set and nobody can
  // read. Launching anyway would put a team to work under limits that exist
  // only on the screen that is about to close.
  const deps = recordingDeps({
    publishPolicy: async () => {
      throw new Error("rate-limited: quota exceeded");
    },
  });
  const result = await launchCodingSessionCrew(
    { ...INPUT, policySet: true },
    deps,
  );
  assert.equal(result.ok, false);
  assert.equal(result.failedStep, "policy");
  assert.match(result.failureReason, /policy was not published/);
  assert.match(result.failureReason, /quota exceeded/);
  // The genesis stands: a failed step never un-founds the session before it.
  assert.equal(result.genesisRef, "genesis-1");
  assert.equal(deps.log.includes("publish:lead"), false);
});

test("a launch asked for a policy with nothing able to publish one is refused", async () => {
  const deps = recordingDeps({});
  delete deps.publishPolicy;
  const result = await launchCodingSessionCrew(
    { ...INPUT, policySet: true },
    deps,
  );
  assert.equal(result.ok, false);
  assert.equal(result.failedStep, "policy");
  assert.match(result.failureReason, /nothing here can publish one/);
});

test("F4: a policy core refuses is refused before the session is founded", async () => {
  // Every 44245 rule belongs to buzz-core, which is right — and meant a
  // cross-field refusal was first evaluated after 44226 and 44227 were already
  // on the wire. The dry run costs one call and moves it back to where it
  // costs nothing.
  const deps = recordingDeps({
    validatePolicy: async () => {
      deps.log.push("validate");
      throw new Error("budget.tokensPerSeat must not exceed tokensPerSession");
    },
    publishPolicy: async () => {
      deps.log.push("policy");
      return { eventId: "policy-1" };
    },
  });
  const result = await launchCodingSessionCrew(
    { ...INPUT, policySet: true },
    deps,
  );
  assert.equal(result.ok, false);
  assert.equal(result.failedStep, "family-check");
  assert.match(result.failureReason, /must not exceed tokensPerSession/);
  // Nothing was founded, so there is no orphan to name.
  assert.equal(result.genesisRef, null);
  assert.equal(deps.log.includes("genesis"), false);
  assert.deepEqual(deps.log, ["validate"]);
});

test("F4: a failure after the genesis names the session it left behind", async () => {
  const deps = recordingDeps({
    publishPolicy: async () => {
      throw new Error("rate-limited: quota exceeded");
    },
  });
  const result = await launchCodingSessionCrew(
    { ...INPUT, policySet: true },
    deps,
  );
  assert.equal(result.ok, false);
  // In words, and with the ids — the step list says it only as an icon and a
  // colour, and a session a person cannot find is the same defect as a badge
  // pointing at a message that is not there.
  assert.match(result.failureReason, /already founded and has no seats/);
  assert.match(
    result.failureReason,
    /session 11111111-1111-4111-8111-111111111111/,
  );
  assert.match(result.failureReason, /genesis genesis-1/);
  const step = result.steps.find((entry) => entry.id === "policy");
  assert.match(step.detail, /already founded and has no seats/);
});

test("F4: a launch that never founded anything names no orphan", async () => {
  const deps = recordingDeps({
    publishGenesis: async () => {
      throw new Error("relay refused the genesis");
    },
  });
  const result = await launchCodingSessionCrew(INPUT, deps);
  assert.equal(result.ok, false);
  assert.equal(result.failedStep, "genesis");
  assert.doesNotMatch(result.failureReason, /already founded/);
});

test("finding 84: a seat staged from the project's repository says so on its step", async () => {
  const deps = recordingDeps({
    publishSeatCreate: async ({ index }) => ({
      commandId: `cmd-${index}`,
      packStaged: true,
      packRef: {
        repo: `30617:${"3d3b7169".padEnd(64, "0")}:agiterra-packs`,
        sha: "dd935f43".padEnd(40, "0"),
        role: "lead",
        path: "personas/roles/lead",
      },
    }),
  });
  const result = await launchCodingSessionCrew(INPUT, deps);
  assert.equal(result.ok, true);
  const byId = new Map(result.steps.map((entry) => [entry.id, entry]));
  assert.equal(
    byId.get(codingSessionCrewLaunchSeatStepId(0)).detail,
    "staged from 30617:3d3b7169…:agiterra-packs@dd935f43",
  );
});

/**
 * A founded umbrella — genesis, goal and name on the wire, nothing running —
 * is started, not founded again. The launch takes the refs it already has,
 * publishes no channel and no genesis, and seats the lead under them.
 */
const EXISTING = {
  sessionRef: "22222222-2222-4222-8222-222222222222",
  genesisRef: "genesis-founded",
};

test("an existing umbrella is started under its own refs: no channel, no genesis, and neither dep is called", async () => {
  const deps = recordingDeps({
    ensureChannel: async () => {
      throw new Error("a founded umbrella already has a channel");
    },
    newSessionRef: () => {
      throw new Error("a founded umbrella already has a session ref");
    },
    publishGenesis: async () => {
      throw new Error("a founded umbrella must not be founded twice");
    },
    publishSeatCreate: async ({ seat, index, sessionRef, genesisRef }) => {
      deps.log.push(`publish:${seat.role}:${sessionRef}:${genesisRef}`);
      return { commandId: `cmd-${index}` };
    },
  });

  const result = await launchCodingSessionCrew(
    { ...INPUT, seats: [LEAD], existingUmbrella: EXISTING },
    deps,
  );

  assert.equal(result.ok, true, result.failureReason ?? "");
  assert.equal(result.sessionRef, EXISTING.sessionRef);
  assert.equal(result.genesisRef, EXISTING.genesisRef);
  assert.equal(result.channelId, "chan-1");
  assert.deepEqual(deps.log, [
    `publish:lead:${EXISTING.sessionRef}:${EXISTING.genesisRef}`,
    "receipt:lead",
    "grant:aaaa",
    "seat:aaaa:lead",
    "turn",
  ]);
  const ids = result.steps.map((entry) => entry.id);
  assert.equal(ids.includes(CODING_SESSION_CREW_LAUNCH_CHANNEL_STEP), false);
  assert.equal(ids.includes(CODING_SESSION_CREW_LAUNCH_GENESIS_STEP), false);
  assert.deepEqual(
    planCodingSessionCrewLaunch({
      ...INPUT,
      seats: [LEAD],
      existingUmbrella: EXISTING,
    }).map((entry) => entry.id),
    ids,
    "the plan shown up front is the sequence the launch walked",
  );
});

test("an existing umbrella still gets its policy, between nothing and the lead's seat", async () => {
  const deps = recordingDeps({
    publishGenesis: async () => {
      throw new Error("a founded umbrella must not be founded twice");
    },
    publishPolicy: async ({ sessionRef, genesisRef }) => {
      deps.log.push(`policy:${sessionRef}:${genesisRef}`);
      return { eventId: "policy-1" };
    },
  });
  const result = await launchCodingSessionCrew(
    { ...INPUT, seats: [LEAD], policySet: true, existingUmbrella: EXISTING },
    deps,
  );
  assert.equal(result.ok, true, result.failureReason ?? "");
  assert.equal(result.policyEventId, "policy-1");
  assert.equal(
    deps.log[0],
    `policy:${EXISTING.sessionRef}:${EXISTING.genesisRef}`,
  );
  assert.equal(deps.log[1], "publish:lead");
});

test("an existing umbrella with no channel to seat into is refused before anything is signed", async () => {
  const deps = recordingDeps({
    ensureChannel: async () => {
      throw new Error("a second channel must never be minted");
    },
  });
  const result = await launchCodingSessionCrew(
    { ...INPUT, channelId: null, seats: [LEAD], existingUmbrella: EXISTING },
    deps,
  );
  assert.equal(result.ok, false);
  assert.equal(result.failedStep, CODING_SESSION_CREW_LAUNCH_FAMILY_STEP);
  assert.match(result.failureReason, /already founded in a channel/);
  assert.deepEqual(deps.log, [], "nothing may be signed");
  // The refusal still names the umbrella it was asked to start.
  assert.equal(result.sessionRef, EXISTING.sessionRef);
  assert.equal(result.genesisRef, EXISTING.genesisRef);
  assert.equal(result.channelId, null);
});
