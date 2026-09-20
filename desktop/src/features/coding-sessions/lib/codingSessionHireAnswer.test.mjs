import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import test from "node:test";

import {
  codingSessionHireModelNoticeLine,
  codingSessionHireRefusalNotice,
  planCodingSessionHireAnswer,
} from "./codingSessionHireAnswer.ts";
import { DEFAULT_CODING_SESSION_HIRE_POLICY } from "./codingSessionHirePolicy.ts";

const SESSION_REF = "5b7e1c2a-90d4-4b0e-a1f3-7c2d8e6f4a10";
const GENESIS_REF = "a".repeat(64);
const CHANNEL_ID = "3d2a7b18-9b7a-4a41-9a86-6a52a1c0b7e1";
const FOUNDER = "f".repeat(64);
const LEAD = "1".repeat(64);
const STRANGER = "2".repeat(64);
const ADA = "d".repeat(64);
const PROVIDER = "9".repeat(64);
const OWNER = "3d3b7169".padEnd(64, "0");
/** The umbrella's project. Candidates carry it: hiring is project-scoped. */
const PROJECT_REF = `30621:${OWNER}:beekeeper`;

const REQUEST = {
  eventId: "e".repeat(64),
  channelId: CHANNEL_ID,
  commandId: "csl-hire-1",
  requesterPubkey: LEAD,
  createdAt: 1_700_000_000,
  action: {
    type: "session.hire",
    sessionRef: SESSION_REF,
    genesisRef: GENESIS_REF,
    role: "builder",
    providerInstanceRef: null,
    model: null,
    brief: "Take the badge lane. Red test first.",
  },
};

const UMBRELLA = {
  sessionRef: SESSION_REF,
  genesisRef: GENESIS_REF,
  title: "Agent Teams",
  projectRef: PROJECT_REF,
  executions: [
    {
      activeGeneration: { agentRef: LEAD, role: "lead", status: "running" },
    },
  ],
};

function answer(overrides = {}) {
  return planCodingSessionHireAnswer({
    request: REQUEST,
    umbrella: UMBRELLA,
    authority: { founderPubkey: FOUNDER, grantedOperators: [LEAD] },
    policy: DEFAULT_CODING_SESSION_HIRE_POLICY,
    candidates: [
      {
        pubkey: ADA,
        name: "Ada",
        homeRole: "builder",
        projectRef: PROJECT_REF,
        hasRolePack: true,
      },
    ],
    availableProviderInstanceRefs: ["claude-primary"],
    providerAuthorityPubkey: PROVIDER,
    commandId: "csl-seat-1",
    // The fixture request is fixed in time, so the clock is too: a suite
    // whose hires age out as the wall clock moves is a suite that starts
    // failing on its own.
    now: REQUEST.createdAt,
    ...overrides,
  });
}

test("a granted lead's hire produces a seat carrying the brief as its first turn", () => {
  const result = answer();
  assert.equal(result.kind, "seat");
  assert.equal(result.plan.actor, ADA);
  assert.equal(result.plan.role, "builder");
  assert.equal(
    result.plan.initialTurn,
    "[From the lead] Take the badge lane. Red test first.",
  );
  assert.equal(result.plan.title, "Agent Teams");
  assert.equal(result.plan.projectRef, PROJECT_REF);
  assert.equal(result.plan.providerAuthorityPubkey, PROVIDER);
  assert.equal(result.plan.worktreeName, "agent-teams-builder-1");
});

test("a stranger's hire is ignored, never answered", () => {
  const result = answer({
    request: { ...REQUEST, requesterPubkey: STRANGER },
  });
  assert.deepEqual(result, { kind: "ignored", why: "unauthorized" });
});

test("the founder may hire without a grant", () => {
  const result = answer({ request: { ...REQUEST, requesterPubkey: FOUNDER } });
  assert.equal(result.kind, "seat");
});

test("an umbrella this host has not observed is ignored, not refused", () => {
  assert.deepEqual(answer({ umbrella: null }), {
    kind: "ignored",
    why: "unknown-umbrella",
  });
  // A hire naming a genesis this umbrella does not have is somebody else's
  // session, however matching its sessionRef looks.
  assert.deepEqual(
    answer({ umbrella: { ...UMBRELLA, genesisRef: "b".repeat(64) } }),
    { kind: "ignored", why: "unknown-umbrella" },
  );
});

test("a policy refusal is a code, a reason, and the exact turn text", () => {
  const result = answer({
    policy: { ...DEFAULT_CODING_SESSION_HIRE_POLICY, enabled: false },
  });
  assert.equal(result.kind, "refused");
  assert.equal(result.code, "HIRE_OFF");
  assert.equal(result.text, `hire refused: HIRE_OFF — ${result.reason}`);
});

