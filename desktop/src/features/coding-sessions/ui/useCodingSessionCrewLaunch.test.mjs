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
async function harness({ goalFailure = null } = {}) {
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
    createWorktree: async (input) => ({ path: `/tmp/trees/${input.name}` }),
    newSessionRef: () => SESSION_REF,
    newSeatCommandId: () => "csl-seat-1",
    newTurnCommandId: () => "csc-turn-1",
    ensureProviderMembership: async () => {},
    publishGenesis: (input) => publishCodingSessionGenesis(input, client),
    publishGoal: async (input) => {
      if (goalFailure !== null) throw new Error(goalFailure);
      return publishCodingSessionGoal(input, client);
    },
    stageCreateHint: async () => {},
    recordWorkdirUse: async () => {},
    seatDeps: {
      ensureMembership: async () => {},
      stageSeat: async () => ({ packStaged: true }),
      clearSeat: async () => {},
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
    result = await mounted.result.current.launch(LAUNCH_INPUT, RUNTIME_TARGET);
  });

  return {
    kinds: published.map((event) => event.kind),
    of: (kind) => published.filter((event) => event.kind === kind),
    published,
    result,
    teardown: () => mounted.unmount(),
  };
}

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
