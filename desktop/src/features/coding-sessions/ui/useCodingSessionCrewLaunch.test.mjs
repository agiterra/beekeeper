/**
 * The team launch, mounted, publishing the goal it was given.
 *
 * Live on 2026-08-31 (item 103, finding 5): a team launched with a written
 * goal sent it as the lead's first turn and published no kind:44227 at all.
 * The goal pill was empty for the whole run, and every seat the lead hired was
 * hired against a goal that existed only inside one agent's transcript.
 *
 * The launch sequence itself is the real `launchCodingSessionCrew`, and the
 * genesis, goal and create events below are built by the real builders — only
 * the relay, the keyring and this computer's disk are faked, so the order
 * asserted here is the order a running desktop publishes in.
 */
import assert from "node:assert/strict";
import { after, before, test } from "node:test";

import { JSDOM } from "jsdom";
import { finalizeEvent, generateSecretKey } from "nostr-tools/pure";

const dom = new JSDOM("<!doctype html><html><body></body></html>", {
  url: "http://localhost",
});

before(() => {
  Object.assign(globalThis, {
    document: dom.window.document,
    HTMLElement: dom.window.HTMLElement,
    IS_REACT_ACT_ENVIRONMENT: true,
    localStorage: dom.window.localStorage,
    window: dom.window,
  });
});

after(() => dom.window.close());

const CHANNEL_ID = "3d2a7b18-9b7a-4a41-9a86-6a52a1c0b7e1";
const SESSION_REF = "11111111-1111-4111-8111-111111111111";
const OPERATOR_SECRET = generateSecretKey();
const PROVIDER_PUBKEY = "9".repeat(64);
const GOAL = "Close ledger item 104: the goal reaches the wire.";

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
  model: "claude-opus-5",
  vendor: null,
};

const LAUNCH_INPUT = {
  channelId: CHANNEL_ID,
  goal: GOAL,
  seats: [LEAD, BUILDER],
  primaryPersonaId: "p-lead",
  provider: {
    label: "claude-agent-acp",
    allowedModels: ["claude-opus-5"],
    instanceRef: "claude-primary",
  },
};

const RUNTIME_TARGET = {
  provider: { providerInstanceRef: "claude-primary" },
  signerPubkey: PROVIDER_PUBKEY,
};

/**
 * Mount the hook with one fake relay client recording every signed event.
 *
 * `goalFailure`, when given, is the message the goal publish answers with.
 */
async function harness({
  goalFailure = null,
  launchInput = LAUNCH_INPUT,
  workdir = null,
  rememberWorkspace,
  /** The project's 30624 the host's reader answers with, by project ref. */
  packSources = new Map(),
} = {}) {
  /** Every custody and pack-source call this host made, in order. */
  const staging = [];
  const worktrees = [];
  const hints = [];
  const rememberedWorkdirs = [];
  const { act, renderHook } = await import("@testing-library/react");
  const { useCodingSessionCrewLaunch } = await import(
    "./useCodingSessionCrewLaunch.ts"
  );
  const { publishCodingSessionGenesis } = await import(
    "../lib/codingSessionGenesis.ts"
  );
  const { publishCodingSessionGoal } = await import(
    "../lib/codingSessionGoal.ts"
  );
  const { launchCodingSessionCrew } = await import(
    "../lib/codingSessionCrewLaunch.ts"
  );

  const published = [];
  const signer = async (input) =>
    finalizeEvent({ created_at: 1_700_000_000, ...input }, OPERATOR_SECRET);
  const publisher = {
    publishEvent: async (event) => {
      published.push(event);
      return event;
    },
  };
  const client = { publisher, signer };

  const deps = {
    runLaunch: launchCodingSessionCrew,
    createWorktree: async (input) => {
      worktrees.push(input);
      return { path: `/tmp/trees/${input.name}` };
    },
    newSessionRef: () => SESSION_REF,
    newSeatCommandId: () => "csl-seat-1",
    newTurnCommandId: () => "csc-turn-1",
    ensureProviderMembership: async () => {},
    publishGenesis: (input) => publishCodingSessionGenesis(input, client),
    publishGoal: async (input) => {
      if (goalFailure !== null) throw new Error(goalFailure);
      return publishCodingSessionGoal(input, client);
    },
    stageCreateHint: async (input) => hints.push(input),
    recordWorkdirUse: async (path) => rememberedWorkdirs.push(path),
    seatDeps: {
      ensureMembership: async () => {},
      stageSeat: async (input) => {
        staging.push(["stageSeat", input]);
        return {
          packStaged: true,
          packRef:
            input.packSource === null
              ? null
              : {
                  repo: input.packSource.repo,
                  sha: "dd935f43".padEnd(40, "0"),
                  role: input.role,
                  path: `${input.packSource.path}/${input.role}`,
                },
        };
      },
      clearSeat: async () => {},
      fetchPackSource: async (projectRef) => {
        staging.push(["fetchPackSource", projectRef]);
        return packSources.get(projectRef) ?? null;
      },
    },
    signer,
    publisher,
    recordPendingLifecycle: () => {},
    awaitSeatReceipt: async ({ commandId }) => ({
      driver: "claude-agent-acp",
      instanceId: "claude-primary",
      sessionId: `sess-${commandId}`,
      generation: 1,
    }),
    ensureCreateOperatorGrants: async () => ({ ok: true }),
    ensureSeatGrant: async () => ({ ok: true }),
    publishCommand: async () => ({}),
  };

  const mounted = renderHook(() =>
    useCodingSessionCrewLaunch({
      workdir,
      rememberWorkspace,
      title: null,
      deps,
    }),
  );

  let result = null;
  await act(async () => {
    result = await mounted.result.current.launch(launchInput, RUNTIME_TARGET);
  });

  return {
    kinds: published.map((event) => event.kind),
    of: (kind) => published.filter((event) => event.kind === kind),
    published,
    result,
    staging,
    worktrees,
    hints,
    rememberedWorkdirs,
    teardown: () => mounted.unmount(),
  };
}

