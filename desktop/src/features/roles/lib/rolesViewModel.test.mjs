import assert from "node:assert/strict";
import { test } from "node:test";

import {
  buildRolesView,
  describePacksSource,
  roleAgentPackState,
  seatAgeSeconds,
} from "@/features/roles/lib/rolesViewModel";

const NOW = 1_800_000_000;
const SHA_A = "a".repeat(40);
const SHA_B = "b".repeat(40);
const LEAD_PUBKEY = "1".repeat(64);
const REVIEWER_PUBKEY = "2".repeat(64);
const STRANGER_PUBKEY = "3".repeat(64);

function pack(overrides = {}) {
  return {
    role: "lead",
    displayName: "Lead",
    description: "Runs the crew.",
    summary: "Triage first, then brief.",
    version: "1.2.0",
    origin: "project",
    packDir: "/packs/personas/roles/lead",
    packRef: { repo: "30617:owner:packs", sha: SHA_A },
    skills: [{ name: "triage", description: "Sort findings", shared: false }],
    refusal: null,
    ...overrides,
  };
}

function agent(overrides = {}) {
  return {
    pubkey: LEAD_PUBKEY,
    name: "Ada",
    homeRole: "lead",
    hasRolePack: true,
    packRefusedSharedHome: false,
    status: "running",
    ...overrides,
  };
}

function project(overrides = {}) {
  return { id: "proj-1", name: "Beekeeper", ...overrides };
}

function entry(overrides = {}) {
  const { session: sessionOverrides = {}, ...rest } = overrides;
  return {
    placement: "project",
    projectId: "proj-1",
    channelId: "chan-1",
    generationId: "gen-1",
    label: "Fix the gate",
    isClosed: false,
    isArchived: false,
    session: {
      status: "running",
      statusAt: (NOW - 90) * 1_000,
      agentRef: LEAD_PUBKEY,
      role: "lead",
      packRef: {
        repo: "30617:owner:packs",
        sha: SHA_A,
        role: "lead",
        path: "",
      },
      ...sessionOverrides,
    },
    ...rest,
  };
}

test("seatAgeSeconds converts catalog milliseconds to whole seconds and never goes negative", () => {
  assert.equal(seatAgeSeconds((NOW - 90) * 1_000, NOW), 90);
  assert.equal(seatAgeSeconds((NOW + 5) * 1_000, NOW), 0);
  assert.equal(seatAgeSeconds(null, NOW), null);
  assert.equal(seatAgeSeconds(undefined, NOW), null);
  assert.equal(seatAgeSeconds(Number.NaN, NOW), null);
});

test("a chip carries the agent record's own avatar, harness and model, unchanged", () => {
  const view = buildRolesView({
    rolePacks: [pack()],
    agents: [
      agent({
        avatarUrl: "https://example.test/ada.png",
        runtime: "goose",
        model: "claude-opus",
      }),
    ],
    shelfEntries: [],
    projects: [project()],
    nowSeconds: NOW,
  });
  const chip = view.roles.find((row) => row.role === "lead")?.agents[0];
  assert.ok(chip);
  assert.equal(chip.avatarUrl, "https://example.test/ada.png");
  assert.equal(chip.runtime, "goose");
  assert.equal(chip.model, "claude-opus");

  // A record that names none of the three passes `null` through rather than
  // inventing a face, a harness or a model for it.
  const bare = buildRolesView({
    rolePacks: [pack()],
    agents: [agent({ avatarUrl: null, runtime: null, model: null })],
    shelfEntries: [],
    projects: [project()],
    nowSeconds: NOW,
  });
  const bareChip = bare.roles.find((row) => row.role === "lead")?.agents[0];
  assert.ok(bareChip);
  assert.equal(bareChip.avatarUrl, null);
  assert.equal(bareChip.runtime, null);
  assert.equal(bareChip.model, null);
});

test("an agent with a home role and no pack on this computer keeps that disclosure on its chip", () => {
  const view = buildRolesView({
    rolePacks: [pack()],
    agents: [agent({ hasRolePack: false })],
    shelfEntries: [],
    projects: [project()],
    nowSeconds: NOW,
  });
  const lead = view.roles.find((row) => row.role === "lead");
  assert.ok(lead);
  assert.equal(lead.hasPack, true);
  assert.deepEqual(
    lead.agents.map((chip) => [chip.name, chip.hasRolePack]),
    [["Ada", false]],
  );
});