test("the seat ordinal counts the builders already in this umbrella", () => {
  const result = answer({
    umbrella: {
      ...UMBRELLA,
      executions: [
        ...UMBRELLA.executions,
        {
          activeGeneration: {
            agentRef: "c".repeat(64),
            role: "builder",
            status: "running",
          },
        },
      ],
    },
  });
  assert.equal(result.plan.worktreeName, "agent-teams-builder-2");
});

test("the umbrella's line names who asked, for what, and the answer", () => {
  assert.equal(
    codingSessionHireRefusalNotice({
      role: "builder",
      requesterLabel: "Keystone",
      text: "hire refused: HIRE_LIMIT — no room.",
    }),
    "Keystone asked to hire a builder — hire refused: HIRE_LIMIT — no room.",
  );
});

test("a hire older than the host's window is refused HIRE_STALE, never seated", () => {
  const result = answer({
    now: REQUEST.createdAt + 16 * 60,
  });
  assert.equal(result.kind, "refused");
  assert.equal(result.code, "HIRE_STALE");
  assert.equal(
    result.reason,
    "this hire request is older than the host's window; hire again",
  );
  assert.equal(result.text, `hire refused: HIRE_STALE — ${result.reason}`);
});

test("a hire inside the window is still seated", () => {
  const result = answer({ now: REQUEST.createdAt + 14 * 60 });
  assert.equal(result.kind, "seat");
});

test("a stranger's stale hire is still ignored, not refused", () => {
  const result = answer({
    request: { ...REQUEST, requesterPubkey: STRANGER },
    now: REQUEST.createdAt + 60 * 60,
  });
  assert.deepEqual(result, { kind: "ignored", why: "unauthorized" });
});

test("a hire whose model the runtime cannot offer is refused, with the list", () => {
  const result = answer({
    request: {
      ...REQUEST,
      action: { ...REQUEST.action, model: "claude-sonnet-5" },
    },
    modelCatalogs: new Map([
      ["claude-primary", ["default", "claude-fable-5[1m]"]],
    ]),
  });
  assert.equal(result.kind, "refused");
  assert.equal(result.code, "HIRE_MODEL_NOT_OFFERED");
  assert.match(result.reason, /claude-fable-5\[1m\]/);
});

// Brian's ruling, 2026-08-29: no alias translation anywhere. A catalog that
// publishes `sonnet` does not publish `claude-sonnet-5`, so this reaches the
// same refusal as any other unoffered id — it used to reach the seat plan on
// `sonnet` with a disclosure attached.
test("a vendor alias the catalog does not publish never reaches a seat plan", () => {
  const result = answer({
    request: {
      ...REQUEST,
      action: { ...REQUEST.action, model: "claude-sonnet-5" },
    },
    modelCatalogs: new Map([["claude-primary", ["default", "sonnet"]]]),
  });
  assert.equal(result.kind, "refused");
  assert.equal(result.code, "HIRE_MODEL_NOT_OFFERED");
  assert.match(result.reason, /claude-sonnet-5/);
  assert.match(result.reason, /default, sonnet/);
});

test("a substituted model is disclosed in the umbrella, naming the seat's role", () => {
  const line = codingSessionHireModelNoticeLine({
    role: "builder",
    notice:
      "The hire asked for claude-sonnet-5; this computer's claude-primary " +
      "runtime does not offer that id, so the seat runs sonnet instead.",
  });
  assert.match(line, /^Hired a builder — /);
  assert.match(line, /claude-sonnet-5/);
  assert.match(line, /sonnet instead\.$/);
});

// --- item 88(c): the hired seat's create carried projectRef NONE ------------

test("the hire inherits the project from the umbrella's own executions", () => {
  // The real fold — `groupCodingSessionCatalog` → `CodingSessionUmbrellaRecord`
  // — carries no `projectRef` field at all, so `umbrella.projectRef` was
  // always undefined and every hired seat landed outside the project. The
  // project the lead's create signed lives on each execution's active
  // generation, which is what the projects sidebar reads back.
  const result = answer({
    umbrella: {
      sessionRef: SESSION_REF,
      genesisRef: GENESIS_REF,
      title: "Agent Teams",
      executions: [
        {
          activeGeneration: {
            agentRef: LEAD,
            role: "lead",
            status: "running",
            projectRef: PROJECT_REF,
          },
        },
      ],
    },
  });
  assert.equal(result.kind, "seat");
  assert.equal(result.plan.projectRef, PROJECT_REF);
});

