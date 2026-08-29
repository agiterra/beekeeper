/**
 * The host, mounted, answering a real hire.
 *
 * Everything below the hook is the production code: the wire classifier, the
 * standing policy, the identity choice, the seat plan, the seated-create
 * ordering. Only the outside world is injected — the event bus, the worktree,
 * custody, the keystore, the relay and the clock — so what these assert is the
 * sequence a running desktop actually performs, in order, and the exact events
 * it puts on the wire.
 *
 * Live evidence for why this file exists: on 2026-08-28 a lead published a
 * valid `session.hire` and nothing answered for 60 s, because the hook was
 * written but never mounted (item 81, lane H residual).
 */
import assert from "node:assert/strict";
import { after, before, test } from "node:test";

import { JSDOM } from "jsdom";
import {
  finalizeEvent,
  generateSecretKey,
  getPublicKey,
} from "nostr-tools/pure";

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
const SESSION_REF = "5b7e1c2a-90d4-4b0e-a1f3-7c2d8e6f4a10";
const GENESIS_REF = "a".repeat(64);
const LEAD_SECRET = generateSecretKey();
const LEAD_PUBKEY = getPublicKey(LEAD_SECRET);
const OPERATOR_SECRET = generateSecretKey();
const OPERATOR_PUBKEY = getPublicKey(OPERATOR_SECRET);
const PROVIDER_PUBKEY = "9".repeat(64);
const ADA_PUBKEY = "d".repeat(64);
const HIRE_CREATED_AT = 1_700_000_000;

const LEAD_TARGET = {
  driver: "claude",
  instanceId: "claude-primary",
  sessionId: "lead-session",
  generation: 1,
};

const UMBRELLA = {
  umbrellaKey: SESSION_REF,
  sessionRef: SESSION_REF,
  genesisRef: GENESIS_REF,
  title: "Agent Teams",
  founderPubkey: OPERATOR_PUBKEY,
  executions: [
    {
      activeGeneration: {
        agentRef: LEAD_PUBKEY,
        role: "lead",
        status: "running",
      },
    },
  ],
};

// Exactly what `list_runtimes()` answers: every row's model list is the
// hardcoded placeholder `["default"]`
// (desktop/src-tauri/src/session_provider/runtimes.rs:269). Reading a catalog
// off this is what refused `sonnet` live on 2026-08-28 (item 88(a)).
const CLAUDE_RUNTIME = {
  instanceRef: "claude-primary",
  runtime: "claude",
  driver: "claude",
  label: "Claude Code",
  authState: "ready",
  defaultModel: "default",
  allowedModels: ["default"],
  capabilities: {},
};

const CODEX_RUNTIME = {
  instanceRef: "codex-primary",
  runtime: "codex",
  driver: "codex",
  label: "Codex",
  authState: "ready",
  defaultModel: "default",
  allowedModels: ["default"],
  capabilities: {},
};

/** What `coding_session_provider_models` answers — the published 44222 list. */
const CLAUDE_CATALOG = [
  "default",
  "claude-fable-5[1m]",
  "haiku",
  "opus[1m]",
  "sonnet",
];

async function signedHire({
  model = null,
  createdAt = HIRE_CREATED_AT,
  role = "builder",
  providerInstanceRef = "claude-primary",
} = {}) {
  const { buildCodingSessionHireEvent } = await import(
    "../lib/codingSessionHireWire.ts"
  );
  return finalizeEvent(
    {
      created_at: createdAt,
      ...buildCodingSessionHireEvent({
        channelId: CHANNEL_ID,
        commandId: "csl-hire-1",
        sessionRef: SESSION_REF,
        genesisRef: GENESIS_REF,
        role,
        providerInstanceRef,
        model,
        brief: "Take the badge lane. Red test first.",
      }),
    },
    LEAD_SECRET,
  );
}

/**
 * Mount the real host with the outside world recorded rather than performed.
 */
