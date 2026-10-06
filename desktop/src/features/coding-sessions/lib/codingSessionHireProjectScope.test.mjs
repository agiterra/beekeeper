/**
 * A hire is answered only from the umbrella's own project's agents.
 *
 * Live 2026-09-14 (docs/PROJECT_AGENT_HIRING_IMPL.md): Tank Loop's lead hired
 * Bob, Gordan and Ira — the Beekeeper crew — because every managed agent on
 * the founder's computer with a matching home role was eligible and the name
 * broke the tie ("Bob" < "Builder"). These pin the acceptance rules: each
 * project hires its own (A), renames change nothing (B), another project's or
 * an unassociated agent never enters (C), a missing project agent is refused
 * with a remedy rather than borrowed (D), and the refusal codes match the
 * relay's list.
 */
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import test from "node:test";

import { planCodingSessionHireAnswer } from "./codingSessionHireAnswer.ts";
import {
  codingSessionHireAgentsFromManaged,
  codingSessionHireCandidatesOf,
} from "./codingSessionHireCandidates.ts";
import {
  CODING_SESSION_HIRE_REFUSAL_CODES,
  DEFAULT_CODING_SESSION_HIRE_POLICY,
  codingSessionHireAllowedRoles,
  decideCodingSessionHire,
} from "./codingSessionHirePolicy.ts";

const OWNER_1 = "1a".repeat(32);
const OWNER_2 = "2b".repeat(32);
const P1 = `30621:${OWNER_1}:tank-loop`;
const P2 = `30621:${OWNER_2}:beekeeper`;

/** One agent per project per role, same names in both projects. */
function key(project, role) {
  const digit = { builder: "1", runner: "2", verifier: "3" }[role];
  return `${project}${digit}`.padEnd(64, project === "a" ? "a" : "b");
}
const ROLES = ["builder", "runner", "verifier"];
const P1_AGENTS = ROLES.map((role) => ({
  pubkey: key("a", role),
  name: role[0].toUpperCase() + role.slice(1),
  homeRole: role,
  projectRef: P1,
  hasRolePack: true,
}));
const P2_AGENTS = ROLES.map((role) => ({
  pubkey: key("b", role),
  name: role[0].toUpperCase() + role.slice(1),
  homeRole: role,
  projectRef: P2,
  hasRolePack: true,
}));
/** Bob: a builder that belongs to no project, and sorts first by name. */
const BOB = {
  pubkey: "0".repeat(64),
  name: "Bob",
  homeRole: "builder",
  projectRef: null,
  hasRolePack: true,
};

function decide(overrides = {}) {
  return decideCodingSessionHire({
    request: { role: "builder", providerInstanceRef: null, model: null },
    policy: DEFAULT_CODING_SESSION_HIRE_POLICY,
    projectRef: P1,
    candidates: [BOB, ...P2_AGENTS, ...P1_AGENTS],
    liveSeats: [],
    availableProviderInstanceRefs: ["claude-primary"],
    ...overrides,
  });
}

function hire(role, projectRef, overrides = {}) {
  return decide({
    request: { role, providerInstanceRef: null, model: null },
    projectRef,
    ...overrides,
  });
}

test("A: each project's hires seat that project's own builder, runner and verifier", () => {
  for (const role of ROLES) {
    const inP1 = hire(role, P1);
    assert.equal(inP1.ok, true, `${role} in P1`);
    assert.equal(inP1.identity.pubkey, key("a", role));
    assert.equal(inP1.identity.projectRef, P1);
    const inP2 = hire(role, P2);
    assert.equal(inP2.ok, true, `${role} in P2`);
    assert.equal(inP2.identity.pubkey, key("b", role));
    assert.equal(inP2.identity.projectRef, P2);
  }
});

test("A: a differently-cased or padded umbrella coordinate still names the same project", () => {
  const decision = hire(
    "builder",
    `  30621:${OWNER_1.toUpperCase()}:tank-loop `,
  );
  assert.equal(decision.ok, true);
  assert.equal(decision.identity.pubkey, key("a", "builder"));
});

test("B: renaming an agent does not change who is hired", () => {
  const before = hire("builder", P1);
  // Renamed to sort before every other candidate, including P2's and Bob.
  const renamed = P1_AGENTS.map((agent) =>
    agent.homeRole === "builder" ? { ...agent, name: "Aaron" } : agent,
  );
  const after = hire("builder", P1, {
    candidates: [BOB, ...P2_AGENTS, ...renamed],
  });
  assert.equal(after.ok, true);
  assert.equal(after.identity.pubkey, before.identity.pubkey);
  // And renaming the other project's builder to sort first changes nothing.
  const p2Renamed = P2_AGENTS.map((agent) =>
    agent.homeRole === "builder" ? { ...agent, name: "Aardvark" } : agent,
  );
  const still = hire("builder", P1, {
    candidates: [{ ...BOB, name: "AAA" }, ...p2Renamed, ...P1_AGENTS],
  });
  assert.equal(still.identity.pubkey, before.identity.pubkey);
});