test("an umbrella in no project seats a hire with no project, never a guess", () => {
  const result = answer({
    // A projectless umbrella seats only an agent that belongs to no project.
    candidates: [
      {
        pubkey: ADA,
        name: "Ada",
        homeRole: "builder",
        projectRef: null,
        hasRolePack: true,
      },
    ],
    umbrella: {
      sessionRef: SESSION_REF,
      genesisRef: GENESIS_REF,
      title: "Agent Teams",
      executions: [
        {
          activeGeneration: {
            agentRef: LEAD,
            role: "lead",
            status: "running",
            projectRef: null,
          },
        },
      ],
    },
  });
  assert.equal(result.kind, "seat");
  assert.equal(result.plan.projectRef, null);
});

/**
 * The router, in the answer this host actually publishes.
 *
 * Three properties, and each is one the seat's honesty rests on: an unrouted
 * hire is unchanged, a routed one carries the whole decision onto the create,
 * and a routed one this host cannot route is refused out loud rather than
 * seated on the identity's own model as though nobody had asked.
 */
const ROUTING_FIXTURE = readFileSync(
  new URL("../../../../../team/model-registry.yaml", import.meta.url),
  "utf8",
);
const READABLE_REGISTRY = {
  kind: "readable",
  text: ROUTING_FIXTURE,
  label: "team/model-registry.yaml",
};
const CLAUDE_CATALOG = new Map([
  [
    "claude-primary",
    ["claude-fable-5[1m]", "default", "haiku", "opus[1m]", "sonnet"],
  ],
]);

function routedRequest(routing) {
  return { ...REQUEST, action: { ...REQUEST.action, routing } };
}

test("an unrouted hire still carries no routing at all", () => {
  const result = answer();
  assert.equal(result.kind, "seat");
  assert.equal(result.plan.routing, null);
});

test("a routed hire puts the whole decision on the seat", () => {
  const result = answer({
    request: routedRequest({
      class: "builder",
      risk: { impact: 3, uncertainty: 3, irreversibility: 2 },
    }),
    registry: READABLE_REGISTRY,
    modelCatalogs: CLAUDE_CATALOG,
    catalogRevision: 12,
  });
  assert.equal(result.kind, "seat");
  assert.equal(result.plan.model, "sonnet");
  assert.equal(result.plan.routing.chosen.model, "sonnet");
  assert.equal(result.plan.routing.chosen.effort, "medium");
  assert.equal(result.plan.routing.catalogRevision, 12);
  assert.equal(result.plan.routing.reviewRequired, false);
});

test("a routed verifier automatically prefers an eligible identity from another vendor", () => {
  const result = answer({
    request: {
      ...routedRequest({
        class: "verifier",
        risk: { impact: 3, uncertainty: 3, irreversibility: 2 },
      }),
      action: {
        ...routedRequest({
          class: "verifier",
          risk: { impact: 3, uncertainty: 3, irreversibility: 2 },
        }).action,
        role: "verifier",
      },
    },
    umbrella: {
      ...UMBRELLA,
      executions: [
        ...UMBRELLA.executions,
        {
          activeGeneration: {
            agentRef: ADA,
            role: "builder",
            status: "running",
            provider: "claude-primary",
          },
        },
      ],
    },
    candidates: [
      {
        pubkey: "3".repeat(64),
        name: "A Claude verifier",
        homeRole: "verifier",
        projectRef: PROJECT_REF,
        hasRolePack: true,
        runtime: "claude",
      },
      {
        pubkey: "4".repeat(64),
        name: "Z Codex verifier",
        homeRole: "verifier",
        projectRef: PROJECT_REF,
        hasRolePack: true,
        runtime: "codex",
      },
    ],
    availableProviderInstanceRefs: ["claude-primary", "codex-primary"],
    providerRuntimeSlugs: new Map([
      ["claude-primary", "claude"],
      ["codex-primary", "codex"],
    ]),
    registry: READABLE_REGISTRY,
    modelCatalogs: new Map([
      ...CLAUDE_CATALOG,
      ["codex-primary", ["gpt-5.6-sol[medium]"]],
    ]),
    catalogRevision: 12,
  });
  assert.equal(result.kind, "seat");
  assert.equal(result.plan.actor, "4".repeat(64));
  assert.equal(result.plan.providerInstanceRef, "codex-primary");
  assert.equal(result.plan.routing.chosen.provider, "codex-primary");
  assert.match(result.plan.routing.reason, /failure-mode diversity/);
});