async function harness({
  policy,
  runtimes = [CLAUDE_RUNTIME],
  modelCatalogs = new Map([["claude-primary", CLAUDE_CATALOG]]),
  agents = [
    {
      pubkey: ADA_PUBKEY,
      name: "Ada",
      homeRole: "builder",
      hasRolePack: true,
      model: "opus[1m]",
    },
  ],
  umbrellas = [UMBRELLA],
  now = HIRE_CREATED_AT,
  receiptError = null,
  grantError = null,
} = {}) {
  const { act, render } = await import("@testing-library/react");
  const React = (await import("react")).default;
  const { DEFAULT_CODING_SESSION_HIRE_POLICY } = await import(
    "../lib/codingSessionHirePolicy.ts"
  );
  const { CodingSessionHireRunner } = await import(
    "./CodingSessionHireHost.tsx"
  );

  const steps = [];
  const published = [];
  const grants = [];
  let listener = null;

  const deps = {
    subscribe: (receive) => {
      listener = receive;
      return () => {
        listener = null;
      };
    },
    fetchRosterFold: async () => ({
      accepted: new Map([[LEAD_PUBKEY, "operator"]]),
    }),
    createWorktree: async (input) => {
      steps.push("worktree");
      return { path: `/tmp/trees/${input.name}` };
    },
    stageCreateHint: async () => {
      steps.push("hint");
    },
    seatDeps: {
      ensureMembership: async () => {
        steps.push("membership");
      },
      stageSeat: async () => {
        steps.push("custody");
        return { packStaged: true };
      },
      clearSeat: async () => {
        steps.push("custody-cleared");
      },
    },
    signer: async (input) => {
      steps.push(`sign:${input.kind}`);
      return finalizeEvent({ created_at: now, ...input }, OPERATOR_SECRET);
    },
    publisher: {
      publishEvent: async (event) => {
        steps.push(`publish:${event.kind}`);
        published.push(event);
        return event;
      },
    },
    awaitSeatReceipt: async ({ channelId, commandId }) => {
      steps.push("receipt");
      if (receiptError !== null) throw new Error(receiptError);
      return {
        driver: "claude",
        instanceId: "claude-primary",
        sessionId: `${channelId}:${commandId}`,
        generation: 1,
      };
    },
    grantOperator: async (input) => {
      steps.push("grant");
      grants.push(input);
      if (grantError !== null) throw new Error(grantError);
    },
    newSeatCommandId: () => "csl-seat-1",
    newTurnCommandId: () => "csc-refusal-1",
    now: () => now,
  };

  const view = render(
    React.createElement(CodingSessionHireRunner, {
      agents,
      channelIds: [CHANNEL_ID],
      modelCatalogs,
      checkoutForChannel: () => "/Users/brian/Projects/beekeeper",
      deps,
      operatorPubkey: OPERATOR_PUBKEY,
      policy: policy ?? DEFAULT_CODING_SESSION_HIRE_POLICY,
      providerAuthorityPubkey: PROVIDER_PUBKEY,
      runtimes,
      targetForActor: (channelId, actorPubkey) =>
        channelId === CHANNEL_ID && actorPubkey === LEAD_PUBKEY
          ? LEAD_TARGET
          : null,
      umbrellas,
    }),
  );

  const settle = async () => {
    for (let round = 0; round < 8; round += 1) {
      await act(async () => {
        await new Promise((resolve) => setTimeout(resolve, 0));
      });
    }
  };
  await settle();

  return {
    deliver: async (event) => {
      assert.ok(listener !== null, "the host never subscribed to 44221 events");
      await act(async () => {
        listener([event]);
      });
      await settle();
    },
    grants,
    of: (kind) => published.filter((event) => event.kind === kind),
    published,
    steps,
    teardown: () => view.unmount(),
  };
}

