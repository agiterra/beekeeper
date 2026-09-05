import assert from "node:assert/strict";
import { test } from "node:test";

import {
  buildRolesView,
  describePacksSource,
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
