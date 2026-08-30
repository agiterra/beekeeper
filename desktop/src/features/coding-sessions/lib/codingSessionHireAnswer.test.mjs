import assert from "node:assert/strict";
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
  projectRef: "30621:owner:beekeeper",
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
      { pubkey: ADA, name: "Ada", homeRole: "builder", hasRolePack: true },
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
  assert.equal(result.plan.projectRef, "30621:owner:beekeeper");
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
            projectRef: "30621:owner:beekeeper",
          },
        },
      ],
    },
  });
  assert.equal(result.kind, "seat");
  assert.equal(result.plan.projectRef, "30621:owner:beekeeper");
});

test("an umbrella in no project seats a hire with no project, never a guess", () => {
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
            projectRef: null,
          },
        },
      ],
    },
  });
  assert.equal(result.kind, "seat");
  assert.equal(result.plan.projectRef, null);
});