test("an agent whose home role has no pack at all still gets a row that says so", () => {
  const view = buildRolesView({
    rolePacks: [pack()],
    agents: [
      agent({ pubkey: REVIEWER_PUBKEY, name: "Bo", homeRole: "reviewer" }),
    ],
    shelfEntries: [],
    projects: [],
    nowSeconds: NOW,
  });
  const reviewer = view.roles.find((row) => row.role === "reviewer");
  assert.ok(reviewer);
  assert.equal(reviewer.hasPack, false);
  assert.equal(reviewer.displayName, "reviewer");
  assert.equal(reviewer.origin, null);
  assert.deepEqual(reviewer.skills, []);
  assert.deepEqual(
    reviewer.agents.map((chip) => chip.pubkey),
    [REVIEWER_PUBKEY],
  );
});

test("a seat whose role has no pack is listed under an unknown-role row, not dropped", () => {
  const view = buildRolesView({
    rolePacks: [pack()],
    agents: [],
    shelfEntries: [
      entry({
        generationId: "gen-9",
        session: { agentRef: STRANGER_PUBKEY, role: "scribe", packRef: null },
      }),
    ],
    projects: [project()],
    nowSeconds: NOW,
  });
  const scribe = view.roles.find((row) => row.role === "scribe");
  assert.ok(scribe);
  assert.equal(scribe.hasPack, false);
  assert.equal(scribe.seats.length, 1);
  const seat = scribe.seats[0];
  assert.equal(seat.agentName, null, "an unmanaged agent has no name here");
  assert.equal(seat.agentPubkey, STRANGER_PUBKEY);
  assert.equal(seat.packSha, null, "no packRef means no sha, never a guess");
  assert.equal(seat.ageSeconds, 90);
  assert.equal(seat.status, "running");
});

test("a project with zero seats is present in byProject with an empty list", () => {
  const view = buildRolesView({
    rolePacks: [],
    agents: [],
    shelfEntries: [entry()],
    projects: [project(), project({ id: "proj-2", name: "Empty" })],
    nowSeconds: NOW,
  });
  assert.deepEqual(
    view.byProject.map((row) => [row.projectId, row.seats.length]),
    [
      ["proj-1", 1],
      ["proj-2", 0],
    ],
  );
  assert.equal(view.byProject[0].seats[0].projectName, "Beekeeper");
});

test("an unplaced seat, and a seat claimed by a project not in the list, land in unplaced", () => {
  const view = buildRolesView({
    rolePacks: [],
    agents: [],
    shelfEntries: [
      entry({
        placement: "unassigned",
        projectId: null,
        generationId: "gen-u",
      }),
      entry({ projectId: "proj-ghost", generationId: "gen-g" }),
      entry(),
    ],
    projects: [project()],
    nowSeconds: NOW,
  });
  assert.deepEqual(view.unplaced.map((seat) => seat.generationId).sort(), [
    "gen-g",
    "gen-u",
  ]);
  assert.equal(
    view.unplaced.find((s) => s.generationId === "gen-g").projectName,
    null,
  );
  assert.equal(view.byProject[0].seats.length, 1);
});

test("a person's session with no agent keeps its place under the project but joins no role", () => {
  const view = buildRolesView({
    rolePacks: [pack()],
    agents: [],
    shelfEntries: [entry({ session: { agentRef: null, role: null } })],
    projects: [project()],
    nowSeconds: NOW,
  });
  assert.equal(view.roles.length, 1);
  assert.equal(view.roles[0].seats.length, 0);
  assert.equal(view.byProject[0].seats.length, 1);
  assert.equal(view.byProject[0].seats[0].role, null);
});

test("closed and archived sessions hold no seat anywhere", () => {
  const view = buildRolesView({
    rolePacks: [pack()],
    agents: [],
    shelfEntries: [
      entry({ isClosed: true, generationId: "gen-closed" }),
      entry({ isClosed: true, isArchived: true, generationId: "gen-arch" }),
    ],
    projects: [project()],
    nowSeconds: NOW,
  });
  assert.equal(view.roles[0].seats.length, 0);
  assert.equal(view.byProject[0].seats.length, 0);
  assert.equal(view.unplaced.length, 0);
});

test("ordering is stable: roles by slug, seats freshest first then by key, chips by name", () => {
  const input = {
    rolePacks: [pack({ role: "reviewer", displayName: "Reviewer" }), pack()],
    agents: [
      agent({ pubkey: REVIEWER_PUBKEY, name: "Zed" }),
      agent({ name: "Ada" }),
    ],
    shelfEntries: [
      entry({
        generationId: "gen-b",
        session: { statusAt: (NOW - 10) * 1_000 },
      }),
      entry({
        generationId: "gen-a",
        session: { statusAt: (NOW - 10) * 1_000 },
      }),
      entry({ generationId: "gen-none", session: { statusAt: null } }),
      entry({
        generationId: "gen-old",
        session: { statusAt: (NOW - 500) * 1_000 },
      }),
    ],
    projects: [project()],
    nowSeconds: NOW,
  };
  const first = buildRolesView(input);
  const second = buildRolesView({
    ...input,
    rolePacks: [...input.rolePacks].reverse(),
    agents: [...input.agents].reverse(),
    shelfEntries: [...input.shelfEntries].reverse(),
  });
  assert.deepEqual(first, second);
  assert.deepEqual(
    first.roles.map((row) => row.role),
    ["lead", "reviewer"],
  );
  assert.deepEqual(
    first.roles[0].seats.map((seat) => seat.generationId),
    ["gen-a", "gen-b", "gen-old", "gen-none"],
  );
  assert.deepEqual(
    first.roles[0].agents.map((chip) => chip.name),
    ["Ada", "Zed"],
  );
});