test("contextual governed launch stages the reused folder without remembering or copying it", async () => {
  const path = "C:\\Users\\WinBrian\\Projects\\beekeeper-wt-existing";
  const host = await harness({
    workdir: path,
    rememberWorkspace: false,
    launchInput: { ...LAUNCH_INPUT, workdir: path, leadWorktree: null },
  });
  assert.equal(host.result.ok, true, host.result.failureReason ?? "");
  assert.deepEqual(host.worktrees, []);
  // A reused folder is pinned for the command but never remembered for the
  // project: `projectRef` is null, exactly as the solo path stages it.
  assert.deepEqual(host.hints, [
    { commandId: "csl-seat-1", path, projectRef: null, rememberPath: path },
  ]);
  assert.deepEqual(host.rememberedWorkdirs, []);
  assert.equal(host.of(44221).length, 1);
  host.teardown();
});

test("ordinary governed worktree creation remembers the checkout, not the new execution folder", async () => {
  const checkout = "/repo/primary";
  const host = await harness({
    workdir: checkout,
    launchInput: {
      ...LAUNCH_INPUT,
      workdir: checkout,
      leadWorktree: { name: "fresh-work", source: "main" },
    },
  });
  assert.equal(host.result.ok, true, host.result.failureReason ?? "");
  assert.deepEqual(host.worktrees, [
    { workdir: checkout, name: "fresh-work", source: "main" },
  ]);
  assert.deepEqual(host.hints, [
    {
      commandId: "csl-seat-1",
      path: "/tmp/trees/fresh-work",
      projectRef: null,
      rememberPath: checkout,
    },
  ]);
  assert.deepEqual(host.rememberedWorkdirs, [checkout]);
  host.teardown();
});

test("a project team launch records the checkout as the project's folder, as the solo path does", async () => {
  // The first RPG Test team session (2026-09-19) recorded only the MRU:
  // `byProject` stayed empty, and the next session pre-filled another
  // project's checkout. The hint now carries the project coordinate and the
  // checkout (not the worktree cut from it), which is what writes
  // `byProject` (`useNewCodingSessionCreate.ts` stages the same).
  const checkout = "/repo/rpg-test";
  const PROJECT_REF = `30621:${"3d3b7169".padEnd(64, "0")}:rpg-test`;
  const host = await harness({
    workdir: checkout,
    launchInput: {
      ...LAUNCH_INPUT,
      projectRef: PROJECT_REF,
      workdir: checkout,
      leadWorktree: { name: "fresh-work", source: "main" },
    },
  });
  assert.equal(host.result.ok, true, host.result.failureReason ?? "");
  assert.deepEqual(host.hints, [
    {
      commandId: "csl-seat-1",
      path: "/tmp/trees/fresh-work",
      projectRef: PROJECT_REF,
      rememberPath: checkout,
    },
  ]);
  assert.deepEqual(host.rememberedWorkdirs, [checkout]);
  host.teardown();
});