test("a routed verifier discloses when this host has no eligible cross-vendor identity", () => {
  const result = answer({
    request: {
      ...routedRequest({
        class: "verifier",
        risk: { impact: 3, uncertainty: 3, irreversibility: 2 },
      }),
      action: {
        ...routedRequest({
          class: "verifier",
          risk: { impact: 3, uncertainty: 3, irreversibility: 2 },
        }).action,
        role: "verifier",
      },
    },
    umbrella: {
      ...UMBRELLA,
      executions: [
        ...UMBRELLA.executions,
        {
          activeGeneration: {
            agentRef: ADA,
            role: "builder",
            status: "running",
            provider: "claude-primary",
          },
        },
      ],
    },
    candidates: [
      {
        pubkey: "3".repeat(64),
        name: "Claude verifier",
        homeRole: "verifier",
        projectRef: PROJECT_REF,
        hasRolePack: true,
        runtime: "claude",
      },
    ],
    availableProviderInstanceRefs: ["claude-primary", "codex-primary"],
    providerRuntimeSlugs: new Map([
      ["claude-primary", "claude"],
      ["codex-primary", "codex"],
    ]),
    registry: READABLE_REGISTRY,
    modelCatalogs: new Map([
      ...CLAUDE_CATALOG,
      ["codex-primary", ["gpt-5.6-sol[medium]"]],
    ]),
    catalogRevision: 12,
  });
  assert.equal(result.kind, "seat");
  assert.equal(result.plan.providerInstanceRef, "claude-primary");
  assert.match(result.plan.routing.reason, /no eligible cross-provider target/);
});

test("a routed hire this host cannot route is refused HIRE_NO_ROUTE", () => {
  const result = answer({
    request: routedRequest({
      class: "lead",
      risk: { impact: 5, uncertainty: 5, irreversibility: 5 },
    }),
    registry: READABLE_REGISTRY,
    modelCatalogs: new Map([["claude-primary", ["haiku"]]]),
  });
  assert.equal(result.kind, "refused");
  assert.equal(result.code, "HIRE_NO_ROUTE");
  assert.match(result.text, /^hire refused: HIRE_NO_ROUTE — /);
});

test("a host with no registry refuses a routed hire and says which file", () => {
  const result = answer({
    request: routedRequest({
      class: "builder",
      risk: { impact: 3, uncertainty: 3, irreversibility: 2 },
    }),
    modelCatalogs: CLAUDE_CATALOG,
  });
  assert.equal(result.kind, "refused");
  assert.equal(result.code, "HIRE_NO_ROUTE");
  assert.match(result.reason, /registry not readable on this host/);
});

test("a routed hire is not refused for the identity's own stale model", () => {
  // The identity's record names a model this runtime does not publish. On an
  // unrouted hire that is `HIRE_MODEL_NOT_OFFERED` and must stay so (item
  // 88(a)); on a routed hire nothing is inherited, so the router answers.
  const stale = [
    {
      pubkey: ADA,
      name: "Ada",
      homeRole: "builder",
      projectRef: PROJECT_REF,
      hasRolePack: true,
      model: "claude-sonnet-5",
    },
  ];
  const unrouted = answer({ candidates: stale, modelCatalogs: CLAUDE_CATALOG });
  assert.equal(unrouted.kind, "refused");
  assert.equal(unrouted.code, "HIRE_MODEL_NOT_OFFERED");

  const routed = answer({
    candidates: stale,
    request: routedRequest({
      class: "builder",
      risk: { impact: 3, uncertainty: 3, irreversibility: 2 },
    }),
    registry: READABLE_REGISTRY,
    modelCatalogs: CLAUDE_CATALOG,
  });
  assert.equal(routed.kind, "seat");
  assert.equal(routed.plan.model, "sonnet");
});

// ── team.yml's per-role hint on an unrouted hire (ledger 179(b), 180) ───────
//
// An unrouted hire used to have exactly one input for the model: the
// identity's own pin. That is how seven seats on Andy's run took
// `claude-fable-5-1[1m]` under an `opus[1m]` lead — nobody chose it, it was
// simply what their records said. The project's `team.yml` gets a say now,
// and every one of these cases is about the seat still being honest about
// which source chose.

/** The identity, pinned to `opus[1m]` the way a minted agent is. */
const PINNED = [
  {
    pubkey: ADA,
    name: "Ada",
    homeRole: "builder",
    projectRef: PROJECT_REF,
    hasRolePack: true,
    model: "opus[1m]",
  },
];

const CLAUDE_SLUGS = new Map([["claude-primary", "claude"]]);