test("describePacksSource reports a value only when every role agrees", () => {
  assert.deepEqual(describePacksSource([]), {
    origins: [],
    location: null,
    sha: null,
    shasDiffer: false,
  });
  assert.deepEqual(describePacksSource([pack(), pack({ role: "reviewer" })]), {
    origins: ["project"],
    location: "30617:owner:packs",
    sha: SHA_A,
    shasDiffer: false,
  });
  const mixed = describePacksSource([
    pack(),
    pack({
      role: "reviewer",
      origin: "shipped",
      packDir: "/app/shipped/reviewer",
      packRef: { repo: "app:shipped", sha: SHA_B },
    }),
  ]);
  assert.deepEqual(mixed.origins, ["project", "shipped"]);
  assert.equal(mixed.location, null);
  assert.equal(mixed.sha, null);
  assert.equal(mixed.shasDiffer, true);
  const unknown = describePacksSource([
    pack({ packRef: null }),
    pack({ role: "reviewer", packRef: null }),
  ]);
  assert.equal(unknown.location, "/packs/personas/roles");
  assert.equal(unknown.sha, null);
  assert.equal(unknown.shasDiffer, false);
});

test("roleAgentPackState reads the role row first, the agent's own probe last (Fix 2)", () => {
  // A shared home that refused the pack is a fact about this agent
  // specifically, so it outranks even a role with no pack at all.
  assert.equal(
    roleAgentPackState({
      roleHasPack: false,
      roleRefusal: null,
      agentPackRefusedSharedHome: true,
    }),
    "refused",
  );
  assert.equal(
    roleAgentPackState({
      roleHasPack: false,
      roleRefusal: null,
      agentPackRefusedSharedHome: false,
    }),
    "missing",
  );
  assert.equal(
    roleAgentPackState({
      roleHasPack: true,
      roleRefusal: "The ladder refused this role: duplicate slug.",
      agentPackRefusedSharedHome: undefined,
    }),
    "blocked",
  );
  // The role resolved a pack and nothing refuses it — present, even for an
  // agent whose own `hasRolePack` probe was never asked (`undefined`).
  assert.equal(
    roleAgentPackState({
      roleHasPack: true,
      roleRefusal: null,
      agentPackRefusedSharedHome: undefined,
    }),
    "present",
  );
});

function relayAgent(overrides = {}) {
  return {
    pubkey: REVIEWER_PUBKEY,
    ownerPubkey: "owner-andy",
    name: "Andy's reviewer",
    status: "offline",
    channelIds: ["chan-1"],
    capabilities: ["lead"],
    ...overrides,
  };
}

function projectView(overrides = {}) {
  return buildRolesView({
    rolePacks: [pack()],
    agents: [],
    relayAgents: [],
    shelfEntries: [],
    projects: [project(), project({ id: "proj-2" })],
    projectId: "proj-1",
    projectChannelIds: ["chan-1"],
    nowSeconds: NOW,
    ...overrides,
  });
}

test("selected project excludes other-project and unclaimed sessions, local home roles alone, and unplaced rows", () => {
  const view = projectView({
    agents: [agent({ homeRole: "builder" })],
    shelfEntries: [
      entry({ projectId: "proj-2", session: { role: "other-role" } }),
      entry({ projectId: null, session: { role: "unclaimed-role" } }),
    ],
  });
  assert.deepEqual(
    view.roles.map((row) => row.role),
    ["lead"],
  );
  assert.deepEqual(view.roles[0].agents, []);
  assert.deepEqual(
    view.byProject.map((row) => [row.projectId, row.seats.length]),
    [["proj-1", 0]],
  );
  assert.deepEqual(view.unplaced, []);
});

test("historical scoped sessions retain named remote participants without inventing a local process or pack status", () => {
  const view = projectView({
    relayAgents: [relayAgent({ channelIds: [] })],
    shelfEntries: [
      entry({
        isClosed: true,
        session: { agentRef: REVIEWER_PUBKEY, role: "reviewer" },
      }),
    ],
  });
  const reviewer = view.roles.find((row) => row.role === "reviewer");
  assert.equal(reviewer.seats.length, 0);
  assert.equal(reviewer.agents.length, 1);
  assert.equal(reviewer.agents[0].name, "Andy's reviewer");
  assert.equal(reviewer.agents[0].ownerPubkey, "owner-andy");
  assert.equal(reviewer.agents[0].isManagedHere, false);
  assert.equal(reviewer.agents[0].status, undefined);
  assert.equal(reviewer.agents[0].hasRolePack, undefined);
});