test("A2.2: the launch publishes the goal, once, before the lead's create", async () => {
  const host = await harness();

  assert.equal(host.result.ok, true, host.result.failureReason ?? "");
  // Genesis, then the goal, then the lead's create. The goal is a fact about
  // the umbrella, so it is on the wire before any seat that has to read it.
  assert.deepEqual(host.kinds, [44226, 44227, 44221]);

  const goals = host.of(44227);
  assert.equal(goals.length, 1, "exactly one 44227 per launch");
  const [goal] = goals;
  assert.equal(goal.content, GOAL);
  assert.deepEqual(goal.tags, [
    ["h", CHANNEL_ID],
    // The umbrella's own session ref — the same one the genesis and the
    // creates carry, so the pill reads this launch's goal and not another's.
    ["d", SESSION_REF],
    ["csgl-v", "csgl1-1"],
  ]);
  assert.deepEqual(host.result.goal, { published: true, reason: null });
  host.teardown();
});

test("A2.2: a goal that will not publish is disclosed, and the team still launches", async () => {
  const host = await harness({
    goalFailure: "Failed to update the session goal.",
  });

  // The launch is not aborted: the first turn still carries the words, so a
  // team with an unpublished goal is worth more than no team.
  assert.equal(host.result.ok, true, host.result.failureReason ?? "");
  assert.deepEqual(host.kinds, [44226, 44221]);
  assert.deepEqual(host.result.goal, {
    published: false,
    reason: "Failed to update the session goal.",
  });
  host.teardown();
});

test("F1: an unpublished goal always carries a reason, never a blank one", async () => {
  // A publisher that fails with nothing to say is the one case that could
  // render as "no goal" with no explanation — the silence this path exists
  // to end.
  const host = await harness({ goalFailure: "   " });

  assert.equal(host.result.ok, true, host.result.failureReason ?? "");
  assert.deepEqual(host.kinds, [44226, 44221]);
  assert.equal(host.result.goal.published, false);
  assert.equal(host.result.goal.reason, "the goal publish did not go out");
  assert.notEqual(host.result.goal.reason.trim(), "");
  host.teardown();
});

/**
 * Finding 84: a team launch staged the lead from this computer's copy of the
 * role pack. The project names a packs repository (kind:30624), the launch
 * dialog's preview read it, and the launch's staging call never did.
 */
test("finding 84: a team launch stages the lead from the project's pack source", async () => {
  const OWNER = "3d3b7169".padEnd(64, "0");
  const PROJECT_REF = `30621:${OWNER}:beekeeper`;
  const PACK_SOURCE = {
    repo: `30617:${OWNER}:agiterra-packs`,
    gitRef: "refs/heads/main",
    sha: null,
    path: "personas/roles",
  };
  const host = await harness({
    launchInput: { ...LAUNCH_INPUT, projectRef: PROJECT_REF },
    packSources: new Map([[PROJECT_REF, PACK_SOURCE]]),
  });

  assert.equal(host.result.ok, true, host.result.failureReason ?? "");
  assert.deepEqual(
    host.staging.map(([name]) => name),
    ["fetchPackSource", "stageSeat"],
  );
  assert.deepEqual(host.staging[0], ["fetchPackSource", PROJECT_REF]);
  const [, staged] = host.staging[1];
  assert.equal(staged.agentPubkey, LEAD.actor);
  assert.equal(staged.role, "lead");
  assert.deepEqual(staged.packSource, PACK_SOURCE);

  // The step list names where the pack came from, on the seat's own row.
  const leadStep = host.result.steps.find((step) =>
    step.id.startsWith("create:"),
  );
  assert.ok(leadStep, `no seat step in ${JSON.stringify(host.result.steps)}`);
  assert.equal(
    leadStep.detail,
    `staged from 30617:3d3b7169…:agiterra-packs@dd935f43`,
  );
  host.teardown();
});

test("finding 84: a launch with no project stages with no source, as before", async () => {
  const host = await harness();

  assert.equal(host.result.ok, true, host.result.failureReason ?? "");
  assert.deepEqual(
    host.staging.map(([name]) => name),
    ["stageSeat"],
  );
  assert.equal(host.staging[0][1].packSource, null);
  const leadStep = host.result.steps.find((step) =>
    step.id.startsWith("create:"),
  );
  assert.ok(leadStep, "the lead's seat step exists");
  assert.equal(leadStep.detail, null);
  host.teardown();
});

