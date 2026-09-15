import assert from "node:assert/strict";
import { test } from "node:test";

import { buildProjectAgents } from "@/features/project-agents/lib/projectAgentsModel";
import {
  acceptPublishedProjectAgents,
  projectAgentAssociateAccess,
  projectAgentAuthorizedAuthors,
} from "@/features/project-agents/lib/publishedProjectAgents";
import { projectAgentDigest } from "@/shared/lib/projectAgentAssociation";

const OWNER = "6".repeat(64);
const PROJECT = `30621:${OWNER}:tank-loop`;
const OTHER_PROJECT = `30621:${OWNER}:attic`;
const ANDY = "a".repeat(64);
const COLLAB = "c".repeat(64);
const VIEWER = "e".repeat(64);
const BRIAN = "b".repeat(64);
const LOOM = "1".repeat(64);
const BOB = "2".repeat(64);
const IRA = "3".repeat(64);
const BUILDER = "4".repeat(64);
const ATTIC_BUILDER = "5".repeat(64);
const REMOTE = "7".repeat(64);
const PROVIDER = "9".repeat(64);
const SESSION_REF = "d".repeat(64);
const CLOSED_REF = "f".repeat(64);
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
  const { channelId = "chan-1", sessionId = "lead", ...session } = overrides;
  return {
    channelId,
    session: {
      generationId: `gen-${sessionId}`,
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
        sessionId,
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

const CLOSED_UMBRELLA = umbrella({
  sessionRef: CLOSED_REF,
  generationId: "gen-old",
  label: "Old audit",
  isClosed: true,
});

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

function local(pubkey, name, homeRole, projectRef = null) {
  return { pubkey, name, avatarUrl: null, homeRole, projectRef };
}

const LOCAL = [
  local(LOOM, "Loom", "lead", PROJECT),
  local(BUILDER, "Builder", "builder", PROJECT),
  local(ATTIC_BUILDER, "Attic Builder", "builder", OTHER_PROJECT),
  // The Beekeeper crew: same role names, no association.
  local(BOB, "Bob", "builder"),
  local(IRA, "Ira", "verifier"),
];

function build(overrides = {}) {
  return buildProjectAgents({
    projectRef: PROJECT,
    executions: [],
    umbrellas: [umbrella(), CLOSED_UMBRELLA],
    declaredSessions: [],
    installations: [],
    localAgents: LOCAL,
    publishedAgents: [],
    otherNames: new Map([
      [BRIAN, "Brian"],
      [ANDY, "Andy"],
    ]),
    projectNames: new Map([[OTHER_PROJECT, "Attic"]]),
    nowSeconds: NOW,
    ...overrides,
  });
}

function names(rows) {
  return rows.map((row) => row.name);
}

test("two projects with the same role names each list only their own agents", () => {
  const tank = build();
  assert.deepEqual(names(tank.projectAgents), ["Builder", "Loom"]);
  const attic = build({ projectRef: OTHER_PROJECT });
  assert.deepEqual(names(attic.projectAgents), ["Attic Builder"]);
  // Bob and Ira hold builder/verifier home roles and belong to neither.
  for (const model of [tank, attic]) {
    assert.equal(model.borrowed.length + model.previous.length, 0);
  }
  const builder = tank.projectAgents[0];
  assert.equal(builder.primaryRole, "builder");
  assert.equal(builder.state, "available");
  assert.deepEqual(builder.location, { kind: "here" });
  assert.equal(builder.mayAssociate, false);
});

test("an unassociated agent seated in an open session is borrowed, with its work attributed", () => {
  const model = build({
    executions: [
      execution(),
      execution({
        sessionId: "bob",
        agentRef: BOB,
        role: "builder",
        model: "sonnet",
        status: "running",
        packRef: packRef("builder"),
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
  assert.deepEqual(names(model.borrowed), ["Bob"]);
  const bob = model.borrowed[0];
  assert.equal(bob.isProjectAgent, false);
  assert.equal(bob.state, "working");
  assert.equal(bob.mayAssociate, true);
  assert.equal(bob.otherProject, null);
  assert.deepEqual(bob.seatedRoles, ["builder"]);
  assert.deepEqual(bob.relationship, {
    kind: "seated",
    role: "builder",
    sessionName: "Project team setup",
    assignerName: "Loom",
  });
  // Opening the seat opens the umbrella it sits in.
  assert.deepEqual(bob.sessions[0].openTarget, {
    channelId: "chan-1",
    generationId: "gen-lead",
    founded: false,
  });
  assert.equal(bob.assignments[0].latestDecision, "approve-with-notes");
  // Loom stays a project agent, idle in its open session.
  assert.deepEqual(names(model.projectAgents), ["Builder", "Loom"]);
  assert.equal(model.projectAgents[1].state, "idle");
});

test("another project's agent seated here is borrowed and names where it belongs", () => {
  const model = build({
    executions: [
      execution({
        sessionId: "attic",
        agentRef: ATTIC_BUILDER,
        role: "builder",
      }),
    ],
  });
  const row = model.borrowed[0];
  assert.equal(row.name, "Attic Builder");
  assert.deepEqual(row.otherProject, { ref: OTHER_PROJECT, name: "Attic" });
  assert.equal(row.mayAssociate, false, "belongs to another project");
});

test("an unassociated agent seen only in closed sessions is previously here, labelled borrowed", () => {
  const model = build({
    executions: [
      execution({
        sessionId: "ira",
        agentRef: IRA,
        role: "verifier",
        sessionRef: CLOSED_REF,
        status: "running",
      }),
    ],
  });
  assert.deepEqual(names(model.previous), ["Ira"]);
  const ira = model.previous[0];
  assert.equal(ira.isProjectAgent, false);
  // A closed session's last status is not a present tense.
  assert.equal(ira.state, "historical");
  assert.equal(ira.sessions[0].sessionClosed, true);
});

test("a project agent with closed history stays a project agent", () => {
  const model = build({
    executions: [execution({ sessionRef: CLOSED_REF, status: "completed" })],
  });
  assert.deepEqual(names(model.projectAgents), ["Builder", "Loom"]);
  assert.equal(model.previous.length, 0);
  const loom = model.projectAgents[1];
  assert.equal(loom.state, "available");
  assert.equal(loom.sessions.length, 1);
});

test("renaming keeps the grouping: membership is keyed by pubkey, not name", () => {
  const renamed = LOCAL.map((agent) =>
    agent.pubkey === BUILDER
      ? { ...agent, name: "Aardvark" }
      : agent.pubkey === BOB
        ? { ...agent, name: "Builder" }
        : agent,
  );
  const model = build({
    localAgents: renamed,
    executions: [
      execution({ sessionId: "bob", agentRef: BOB, role: "builder" }),
    ],
  });
  assert.deepEqual(
    model.projectAgents.map((row) => row.pubkey),
    [BUILDER, LOOM],
  );
  assert.deepEqual(
    model.borrowed.map((row) => [row.pubkey, row.name]),
    [[BOB, "Builder"]],
  );
});

test("an open session with a stopped execution is not working", () => {
  for (const status of ["stopped", "completed", "failed"]) {
    const model = build({ executions: [execution({ status })] });
    assert.equal(model.projectAgents[1].state, "available", status);
  }
  const borrowed = build({
    executions: [
      execution({ sessionId: "bob", agentRef: BOB, status: "stopped" }),
    ],
  });
  assert.equal(borrowed.borrowed[0].state, "not-running");
});

test("state follows the newest execution in an open session", () => {
  const cases = [
    ["starting", "working"],
    ["waiting_for_input", "working"],
    ["idle", "idle"],
    ["disconnected", "disconnected"],
    ["interrupted", "disconnected"],
  ];
  for (const [status, expected] of cases) {
    const model = build({ executions: [execution({ status })] });
    assert.equal(model.projectAgents[1].state, expected, status);
  }
  // An older running execution does not outrank a newer stopped one.
  const model = build({
    executions: [
      execution({
        sessionId: "old",
        status: "running",
        statusAt: (NOW - 7_200) * 1_000,
      }),
      execution({
        sessionId: "new",
        status: "stopped",
        statusAt: (NOW - 60) * 1_000,
      }),
    ],
  });
  assert.equal(model.projectAgents[1].state, "available");
});

test("an installed agent whose record lacks the association is listed with a warning", () => {
  const model = build({
    installations: [
      { role: "verifier", agentPubkey: IRA, packRef: packRef("verifier") },
      // Installed here, but its record belongs to another project: not this one's.
      {
        role: "builder",
        agentPubkey: ATTIC_BUILDER,
        packRef: packRef("builder"),
      },
    ],
  });
  assert.deepEqual(names(model.projectAgents), ["Builder", "Ira", "Loom"]);
  const ira = model.projectAgents[1];
  assert.equal(ira.associationMissing, true);
  assert.equal(ira.state, "not-associated");
  assert.equal(ira.mayAssociate, true);
  assert.equal(model.projectAgents[0].associationMissing, false);
});

test("a published association from an authorized author is a project agent on another computer", () => {
  const model = build({
    publishedAgents: [
      {
        pubkey: REMOTE,
        ownerPubkey: ANDY,
        name: "Andy's Runner",
        homeRole: "runner",
        projectDigest: projectAgentDigest(PROJECT),
        createdAt: NOW - 100,
        authority: "verified",
      },
      {
        // A claim for a different project's digest never lands here.
        pubkey: "8".repeat(64),
        ownerPubkey: ANDY,
        name: "Elsewhere",
        homeRole: "runner",
        projectDigest: projectAgentDigest(OTHER_PROJECT),
        createdAt: NOW - 100,
      },
    ],
  });
  assert.deepEqual(names(model.projectAgents), [
    "Andy's Runner",
    "Builder",
    "Loom",
  ]);
  const remote = model.projectAgents[0];
  assert.equal(remote.state, "elsewhere");
  assert.equal(remote.managedHere, false);
  assert.equal(remote.primaryRole, "runner");
  assert.deepEqual(remote.location, {
    kind: "elsewhere",
    ownerPubkey: ANDY,
    ownerName: "Andy",
  });
  assert.equal(remote.mayAssociate, false);
});

test("a publication cannot re-associate an agent this computer holds", () => {
  const model = build({
    publishedAgents: [
      {
        pubkey: BOB,
        ownerPubkey: OWNER,
        name: "Bob",
        homeRole: "builder",
        projectDigest: projectAgentDigest(PROJECT),
        createdAt: NOW,
      },
    ],
  });
  assert.equal(
    model.projectAgents.some((row) => row.pubkey === BOB),
    false,
    "the local record says Bob belongs to no project",
  );
});

test("a staged revision that differs from the installed one is flagged; an unknown is not", () => {
  const model = build({
    installations: [
      { role: "builder", agentPubkey: BUILDER, packRef: packRef("builder") },
      { role: "lead", agentPubkey: LOOM, packRef: packRef("lead") },
    ],
    executions: [
      execution({ packRef: null }),
      execution({
        sessionId: "builder",
        agentRef: BUILDER,
        role: "builder",
        packRef: packRef("builder", OLDER_SHA),
      }),
    ],
  });
  const byName = new Map(model.projectAgents.map((row) => [row.name, row]));
  assert.equal(
    byName.get("Builder").sessions[0].packDiffersFromInstalled,
    true,
  );
  assert.equal(byName.get("Loom").sessions[0].packDiffersFromInstalled, false);
});

test("the setup bootstrap may not be offered association", () => {
  const model = build({
    localAgents: [
      ...LOCAL,
      local("0".repeat(64), "Project Setup", "project-setup"),
    ],
    executions: [
      execution({
        sessionId: "setup",
        agentRef: "0".repeat(64),
        role: "project-setup",
      }),
    ],
  });
  assert.equal(model.borrowed[0].mayAssociate, false);
});

// ── Published associations: authorization and newest claim ────────────────

function wire(author, agent, projectRef, createdAt, extra = {}) {
  return {
    id: `${author.slice(0, 2)}${createdAt}`.padEnd(64, "0"),
    kind: 30177,
    pubkey: author,
    created_at: createdAt,
    tags: [["d", agent]],
    content: JSON.stringify({
      name: extra.name ?? "Runner",
      home_role: "runner",
      ...(projectRef ? { project_digest: projectAgentDigest(projectRef) } : {}),
    }),
  };
}

test("authorized authors are the creator and roster owners and collaborators only", () => {
  assert.deepEqual(
    projectAgentAuthorizedAuthors(OWNER.toUpperCase(), [
      { pubkey: COLLAB, role: "collaborator" },
      { pubkey: ANDY, role: "owner" },
      { pubkey: VIEWER, role: "viewer" },
    ]),
    [ANDY, OWNER, COLLAB].sort(),
  );
});

test("a viewer's or stranger's claim is ignored; a newer claim supersedes an older one", () => {
  const authors = projectAgentAuthorizedAuthors(OWNER, [
    { pubkey: COLLAB, role: "collaborator" },
    { pubkey: VIEWER, role: "viewer" },
  ]);
  const accepted = acceptPublishedProjectAgents({
    projectRef: PROJECT,
    authorizedAuthors: authors,
    rosterVerified: true,
    events: [
      wire(COLLAB, REMOTE, PROJECT, 100),
      wire(VIEWER, "8".repeat(64), PROJECT, 100),
      wire(BRIAN, "9".repeat(64), PROJECT, 100),
      // Owner's older claim for BOB is superseded by a newer one without it.
      wire(OWNER, BOB, PROJECT, 100),
      wire(OWNER, BOB, null, 200),
      { kind: 1, pubkey: OWNER, created_at: 1, tags: [], content: "" },
    ],
  });
  assert.deepEqual(
    accepted.map((entry) => [entry.pubkey, entry.ownerPubkey]),
    [[REMOTE, COLLAB]],
  );
});

test("associate access: creator and collaborators yes; viewer, unknown and loading no, with a reason", () => {
  const base = {
    creatorPubkey: OWNER,
    roster: [
      { pubkey: COLLAB, role: "collaborator" },
      { pubkey: VIEWER, role: "viewer" },
    ],
    projectName: "Tank Loop",
    rosterLoading: false,
    rosterError: null,
    identityError: null,
  };
  assert.equal(
    projectAgentAssociateAccess({ ...base, selfPubkey: OWNER }).kind,
    "allowed",
  );
  assert.equal(
    projectAgentAssociateAccess({ ...base, selfPubkey: COLLAB }).kind,
    "allowed",
  );
  assert.match(
    projectAgentAssociateAccess({ ...base, selfPubkey: VIEWER }).reason,
    /Only Tank Loop's owners and collaborators/,
  );
  assert.match(
    projectAgentAssociateAccess({
      ...base,
      selfPubkey: COLLAB,
      rosterLoading: true,
    }).reason,
    /Checking your access/,
  );
  assert.match(
    projectAgentAssociateAccess({
      ...base,
      selfPubkey: COLLAB,
      rosterError: "relay down",
    }).reason,
    /could not be read.*relay down/,
  );
});