test("local and relay copies join case-insensitively and scoped session roles supplement explicit local home roles", () => {
  const key = "ab".repeat(32);
  const view = projectView({
    agents: [agent({ pubkey: key.toUpperCase(), homeRole: "lead" })],
    relayAgents: [relayAgent({ pubkey: key, name: "Shared name" })],
    shelfEntries: [
      entry({ session: { agentRef: key, role: "runner" } }),
      entry({
        generationId: "gen-2",
        session: { agentRef: key.toUpperCase(), role: "runner" },
      }),
    ],
  });
  assert.deepEqual(
    view.roles.map((row) => [row.role, row.agents.length]),
    [
      ["lead", 1],
      ["runner", 1],
    ],
  );
  const runner = view.roles.find((row) => row.role === "runner");
  assert.equal(runner.agents[0].pubkey, key);
  assert.equal(runner.agents[0].name, "Ada");
  assert.equal(runner.agents[0].isManagedHere, true);
  assert.equal(runner.agents[0].status, "running");
  assert.equal(runner.agents[0].ownerPubkey, "owner-andy");
  assert.equal(runner.seats[0].agentName, "Ada");
});

test("project channel membership admits local explicit home roles but never invents remote roles", () => {
  const view = projectView({
    agents: [agent()],
    relayAgents: [
      relayAgent({ pubkey: LEAD_PUBKEY }),
      relayAgent({ name: "Lead", homeRole: "lead", capabilities: ["lead"] }),
    ],
  });
  assert.deepEqual(
    view.roles[0].agents.map((chip) => chip.pubkey),
    [LEAD_PUBKEY],
  );
});

test("roleless remote sessions stay roleless, unknown identities keep their session pubkey", () => {
  const view = projectView({
    relayAgents: [relayAgent()],
    shelfEntries: [
      entry({
        session: { agentRef: REVIEWER_PUBKEY, role: null, packRef: null },
      }),
      entry({
        generationId: "gen-2",
        session: { agentRef: STRANGER_PUBKEY, role: "runner" },
      }),
    ],
  });
  assert.equal(view.roles[0].agents.length, 0);
  const roleless = view.byProject[0].seats.find((row) => row.role === null);
  assert.equal(roleless.agentName, "Andy's reviewer");
  const runner = view.roles.find((row) => row.role === "runner");
  assert.deepEqual(runner.agents, []);
  assert.equal(runner.seats[0].agentPubkey, STRANGER_PUBKEY);
  assert.equal(runner.seats[0].agentName, null);
});

test("remote pack availability stays unknown regardless of this computer's availability or refusals", () => {
  for (const roleHasPack of [true, false]) {
    for (const roleRefusal of [null, "Local checkout refused"]) {
      for (const agentPackRefusedSharedHome of [true, false, undefined]) {
        assert.equal(
          roleAgentPackState({
            isManagedHere: false,
            roleHasPack,
            roleRefusal,
            agentPackRefusedSharedHome,
          }),
          "unknown",
        );
      }
    }
  }
  assert.equal(
    roleAgentPackState({
      isManagedHere: true,
      roleHasPack: true,
      roleRefusal: null,
      agentPackRefusedSharedHome: undefined,
    }),
    "present",
  );
});

test("a worker seated inside another agent's umbrella counts under its role, only in its own project", () => {
  const owner = "6".repeat(64);
  const address = `30621:${owner}:tank-loop`;
  const execution = (overrides) => ({
    session: {
      agentRef: REVIEWER_PUBKEY,
      role: "builder",
      projectRef: address,
      ...overrides,
    },
  });
  const projects = [
    project({ address }),
    project({ id: "proj-2", address: `30621:${owner}:other` }),
  ];
  const agents = [
    agent({ pubkey: REVIEWER_PUBKEY, name: "Bob", homeRole: "builder" }),
  ];

  const view = projectView({
    agents,
    projects,
    // The umbrella row names only the lead; the worker is an execution of it.
    shelfEntries: [entry()],
    executions: [execution({})],
  });
  const builder = view.roles.find((row) => row.role === "builder");
  assert.deepEqual(
    builder.agents.map((chip) => chip.name),
    ["Bob"],
  );

  const elsewhere = projectView({
    agents,
    projects,
    shelfEntries: [entry()],
    executions: [execution({ projectRef: `30621:${owner}:other` })],
  });
  assert.equal(
    elsewhere.roles.find((row) => row.role === "builder"),
    undefined,
  );
});