test("B: two agents of one project are chosen by key, never by display name", () => {
  const second = {
    ...P1_AGENTS[0],
    pubkey: "a0".padEnd(64, "0"),
    name: "Zed",
  };
  const decision = hire("builder", P1, {
    candidates: [...P1_AGENTS, second],
  });
  assert.equal(decision.identity.pubkey, second.pubkey);
  const renamed = hire("builder", P1, {
    candidates: [
      ...P1_AGENTS.map((agent) => ({ ...agent, name: "Aaa" })),
      { ...second, name: "Zzz" },
    ],
  });
  assert.equal(renamed.identity.pubkey, second.pubkey);
});

test("C: P1's builder busy is HIRE_ROLE_BUSY — never P2's builder, never Bob", () => {
  const decision = hire("builder", P1, {
    liveSeats: [{ actor: key("a", "builder"), role: "builder" }],
  });
  assert.equal(decision.ok, false);
  assert.equal(decision.code, "HIRE_ROLE_BUSY");
  assert.match(decision.reason, /already seated/);
  assert.match(decision.reason, /bee sessions send --to builder/);
});

test("C: a borrowed agent sitting in the umbrella never makes the role busy", () => {
  // Bob was seated before this fix. P1's own builder is not live, so it is
  // hired; Bob's seat is not counted as P1's.
  const decision = hire("builder", P1, {
    liveSeats: [{ actor: BOB.pubkey, role: "builder" }],
  });
  assert.equal(decision.ok, true);
  assert.equal(decision.identity.pubkey, key("a", "builder"));
  // And with no P1 builder at all, Bob in the umbrella is not "busy".
  const none = hire("builder", P1, {
    candidates: [BOB, ...P2_AGENTS],
    liveSeats: [{ actor: BOB.pubkey, role: "builder" }],
  });
  assert.equal(none.code, "HIRE_NO_PROJECT_AGENT");
});

test("D: a project with no agent for the role is refused HIRE_NO_PROJECT_AGENT, with a count and a remedy", () => {
  const decision = hire("builder", P1, {
    candidates: [BOB, ...P2_AGENTS, ...P1_AGENTS.slice(1)],
  });
  assert.equal(decision.ok, false);
  assert.equal(decision.code, "HIRE_NO_PROJECT_AGENT");
  assert.match(
    decision.reason,
    new RegExp(`^project ${P1} has no builder agent on this computer\\.`),
  );
  assert.match(
    decision.reason,
    /2 other agents here have that role but do not belong to this project, and borrowing is not supported\./,
  );
  assert.match(
    decision.reason,
    /Install the project's roles or associate a builder agent on the project's Agents tab, then ask again\.$/,
  );
  // Other projects' agents are counted, never named.
  assert.equal(/Bob|Builder/.test(decision.reason), false);
});

test("D: the count sentence is singular for one, and omitted for none", () => {
  const one = hire("builder", P1, { candidates: [BOB] });
  assert.match(
    one.reason,
    /1 other agent here has that role but does not belong to this project/,
  );
  const zero = hire("builder", P1, {
    candidates: P1_AGENTS.slice(1),
    policy: {
      ...DEFAULT_CODING_SESSION_HIRE_POLICY,
      allowedRoles: ["builder"],
    },
  });
  assert.equal(zero.code, "HIRE_NO_PROJECT_AGENT");
  assert.equal(/other agent/.test(zero.reason), false);
  assert.match(zero.reason, /not supported/);
});

test("D: a project label, when supplied, names the project instead of its coordinate", () => {
  const decision = hire("builder", P1, {
    candidates: [BOB],
    projectLabel: "Tank Loop",
  });
  assert.match(decision.reason, /^Tank Loop has no builder agent/);
});

test("the default role list stays computer-wide, so the refusal is the project's, not the policy's", () => {
  // Only P2 holds a designer; P1's hire for it is a missing project agent.
  const designer = {
    pubkey: "d".repeat(64),
    name: "Designer",
    homeRole: "designer",
    projectRef: P2,
    hasRolePack: true,
  };
  const decision = hire("designer", P1, {
    candidates: [...P1_AGENTS, designer],
  });
  assert.equal(decision.code, "HIRE_NO_PROJECT_AGENT");
});

