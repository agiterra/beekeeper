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
import { readFileSync } from "node:fs";
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
const PROJECT_REF = `30621:${"3d3b7169".padEnd(64, "0")}:beekeeper`;
/** Ada, as an agent of {@link PROJECT_REF}: hiring there seats only its own. */
const PROJECT_ADA = {
  pubkey: ADA_PUBKEY,
  name: "Ada",
  homeRole: "builder",
  projectRef: PROJECT_REF,
  hasRolePack: true,
  model: "opus[1m]",
};

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
  routing,
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
        ...(routing === undefined ? {} : { routing }),
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
  catalogForHire,
  registryForProject,
  agents = [
    {
      pubkey: ADA_PUBKEY,
      name: "Ada",
      homeRole: "builder",
      projectRef: null,
      hasRolePack: true,
      model: "opus[1m]",
    },
  ],
  umbrellas = [UMBRELLA],
  now = HIRE_CREATED_AT,
  receiptError = null,
  receiptInstanceRef = "claude-primary",
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
        instanceId: receiptInstanceRef,
        sessionId: `${channelId}:${commandId}`,
        generation: 1,
      };
    },
    ensureOperatorGrant: async (input) => {
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
      ...(catalogForHire === undefined ? {} : { catalogForHire }),
      channelIds: [CHANNEL_ID],
      modelCatalogs,
      checkoutForChannel: () => "/Users/brian/Projects/beekeeper",
      deps,
      operatorPubkey: OPERATOR_PUBKEY,
      policy: policy ?? DEFAULT_CODING_SESSION_HIRE_POLICY,
      providerAuthorityPubkey: PROVIDER_PUBKEY,
      ...(registryForProject === undefined ? {} : { registryForProject }),
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
    "grant",
  ]);
  assert.deepEqual(host.grants, [
    {
      channelId: CHANNEL_ID,
      genesisRef: GENESIS_REF,
      // Provider wake is made authoritative before the actor can report.
      granteePubkey: PROVIDER_PUBKEY,
    },
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

// Brian's ruling, 2026-08-29: the catalog is the only model list. This host
// used to translate `claude-sonnet-5` onto the catalog's `sonnet`, seat the
// create on it and disclose the swap; a seat now runs the id it was asked for
// or none at all, so the whole catalog — including `sonnet` — refuses a vendor
// name it does not publish.
test("a vendor model id the catalog does not publish is refused, not translated", async () => {
  const host = await harness();
  await host.deliver(await signedHire({ model: "claude-sonnet-5" }));

  assert.equal(host.of(44221).length, 0, "an unoffered model was seated");
  const text = JSON.parse(host.of(44220)[0].content).action.text;
  assert.match(text, /^hire refused: HIRE_MODEL_NOT_OFFERED — /);
  assert.match(text, /claude-sonnet-5/);
  assert.match(
    text,
    /It offers default, claude-fable-5\[1m\], haiku, opus\[1m\], sonnet\./,
  );
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
    "seated, but not granted: provider wake authority: the relay refused the transition — it cannot report until granted",
  );

  const [notice] = host.of(9);
  assert.ok(notice, "the umbrella was never told");
  assert.match(
    notice.content,
    /^Hired a builder — seated, but not granted: provider wake authority: the relay refused the transition — it cannot report until granted$/,
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

test("a receipt target instance is not compared with the provider alias", async () => {
  const host = await harness({ receiptInstanceRef: "remote-claude" });
  await host.deliver(await signedHire());

  assert.equal(host.steps.filter((step) => step === "grant").length, 2);
  assert.equal(host.of(44220).length, 0, "a valid receipt was misclassified");
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
    agents: [PROJECT_ADA],
    umbrellas: [
      {
        ...UMBRELLA,
        executions: [
          {
            activeGeneration: {
              agentRef: LEAD_PUBKEY,
              role: "lead",
              status: "running",
              projectRef: PROJECT_REF,
            },
          },
        ],
      },
    ],
  });
  await host.deliver(await signedHire());

  const [create] = host.of(44221);
  assert.ok(create, "no seated create was published");
  assert.equal(JSON.parse(create.content).action.projectRef, PROJECT_REF);
  host.teardown();
});

test("a routed hire reads its own project and records the signed channel catalog revision", async () => {
  const projectRef = PROJECT_REF;
  const catalogReads = [];
  const registryReads = [];
  const registryText = readFileSync(
    new URL("../../../../../team/model-registry.yaml", import.meta.url),
    "utf8",
  );
  const host = await harness({
    agents: [PROJECT_ADA],
    umbrellas: [
      {
        ...UMBRELLA,
        executions: [
          {
            activeGeneration: {
              agentRef: LEAD_PUBKEY,
              role: "lead",
              status: "running",
              projectRef,
            },
          },
        ],
      },
    ],
    catalogForHire: (input) => {
      catalogReads.push(input);
      return {
        modelCatalogs: new Map([["claude-primary", CLAUDE_CATALOG]]),
        catalogRevision: 12,
        eventId: "1".repeat(64),
      };
    },
    registryForProject: async (input) => {
      registryReads.push(input);
      return {
        kind: "readable",
        text: registryText,
        label: "/checkout/team/model-registry.yaml",
      };
    },
  });

  await host.deliver(
    await signedHire({
      routing: {
        class: "builder",
        risk: { impact: 1, uncertainty: 1, irreversibility: 2 },
      },
    }),
  );

  assert.deepEqual(catalogReads, [{ channelId: CHANNEL_ID, projectRef }]);
  assert.deepEqual(registryReads, [projectRef]);
  const [create] = host.of(44221);
  assert.ok(create, "the routed hire was not seated");
  const action = JSON.parse(create.content).action;
  assert.equal(action.projectRef, projectRef);
  assert.equal(action.routing.catalogRevision, 12);
  assert.ok(CLAUDE_CATALOG.includes(action.routing.chosen.model));
  host.teardown();
});

/**
 * A hire signed with a payload this host cannot read.
 *
 * Built by hand rather than through `buildCodingSessionHireEvent`, because the
 * builder validates and would refuse to produce one — which is the point: the
 * bad payload comes off the relay, from another machine's emitter, and this
 * host is the one that has to answer it.
 */
async function signedMalformedHire(routing) {
  const { KIND_CODING_SESSION_LIFECYCLE_COMMAND } = await import(
    "@/shared/constants/kinds"
  );
  return finalizeEvent(
    {
      created_at: HIRE_CREATED_AT,
      kind: KIND_CODING_SESSION_LIFECYCLE_COMMAND,
      tags: [
        ["h", CHANNEL_ID],
        ["csl-v", "csl1-1"],
        ["csl-command", "csl-hire-bad"],
      ],
      content: JSON.stringify({
        schema: "buzz-coding-session-lifecycle-command/v1",
        commandId: "csl-hire-bad",
        action: {
          type: "session.hire",
          sessionRef: SESSION_REF,
          genesisRef: GENESIS_REF,
          role: "builder",
          providerInstanceRef: "claude-primary",
          model: null,
          brief: "Take the badge lane. Red test first.",
          routing,
        },
      }),
    },
    LEAD_SECRET,
  );
}

/** The exact payload the CLI published on 2026-08-30: the RECORD, on a hire. */
const ROUTING_RECORD_ON_A_HIRE = {
  class: "builder",
  tier: "standard",
  risk: { impact: 3, uncertainty: 3, irreversibility: 2, score: 18 },
  profile: null,
  chosen: { provider: "claude-primary", model: "sonnet", effort: "medium" },
  runnerUp: null,
  reason: "cleared the builder gates",
  reviewRequired: false,
  reviewReasons: [],
  challengerSample: false,
  override: null,
  registryVersion: 1,
  catalogRevision: 7,
};

test("a hire this host cannot read is refused HIRE_MALFORMED, never dropped", async () => {
  const { readCodingSessionHireOutcomes } = await import(
    "../hooks/useCodingSessionHire.ts"
  );
  const warnings = [];
  const realWarn = console.warn;
  console.warn = (...args) => warnings.push(args.join(" "));
  const host = await harness();
  try {
    await host.deliver(await signedMalformedHire(ROUTING_RECORD_ON_A_HIRE));
  } finally {
    console.warn = realWarn;
  }

  // Nothing was seated, and nothing was spent.
  assert.equal(host.of(44221).length, 0);
  assert.equal(host.steps.includes("worktree"), false);

  // 1. The lead is told, by name.
  const [turn] = host.of(44220);
  assert.ok(turn, "the lead was never told the hire was unreadable");
  const text = JSON.parse(turn.content).action.text;
  assert.match(text, /^hire refused: HIRE_MALFORMED — action\.routing\.tier: /);
  assert.match(text, /the host derives the tier from risk/);
  assert.deepEqual(JSON.parse(turn.content).target, LEAD_TARGET);

  // 2. The person who set the policy is told too.
  const [notice] = host.of(9);
  assert.ok(notice, "the umbrella was never told");
  assert.match(notice.content, /hire refused: HIRE_MALFORMED/);

  // 3. This machine keeps a record of what it threw away.
  assert.equal(warnings.length, 1);
  assert.match(warnings[0], /HIRE_MALFORMED — action\.routing\.tier/);

  // 4. And it is countable, so a surface can show it.
  const outcomes = readCodingSessionHireOutcomes();
  assert.equal(outcomes.length, 1);
  assert.equal(outcomes[0].state, "malformed");
  assert.match(outcomes[0].detail, /^action\.routing\.tier: /);
  host.teardown();
});

test("a hire carrying the contract's own request shape is not malformed", async () => {
  const host = await harness();
  await host.deliver(
    await signedMalformedHire({
      class: "builder",
      risk: { impact: 3, uncertainty: 3, irreversibility: 2 },
    }),
  );
  // It is read as a hire, so it reaches the router — and this host has no
  // registry in the test harness, so the honest refusal is HIRE_NO_ROUTE, not
  // a shape complaint.
  const text = JSON.parse(host.of(44220)[0].content).action.text;
  assert.match(text, /^hire refused: HIRE_NO_ROUTE — /);
  assert.match(text, /registry not readable on this host/);
  host.teardown();
});

test("a malformed hire from a stranger is recorded but never answered", async () => {
  const stranger = generateSecretKey();
  const { KIND_CODING_SESSION_LIFECYCLE_COMMAND } = await import(
    "@/shared/constants/kinds"
  );
  const event = finalizeEvent(
    {
      created_at: HIRE_CREATED_AT,
      kind: KIND_CODING_SESSION_LIFECYCLE_COMMAND,
      tags: [
        ["h", CHANNEL_ID],
        ["csl-v", "csl1-1"],
        ["csl-command", "csl-hire-stranger"],
      ],
      content: JSON.stringify({
        schema: "buzz-coding-session-lifecycle-command/v1",
        commandId: "csl-hire-stranger",
        action: {
          type: "session.hire",
          sessionRef: SESSION_REF,
          genesisRef: GENESIS_REF,
          role: "builder",
          providerInstanceRef: "claude-primary",
          model: null,
          brief: "seat me",
          routing: ROUTING_RECORD_ON_A_HIRE,
        },
      }),
    },
    stranger,
  );
  const { readCodingSessionHireOutcomes } = await import(
    "../hooks/useCodingSessionHire.ts"
  );
  const realWarn = console.warn;
  console.warn = () => {};
  const host = await harness();
  try {
    await host.deliver(event);
  } finally {
    console.warn = realWarn;
  }

  // Answering would tell an unknown pubkey that this computer is listening and
  // will sign events on request. It is still counted, and still logged.
  assert.equal(host.of(44220).length, 0);
  assert.equal(host.of(9).length, 0);
  assert.equal(readCodingSessionHireOutcomes()[0]?.state, "malformed");
  host.teardown();
});