test("a hire is answered in order, and the seat it creates is granted", async () => {
  const host = await harness();
  await host.deliver(await signedHire());

  // The grant is the last step and it is not optional: a seated agent that
  // was never granted cannot answer the lead at all — the relay refuses its
  // `sessions send` with "only a session founder or a granted operator may
  // steer" (item 83, live 2026-08-28).
  assert.deepEqual(host.steps, [
    "worktree",
    "hint",
    "membership",
    "custody",
    "sign:44221",
    "publish:44221",
    "receipt",
    "grant",
  ]);
  assert.deepEqual(host.grants, [
    {
      channelId: CHANNEL_ID,
      genesisRef: GENESIS_REF,
      // The seat's own actor, never the lead that asked for it.
      granteePubkey: ADA_PUBKEY,
    },
  ]);

  const [create] = host.of(44221);
  assert.ok(create, "no seated create was published");
  const payload = JSON.parse(create.content);
  assert.equal(payload.action.type, "session.create");
  assert.equal(payload.action.actor, ADA_PUBKEY);
  assert.equal(payload.action.role, "builder");
  assert.equal(payload.commandId, "csl-seat-1");
  assert.equal(
    payload.action.initialTurn,
    "[From the lead] Take the badge lane. Red test first.",
  );
  host.teardown();
});

test("a refused hire comes back as a 44220 turn to the seat that asked", async () => {
  const host = await harness({
    policy: {
      enabled: false,
      allowedRoles: null,
      maxSeatsPerUmbrella: 4,
      allowedProviderInstanceRefs: null,
    },
  });
  await host.deliver(await signedHire());

  // Nothing was cut, staged or seated: a refusal spends nothing.
  assert.equal(host.steps.includes("worktree"), false);
  assert.equal(host.of(44221).length, 0);

  const [turn] = host.of(44220);
  assert.ok(turn, "the lead was never told");
  const payload = JSON.parse(turn.content);
  assert.deepEqual(payload.target, LEAD_TARGET);
  assert.match(payload.action.text, /^hire refused: HIRE_OFF — /);
  // And once more where the person who set the policy can see it.
  const [notice] = host.of(9);
  assert.ok(notice, "the umbrella was never told");
  assert.match(
    notice.content,
    /asked to hire a builder — hire refused: HIRE_OFF/,
  );
  host.teardown();
});

test("a hire older than the host's window is refused HIRE_STALE, never seated", async () => {
  const host = await harness({ now: HIRE_CREATED_AT + 16 * 60 });
  await host.deliver(await signedHire());

  assert.equal(host.of(44221).length, 0);
  const [turn] = host.of(44220);
  assert.ok(turn, "the lead was never told");
  assert.match(
    JSON.parse(turn.content).action.text,
    /^hire refused: HIRE_STALE — this hire request is older than the host's window/,
  );
  host.teardown();
});

test("a vendor model id is translated onto the catalog's, and said out loud", async () => {
  const host = await harness();
  await host.deliver(await signedHire({ model: "claude-sonnet-5" }));

  const [create] = host.of(44221);
  assert.ok(create, "no seated create was published");
  assert.equal(JSON.parse(create.content).action.model, "sonnet");
  const [notice] = host.of(9);
  assert.ok(notice, "the substitution was never disclosed");
  assert.match(notice.content, /^Hired a builder — /);
  assert.match(notice.content, /claude-sonnet-5/);
  host.teardown();
});

test("a model no runtime offers is refused with the offered ids, not guessed", async () => {
  const host = await harness({
    modelCatalogs: new Map([
      ["claude-primary", ["default", "claude-fable-5[1m]"]],
    ]),
  });
  await host.deliver(await signedHire({ model: "claude-sonnet-5" }));

  assert.equal(host.of(44221).length, 0);
  const text = JSON.parse(host.of(44220)[0].content).action.text;
  assert.match(text, /^hire refused: HIRE_MODEL_NOT_OFFERED — /);
  assert.match(text, /default, claude-fable-5\[1m\]/);
  host.teardown();
});

test("the same hire observed twice is answered once", async () => {
  const host = await harness();
  const hire = await signedHire();
  await host.deliver(hire);
  await host.deliver(hire);

  assert.equal(host.of(44221).length, 1);
  host.teardown();
});

