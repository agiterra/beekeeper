import assert from "node:assert/strict";
import { test } from "node:test";

import { buildProjectAgents } from "@/features/project-agents/lib/projectAgentsModel";

const OWNER = "6".repeat(64);
const PROJECT = `30621:${OWNER}:tank-loop`;
const OTHER_PROJECT = `30621:${OWNER}:other`;
const BRIAN = "b".repeat(64);
const LOOM = "1".repeat(64);
const BOB = "2".repeat(64);
const IRA = "3".repeat(64);
const SAGE = "4".repeat(64);
const HOMEBODY = "5".repeat(64);
const PROVIDER = "9".repeat(64);
const SESSION_REF = "c".repeat(64);
const CLOSED_REF = "d".repeat(64);
const INSTALLED_SHA = "f0132d13".padEnd(40, "0");
const OLDER_SHA = "aaaa1111".padEnd(40, "1");
const NOW = 1_800_000_000;

function packRef(role, sha = INSTALLED_SHA) {
  return {
    repo: `30617:${OWNER}:tank-loop-packs`,
    sha,
    role,
    path: `personas/roles/${role}`,
  };
}

function execution(overrides = {}) {
  const { channelId = "chan-1", ...session } = overrides;
  return {
    channelId,
    session: {
      generationId: "gen-lead",
      label: "Loom execution",
      agentRef: LOOM,
      role: "lead",
      projectRef: PROJECT,
      sessionRef: SESSION_REF,
      provider: "claude-primary",
      runtime: "claude",
      model: "opus",
      status: "idle",
      statusAt: (NOW - 3 * 3_600) * 1_000,
      packRef: packRef("lead"),
      providerAuthorityPubkey: PROVIDER,
      commandTarget: {
        driver: "acp",
        instanceId: "inst",
        sessionId: "lead",
        generation: 1,
      },
      ...session,
    },
  };
}

function umbrella(overrides = {}) {
  return {
    channelId: "chan-1",
    generationId: "gen-lead",
    label: "Project team setup",
    sessionRef: SESSION_REF,
    isClosed: false,
    ...overrides,
  };
}

function assignment(overrides = {}) {
  return {
    sourceEventId: "e".repeat(64),
    createdAt: NOW - 7_200,
    assignerPubkey: LOOM,
    assigneeActor: BOB,
    assigneeRole: "builder",
    objective: "Set up local development",
    brief: "Serialized setup on main.",
    branch: null,
    baseSha: null,
    fileOwnership: [],
    acceptanceSteps: ["Run the gate"],
    supersedes: null,
    reports: [{ eventId: "r1" }],
    dispositions: [
      { decision: "changes-requested", createdAt: NOW - 6_000 },
      { decision: "approve-with-notes", createdAt: NOW - 5_000 },
    ],
    settlement: {},
    status: "settled",
    ...overrides,
  };
}

const AGENTS = [
  { pubkey: LOOM, name: "Loom", managedHere: true },
  { pubkey: BOB, name: "Bob", managedHere: true },
  { pubkey: IRA, name: "Ira", managedHere: true },
  { pubkey: SAGE, name: "Sage", managedHere: true },
  { pubkey: HOMEBODY, name: "Homebody", managedHere: true },
];

function build(overrides = {}) {
  return buildProjectAgents({
    projectRef: PROJECT,
    executions: [],
    umbrellas: [umbrella()],
    declaredSessions: [],
    installations: [],
    agents: AGENTS,
    otherNames: new Map([[BRIAN, "Brian"]]),
    nowSeconds: NOW,
    ...overrides,
  });
}

function names(rows) {
  return rows.map((row) => row.name);
}

test("a worker seated inside the lead's umbrella is listed, with the lead's assignment", () => {
  const model = build({
    executions: [
      execution(),
      execution({
        generationId: "gen-bob",
        agentRef: BOB,
        role: "builder",
        model: "sonnet",
        packRef: packRef("builder"),
        commandTarget: {
          driver: "acp",
          instanceId: "inst",
          sessionId: "bob",
          generation: 1,
        },
      }),
    ],
    declaredSessions: [
      {
        sessionRef: SESSION_REF,
        channelId: "chan-1",
        name: "Project team setup",
        lifecycle: "open",
        assignments: [assignment()],
      },
    ],
  });

  assert.deepEqual(names(model.working), ["Bob", "Loom"]);
  const bob = model.working[0];
  assert.deepEqual(bob.relationship, {
    kind: "seated",
    role: "builder",
    sessionName: "Project team setup",
    assignerName: "Loom",
  });
  assert.equal(bob.sessions.length, 1);
  assert.equal(bob.sessions[0].model, "sonnet");
  // Opening the seat opens the umbrella it sits in, not a folded-away row.
  assert.deepEqual(bob.sessions[0].openTarget, {
    channelId: "chan-1",
    generationId: "gen-lead",
    founded: false,
  });
  assert.equal(bob.assignments[0].latestDecision, "approve-with-notes");
  assert.equal(bob.assignments[0].reportCount, 1);
  // Loom has no assignment addressed to it: nobody is named as its assigner.
  assert.equal(model.working[1].relationship.assignerName, null);
});