test("a projectless umbrella seats only an agent that belongs to no project", () => {
  const decision = hire("builder", null);
  assert.equal(decision.ok, true);
  assert.equal(decision.identity.pubkey, BOB.pubkey);
  // No unassociated runner: every runner belongs to a project.
  const runner = hire("runner", null);
  assert.equal(runner.code, "HIRE_NO_PROJECT_AGENT");
  assert.equal(
    runner.reason,
    "this session has no project, and every runner agent on this computer " +
      "belongs to a project. Start the work in that project's session.",
  );
  // A role nobody on this computer holds keeps HIRE_NO_IDENTITY.
  const absent = hire("architect", null, {
    policy: {
      ...DEFAULT_CODING_SESSION_HIRE_POLICY,
      allowedRoles: ["architect"],
    },
  });
  assert.equal(absent.code, "HIRE_NO_IDENTITY");
});

test("a projectless umbrella's busy unassociated builder is HIRE_ROLE_BUSY, never a project's builder", () => {
  const decision = hire("builder", null, {
    liveSeats: [{ actor: BOB.pubkey, role: "builder" }],
  });
  assert.equal(decision.code, "HIRE_ROLE_BUSY");
});

test("a verifier hire never seats a builder-role project agent", () => {
  const decision = hire("verifier", P1, {
    candidates: [...P1_AGENTS.filter((agent) => agent.homeRole !== "verifier")],
    policy: {
      ...DEFAULT_CODING_SESSION_HIRE_POLICY,
      allowedRoles: ["verifier"],
    },
  });
  assert.equal(decision.ok, false);
  assert.equal(decision.code, "HIRE_NO_PROJECT_AGENT");
  const seated = hire("verifier", P1);
  assert.equal(seated.identity.homeRole, "verifier");
  assert.notEqual(seated.identity.pubkey, key("a", "builder"));
});

test("a project team setup actor is never hired, and never makes a role hireable", () => {
  const setup = {
    pubkey: "5".repeat(64),
    name: "Project Setup",
    homeRole: "builder",
    projectRef: P1,
    personaId: `project-team-setup:${P1}`,
    hasRolePack: true,
  };
  const decision = hire("builder", P1, {
    candidates: [setup],
    policy: {
      ...DEFAULT_CODING_SESSION_HIRE_POLICY,
      allowedRoles: ["builder"],
    },
  });
  assert.equal(decision.code, "HIRE_NO_PROJECT_AGENT");
  assert.deepEqual(
    codingSessionHireAllowedRoles(DEFAULT_CODING_SESSION_HIRE_POLICY, [setup]),
    [],
  );
});

test("unreadable coordinates fail closed: neither the umbrella's nor an agent's collapses to projectless", () => {
  const malformedUmbrella = hire("builder", "30621:owner:tank-loop");
  assert.equal(malformedUmbrella.ok, false);
  assert.equal(malformedUmbrella.code, "HIRE_NO_PROJECT_AGENT");
  assert.match(
    malformedUmbrella.reason,
    /not a well-formed project coordinate/,
  );
  const malformedAgent = hire("builder", null, {
    candidates: [{ ...BOB, projectRef: "not-a-project" }],
  });
  assert.equal(malformedAgent.ok, false);
});

// --- The answer: one project value for the decision and the create --------

const SESSION_REF = "5b7e1c2a-90d4-4b0e-a1f3-7c2d8e6f4a10";
const GENESIS_REF = "a".repeat(64);
const LEAD = "c".repeat(64);
const FOUNDER = "f".repeat(64);

function answer(projectRef, candidates) {
  return planCodingSessionHireAnswer({
    request: {
      eventId: "e".repeat(64),
      channelId: "3d2a7b18-9b7a-4a41-9a86-6a52a1c0b7e1",
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
        brief: "Take the lane.",
      },
    },
    umbrella: {
      sessionRef: SESSION_REF,
      genesisRef: GENESIS_REF,
      title: "Tank Loop",
      executions: [
        {
          activeGeneration: {
            agentRef: LEAD,
            role: "lead",
            status: "running",
            projectRef,
          },
        },
      ],
    },
    authority: { founderPubkey: FOUNDER, grantedOperators: [LEAD] },
    policy: DEFAULT_CODING_SESSION_HIRE_POLICY,
    candidates,
    availableProviderInstanceRefs: ["claude-primary"],
    providerAuthorityPubkey: "9".repeat(64),
    commandId: "csl-seat-1",
    now: 1_700_000_000,
  });
}