test("a grant that fails is said to the lead and to the umbrella, never silently", async () => {
  const host = await harness({
    grantError: "the relay refused the transition",
  });
  await host.deliver(await signedHire());

  // The seat is real — a failed grant does not un-create it — and that is
  // exactly why it has to be disclosed: the agent will work and its report
  // will bounce.
  assert.equal(host.of(44221).length, 1);

  const [turn] = host.of(44220);
  assert.ok(turn, "the lead was never told the seat cannot report");
  const payload = JSON.parse(turn.content);
  assert.deepEqual(payload.target, LEAD_TARGET);
  assert.equal(
    payload.action.text,
    "seated, but not granted: the relay refused the transition — it cannot report until granted",
  );

  const [notice] = host.of(9);
  assert.ok(notice, "the umbrella was never told");
  assert.match(
    notice.content,
    /^Hired a builder — seated, but not granted: the relay refused the transition — it cannot report until granted$/,
  );
  host.teardown();
});

test("a create receipt that never lands is disclosed as ungranted, not as granted", async () => {
  const host = await harness({
    receiptError: "The provider did not answer within the wait",
  });
  await host.deliver(await signedHire());

  // No grant was attempted: granting an identity whose create the provider
  // never confirmed would hand authority to a seat that may not exist.
  assert.equal(host.steps.includes("grant"), false);
  const [turn] = host.of(44220);
  assert.ok(turn, "the lead was never told");
  assert.match(
    JSON.parse(turn.content).action.text,
    /^seated, but not granted: The provider did not answer within the wait — it cannot report until granted$/,
  );
  host.teardown();
});

// --- item 88(a),(c),(i): the live DogFood2 findings, at the host ------------

test("a model the provider's catalog offers is seated, not refused by the runtime table", async () => {
  // Live 2026-08-28: `--model sonnet` came back HIRE_MODEL_NOT_OFFERED "It
  // offers default", because the offered list was `list_runtimes()`'s
  // hardcoded `["default"]` rather than `coding_session_provider_models`.
  const host = await harness();
  await host.deliver(await signedHire({ model: "sonnet" }));

  assert.equal(host.of(44220).length, 0, "the hire was refused");
  const [create] = host.of(44221);
  assert.ok(create, "no seated create was published");
  assert.equal(JSON.parse(create.content).action.model, "sonnet");
  host.teardown();
});

test("the refusal sentence lists the provider catalog verbatim", async () => {
  const host = await harness();
  await host.deliver(await signedHire({ model: "gpt-5.6-sol" }));

  const text = JSON.parse(host.of(44220)[0].content).action.text;
  assert.match(text, /^hire refused: HIRE_MODEL_NOT_OFFERED — /);
  assert.match(
    text,
    /It offers default, claude-fable-5\[1m\], haiku, opus\[1m\], sonnet\./,
  );
  host.teardown();
});

test("a codex identity is seated on codex, with its own model", async () => {
  const host = await harness({
    runtimes: [CLAUDE_RUNTIME, CODEX_RUNTIME],
    modelCatalogs: new Map([
      ["claude-primary", CLAUDE_CATALOG],
      ["codex-primary", ["gpt-5.6-sol"]],
    ]),
    agents: [
      {
        pubkey: ADA_PUBKEY,
        name: "Banksy",
        homeRole: "designer",
        hasRolePack: true,
        model: "gpt-5.6-sol",
        runtime: "codex",
      },
    ],
  });
  await host.deliver(await signedHire({ role: "designer" }));

  const [create] = host.of(44221);
  assert.ok(create, "no seated create was published");
  const action = JSON.parse(create.content).action;
  assert.equal(action.providerInstanceRef, "codex-primary");
  assert.equal(action.model, "gpt-5.6-sol");
  // The hire named claude-primary; the host overrode it, so it says so.
  const [notice] = host.of(9);
  assert.ok(notice, "the runtime substitution was never disclosed");
  assert.match(notice.content, /codex-primary/);
  host.teardown();
});

test("the hired seat's create carries the umbrella's project", async () => {
  const host = await harness({
    umbrellas: [
      {
        ...UMBRELLA,
        executions: [
          {
            activeGeneration: {
              agentRef: LEAD_PUBKEY,
              role: "lead",
              status: "running",
              projectRef: "30621:owner:beekeeper",
            },
          },
        ],
      },
    ],
  });
  await host.deliver(await signedHire());

  const [create] = host.of(44221);
  assert.ok(create, "no seated create was published");
  assert.equal(
    JSON.parse(create.content).action.projectRef,
    "30621:owner:beekeeper",
  );
  host.teardown();
});