/**
 * Starting a founded umbrella: the genesis dep — where the founding path
 * ensures the provider's membership and publishes the goal — is never
 * called, so membership has to be ensured before the seat's create on its
 * own, and the goal outcome must describe the 44227 that is already on the
 * wire rather than one this launch never published.
 */
test("starting a founded umbrella publishes only the lead's create, joins the provider first, and claims no goal of its own", async () => {
  const EXISTING = {
    sessionRef: "22222222-2222-4222-8222-222222222222",
    genesisRef: "f".repeat(64),
  };
  const membership = [];
  const { act, renderHook } = await import("@testing-library/react");
  const { useCodingSessionCrewLaunch } = await import(
    "./useCodingSessionCrewLaunch.ts"
  );
  const { launchCodingSessionCrew } = await import(
    "../lib/codingSessionCrewLaunch.ts"
  );
  const published = [];
  const signer = async (input) =>
    finalizeEvent({ created_at: 1_700_000_000, ...input }, OPERATOR_SECRET);
  const publisher = {
    publishEvent: async (event) => {
      published.push(event);
      return event;
    },
  };
  const deps = {
    runLaunch: launchCodingSessionCrew,
    createWorktree: async () => {
      throw new Error("no worktree was asked for");
    },
    newSessionRef: () => {
      throw new Error("a founded umbrella already has a session ref");
    },
    newSeatCommandId: () => "csl-seat-1",
    newTurnCommandId: () => "csc-turn-1",
    ensureProviderMembership: async (input) => {
      // Recorded with how many events had gone out by then: the join must
      // precede the create, or the provider never sees it.
      membership.push({ ...input, publishedSoFar: published.length });
    },
    publishGenesis: async () => {
      throw new Error("a founded umbrella must not be founded twice");
    },
    publishGoal: async () => {
      throw new Error(
        "the goal is already on the wire; nothing here publishes one",
      );
    },
    buildPolicyEvent: async () => {
      throw new Error("no policy was set");
    },
    stageCreateHint: async () => {},
    recordWorkdirUse: async () => {},
    seatDeps: {
      ensureMembership: async () => {},
      stageSeat: async () => ({ packStaged: true, packRef: null }),
      clearSeat: async () => {},
      fetchPackSource: async () => null,
    },
    signer,
    publisher,
    recordPendingLifecycle: () => {},
    awaitSeatReceipt: async ({ commandId }) => ({
      driver: "claude-agent-acp",
      instanceId: "claude-primary",
      sessionId: `sess-${commandId}`,
      generation: 1,
    }),
    ensureCreateOperatorGrants: async () => ({ ok: true }),
    ensureSeatGrant: async () => ({ ok: true }),
    publishCommand: async () => ({}),
  };
  const mounted = renderHook(() =>
    useCodingSessionCrewLaunch({ workdir: null, title: null, deps }),
  );
  let result = null;
  await act(async () => {
    result = await mounted.result.current.launch(
      { ...LAUNCH_INPUT, seats: [LEAD], existingUmbrella: EXISTING },
      RUNTIME_TARGET,
    );
  });

  assert.equal(result.ok, true, result.failureReason ?? "");
  assert.deepEqual(
    published.map((event) => event.kind),
    [44221],
    "no genesis, no goal — one create, under the umbrella that exists",
  );
  const create = published[0];
  // The refs ride in the signed content, exactly as the join path signs
  // them, so the provider mints the execution inside this umbrella.
  assert.ok(
    create.content.includes(EXISTING.sessionRef),
    `the create carries the umbrella's session ref: ${create.content}`,
  );
  assert.ok(
    create.content.includes(EXISTING.genesisRef),
    `the create carries the umbrella's genesis ref: ${create.content}`,
  );
  assert.deepEqual(membership, [
    {
      channelId: CHANNEL_ID,
      providerPubkey: PROVIDER_PUBKEY,
      publishedSoFar: 0,
    },
  ]);
  assert.deepEqual(result.goal, { published: true, reason: null });
  assert.equal(result.sessionRef, EXISTING.sessionRef);
  assert.equal(result.genesisRef, EXISTING.genesisRef);
  mounted.unmount();
});