test("the answer scopes the decision by the umbrella's executions' project", () => {
  const inP1 = answer(P1, [BOB, ...P2_AGENTS, ...P1_AGENTS]);
  assert.equal(inP1.kind, "seat");
  assert.equal(inP1.plan.actor, key("a", "builder"));
  assert.equal(inP1.plan.projectRef, P1);
  const inP2 = answer(P2, [BOB, ...P1_AGENTS, ...P2_AGENTS]);
  assert.equal(inP2.plan.actor, key("b", "builder"));
  const refused = answer(P1, [BOB, ...P2_AGENTS]);
  assert.equal(refused.kind, "refused");
  assert.equal(refused.code, "HIRE_NO_PROJECT_AGENT");
  assert.match(refused.text, /^hire refused: HIRE_NO_PROJECT_AGENT — /);
});

// --- The candidate list carries the association ----------------------------

test("the host's candidate list keeps each agent's project and persona", () => {
  const agents = codingSessionHireAgentsFromManaged([
    {
      pubkey: key("a", "builder"),
      name: "Builder",
      personaId: "persona-builder",
      runtime: null,
      homeRole: "builder",
      projectRef: P1,
      hasRolePack: true,
      model: null,
      provider: null,
    },
    {
      pubkey: BOB.pubkey,
      name: "Bob",
      personaId: null,
      runtime: "codex",
      homeRole: "builder",
      // An older backend: no association field at all.
      model: "gpt-5.6-sol",
      provider: null,
    },
  ]);
  assert.equal(agents[0].projectRef, P1);
  assert.equal(agents[0].personaId, "persona-builder");
  assert.equal(agents[1].projectRef, null);
  const candidates = codingSessionHireCandidatesOf(agents);
  assert.equal(candidates[0].projectRef, P1);
  assert.equal(candidates[1].projectRef, null);
  assert.equal(candidates[1].runtime, "codex");
});

// Ledger 165: the host's effective runtime reaches the decision. Item 139
// taught the resolver to read it, but the candidate builder never handed it
// over, so a crew agent whose record pinned nothing was read from its
// effective harness — `buzz-agent`, the app default — and refused.
test("the host's candidate list carries the host's effective runtime to the decision", () => {
  const runtimeIdForCommand = (command) =>
    command === "buzz-agent" ? "buzz-agent" : null;
  const agents = codingSessionHireAgentsFromManaged(
    [
      {
        pubkey: key("a", "verifier"),
        name: "Verifier",
        personaId: "crew-role:verifier",
        runtime: "claude",
        effectiveRuntime: "claude",
        runtimeSource: "instance",
        agentCommand: "claude-agent-acp",
        homeRole: "verifier",
        projectRef: P1,
        hasRolePack: true,
        model: null,
        provider: null,
      },
      {
        pubkey: key("b", "verifier"),
        name: "Unpinned",
        personaId: "crew-role:verifier",
        runtime: null,
        // The host stopped at the global tier: no tier pins a runtime, and
        // the effective harness is the app default.
        effectiveRuntime: null,
        runtimeSource: "global",
        agentCommand: "buzz-agent",
        homeRole: "verifier",
        projectRef: P1,
        hasRolePack: true,
        model: null,
        provider: null,
      },
    ],
    { runtimeIdForCommand },
  );
  assert.deepEqual(
    [agents[0].runtime, agents[0].runtimeSource, agents[0].runtimeRead],
    ["claude", "record", "claude"],
  );
  // What the Agents screen shows for it, so the hire and the screen agree.
  assert.deepEqual(
    [agents[1].runtime, agents[1].runtimeSource, agents[1].runtimeRead],
    ["buzz-agent", "harness", "buzz-agent"],
  );
});

// --- Refusal-code parity with the relay ------------------------------------

test("the desktop's refusal codes are exactly buzz-core's HIRE_REFUSAL_CODES", () => {
  const source = readFileSync(
    new URL(
      "../../../../../crates/beekeeper-core/src/coding_session_lifecycle_command.rs",
      import.meta.url,
    ),
    "utf8",
  );
  const start = source.indexOf("pub const HIRE_REFUSAL_CODES: &[&str] = &[");
  assert.ok(start >= 0, "HIRE_REFUSAL_CODES not found in buzz-core");
  const body = source.slice(start, source.indexOf("];", start));
  const relay = [
    ...body
      .split("\n")
      .filter((line) => !line.trim().startsWith("//"))
      .join("\n")
      .matchAll(/"(HIRE_[A-Z_]+)"/g),
  ].map((match) => match[1]);
  assert.ok(relay.includes("HIRE_NO_PROJECT_AGENT"));
  assert.deepEqual(
    [...CODING_SESSION_HIRE_REFUSAL_CODES].sort(),
    [...new Set(relay)].sort(),
  );
});