test("an idle agent in an open session is still working here", () => {
  const model = build({
    executions: [execution({ status: "stopped" })],
  });
  assert.deepEqual(names(model.working), ["Loom"]);
  assert.equal(model.working[0].lastSeenSeconds, 3 * 3_600);
});

test("an installed agent with no session or assignment waits for its first assignment", () => {
  const model = build({
    installations: [
      { role: "verifier", agentPubkey: SAGE, packRef: packRef("verifier") },
    ],
  });
  assert.deepEqual(names(model.installed), ["Sage"]);
  assert.deepEqual(model.installed[0].relationship, {
    kind: "installed",
    role: "verifier",
  });
  assert.equal(model.working.length, 0);
});

test("a matching home role alone never lists an agent", () => {
  // Homebody is managed here with a builder home role, but no installation,
  // execution or assignment names it in this project.
  const model = build({
    agents: [
      ...AGENTS,
      { pubkey: HOMEBODY, name: "Homebody", managedHere: true },
    ],
  });
  assert.equal(
    model.working.length + model.installed.length + model.previous.length,
    0,
  );
});

test("executions placed in another project do not list an agent here", () => {
  const model = build({
    executions: [execution({ projectRef: OTHER_PROJECT })],
  });
  assert.equal(model.working.length, 0);
});

test("an agent whose every session is closed is previously here", () => {
  const model = build({
    umbrellas: [
      umbrella(),
      umbrella({
        sessionRef: CLOSED_REF,
        generationId: "gen-old",
        label: "Old audit",
        isClosed: true,
      }),
    ],
    executions: [
      execution({
        agentRef: IRA,
        role: "verifier",
        sessionRef: CLOSED_REF,
        generationId: "gen-old",
        commandTarget: {
          driver: "acp",
          instanceId: "inst",
          sessionId: "ira",
          generation: 1,
        },
      }),
    ],
  });
  assert.deepEqual(names(model.previous), ["Ira"]);
  assert.equal(model.previous[0].sessions[0].sessionClosed, true);
});

test("a person's assignment names the person, and an assignment alone lists the assignee", () => {
  const model = build({
    declaredSessions: [
      {
        sessionRef: SESSION_REF,
        channelId: "chan-1",
        name: "Project team setup",
        lifecycle: "open",
        assignments: [
          assignment({ assignerPubkey: BRIAN, assigneeActor: IRA }),
        ],
      },
    ],
  });
  assert.deepEqual(names(model.working), ["Ira"]);
  assert.deepEqual(model.working[0].relationship, {
    kind: "assigned",
    role: "builder",
    sessionName: "Project team setup",
    assignerName: "Brian",
  });
});

test("a staged revision that differs from the installed one is flagged; an unknown is not", () => {
  const model = build({
    installations: [
      { role: "builder", agentPubkey: BOB, packRef: packRef("builder") },
      { role: "lead", agentPubkey: LOOM, packRef: packRef("lead") },
    ],
    executions: [
      execution({ packRef: null }),
      execution({
        generationId: "gen-bob",
        agentRef: BOB,
        role: "builder",
        packRef: packRef("builder", OLDER_SHA),
        commandTarget: {
          driver: "acp",
          instanceId: "inst",
          sessionId: "bob",
          generation: 1,
        },
      }),
    ],
  });
  const byName = new Map(model.working.map((row) => [row.name, row]));
  assert.equal(byName.get("Bob").sessions[0].packDiffersFromInstalled, true);
  assert.equal(byName.get("Loom").sessions[0].packDiffersFromInstalled, false);
});

test("generations of one execution collapse to the newest", () => {
  const model = build({
    executions: [
      execution({ generationId: "gen-1", status: "failed" }),
      execution({
        generationId: "gen-2",
        status: "running",
        commandTarget: {
          driver: "acp",
          instanceId: "inst",
          sessionId: "lead",
          generation: 2,
        },
      }),
    ],
  });
  assert.equal(model.working[0].sessions.length, 1);
  assert.equal(model.working[0].sessions[0].status, "running");
});

test("an unnamed identity reads as the compact pubkey form, never blank", () => {
  const stranger = "7".repeat(64);
  const model = build({
    executions: [execution({ agentRef: stranger })],
  });
  assert.equal(model.working[0].managedHere, false);
  assert.match(model.working[0].name, /7/);
  assert.notEqual(model.working[0].name, "");
});