test("an unrouted hire runs team.yml's model for the role, and says it did", () => {
  const result = answer({
    candidates: PINNED,
    modelCatalogs: CLAUDE_CATALOG,
    providerRuntimeSlugs: CLAUDE_SLUGS,
    teamRoleHints: new Map([
      ["builder", { runtime: "claude", model: "claude:sonnet" }],
    ]),
  });
  assert.equal(result.kind, "seat");
  assert.equal(result.plan.model, "sonnet");
  // The provenance is the point: a seat that silently ran something other
  // than its identity's pin would be the same class of lie as a "default"
  // label hiding the real model.
  assert.match(result.plan.modelNotice, /team\.yml names it for the builder/);
  assert.match(result.plan.modelNotice, /opus\[1m\]/);
  assert.equal(
    codingSessionHireModelNoticeLine({
      role: result.plan.role,
      notice: result.plan.modelNotice,
    }).startsWith("Hired a builder — ran sonnet"),
    true,
  );
});

test("a hint the runtime does not offer falls back to the pin and says why", () => {
  const result = answer({
    candidates: PINNED,
    modelCatalogs: CLAUDE_CATALOG,
    providerRuntimeSlugs: CLAUDE_SLUGS,
    teamRoleHints: new Map([
      ["builder", { runtime: null, model: "claude:gpt-5.6-sol" }],
    ]),
  });
  // Refuses nothing: a hint is advice, and advice this catalog cannot serve
  // is disclosed rather than fatal.
  assert.equal(result.kind, "seat");
  assert.equal(result.plan.model, "opus[1m]");
  assert.match(result.plan.modelNotice, /does not offer/);
  assert.match(result.plan.modelNotice, /gpt-5\.6-sol/);
});

test("a hint for another vendor does not move the seat off its identity's runtime", () => {
  const result = answer({
    candidates: PINNED,
    // The id exists in this catalog, so only the vendor half can refuse it.
    modelCatalogs: new Map([["claude-primary", ["opus[1m]", "sonnet"]]]),
    providerRuntimeSlugs: CLAUDE_SLUGS,
    teamRoleHints: new Map([
      ["builder", { runtime: "codex", model: "codex:sonnet" }],
    ]),
  });
  assert.equal(result.kind, "seat");
  assert.equal(result.plan.model, "opus[1m]");
  assert.match(result.plan.modelNotice, /names codex/);
  // Item 88(i): the identity decides the runtime, and the notice says so
  // rather than the host quietly switching adapters.
  assert.match(result.plan.modelNotice, /the identity decides the runtime/);
});

test("no hint for the role leaves the identity's pin and says nothing", () => {
  const result = answer({
    candidates: PINNED,
    modelCatalogs: CLAUDE_CATALOG,
    providerRuntimeSlugs: CLAUDE_SLUGS,
    teamRoleHints: new Map([
      ["verifier", { runtime: null, model: "claude:haiku" }],
    ]),
  });
  assert.equal(result.kind, "seat");
  assert.equal(result.plan.model, "opus[1m]");
  assert.equal(result.plan.modelNotice, null);
});

test("no team manifest at all is the behaviour that shipped before", () => {
  const result = answer({ candidates: PINNED, modelCatalogs: CLAUDE_CATALOG });
  assert.equal(result.kind, "seat");
  assert.equal(result.plan.model, "opus[1m]");
  assert.equal(result.plan.modelNotice, null);
});

test("a model the lead named wins over team.yml, with nothing to disclose", () => {
  const result = answer({
    candidates: PINNED,
    request: {
      ...REQUEST,
      action: { ...REQUEST.action, model: "haiku" },
    },
    modelCatalogs: CLAUDE_CATALOG,
    providerRuntimeSlugs: CLAUDE_SLUGS,
    teamRoleHints: new Map([
      ["builder", { runtime: null, model: "claude:sonnet" }],
    ]),
  });
  assert.equal(result.kind, "seat");
  assert.equal(result.plan.model, "haiku");
  assert.equal(result.plan.modelNotice, null);
});

test("a routed hire ignores team.yml: the router already chose", () => {
  const result = answer({
    candidates: PINNED,
    request: routedRequest({
      class: "builder",
      risk: { impact: 3, uncertainty: 3, irreversibility: 2 },
    }),
    registry: READABLE_REGISTRY,
    modelCatalogs: CLAUDE_CATALOG,
    providerRuntimeSlugs: CLAUDE_SLUGS,
    teamRoleHints: new Map([
      ["builder", { runtime: null, model: "claude:haiku" }],
    ]),
  });
  assert.equal(result.kind, "seat");
  assert.equal(result.plan.model, "sonnet");
  assert.equal(result.plan.modelNotice, null);
});
