import assert from "node:assert/strict";
import { test } from "node:test";

import {
  agentDirectoryFilter,
  buildAgentDirectory,
  roleHistoryText,
} from "@/features/agents/lib/agentDirectoryModel";

const LEAD_PUBKEY = "1".repeat(64);
const WIRE_PUBKEY = "2".repeat(64);

function managedAgent(overrides = {}) {
  return {
    pubkey: LEAD_PUBKEY,
    name: "Ada",
    homeRole: "lead",
    hasRolePack: true,
    packRefusedSharedHome: false,
    modelSource: "global",
    model: null,
    provider: null,
    status: "running",
    ...overrides,
  };
}

function relayAgent(overrides = {}) {
  return {
    pubkey: WIRE_PUBKEY,
    ownerPubkey: null,
    name: "Grace",
    agentType: "acp",
    channels: [],
    channelIds: [],
    capabilities: [],
    status: "online",
    respondTo: null,
    respondToAllowlist: [],
    ...overrides,
  };
}

function seat(overrides = {}) {
  return {
    key: "chan-1/gen-1",
    channelId: "chan-1",
    generationId: "gen-1",
    label: "Fix the gate",
    agentPubkey: LEAD_PUBKEY,
    agentName: "Ada",
    role: "lead",
    projectId: "proj-1",
    projectName: "Beekeeper",
    status: "running",
    ageSeconds: 300,
    packSha: null,
    ...overrides,
  };
}

test("roleHistoryText sorts by count desc then slug asc, caps at three groups", () => {
  assert.equal(
    roleHistoryText(
      new Map([
        ["verifier", 3],
        ["lead", 12],
      ]),
    ),
    "lead ×12 · verifier ×3",
  );
  assert.equal(
    roleHistoryText(
      new Map([
        ["a", 1],
        ["b", 1],
        ["c", 1],
        ["d", 1],
      ]),
    ),
    "a ×1 · b ×1 · c ×1 · +1 more",
  );
  assert.equal(roleHistoryText(new Map()), "no seats recorded");
});

test("buildAgentDirectory produces one row per known pubkey, sorted by name", () => {
  const rows = buildAgentDirectory({
    managedAgents: [managedAgent()],
    relayAgents: [relayAgent()],
    seats: [seat()],
    openSeatKeys: new Set(["chan-1/gen-1"]),
    defaultModel: "gpt",
  });
  assert.equal(rows.length, 2);
  assert.deepEqual(
    rows.map((row) => row.name),
    ["Ada", "Grace"],
  );
});

test("a managed agent's current seat is the freshest open seat; a wire-only agent reads Model not known here", () => {
  const rows = buildAgentDirectory({
    managedAgents: [managedAgent()],
    relayAgents: [relayAgent()],
    seats: [seat()],
    openSeatKeys: new Set(["chan-1/gen-1"]),
    defaultModel: "gpt",
  });
  const ada = rows.find((row) => row.pubkey === LEAD_PUBKEY);
  const grace = rows.find((row) => row.pubkey === WIRE_PUBKEY);
  assert.ok(ada.currentSeat);
  assert.equal(ada.currentSeat.projectName, "Beekeeper");
  assert.equal(ada.isInstalled, true);
  assert.equal(grace.isInstalled, false);
  assert.equal(grace.modelLabel, "Model not known here");
  assert.equal(grace.currentSeat, null);
});

test("a closed seat never becomes the current seat, but still counts in role history and seat list", () => {
  const rows = buildAgentDirectory({
    managedAgents: [managedAgent()],
    relayAgents: [],
    seats: [
      seat({
        key: "chan-2/gen-2",
        channelId: "chan-2",
        generationId: "gen-2",
        status: "completed",
      }),
    ],
    openSeatKeys: new Set(), // nothing is open
    defaultModel: "gpt",
  });
  const ada = rows[0];
  assert.equal(ada.currentSeat, null);
  assert.equal(ada.seats.length, 1);
  assert.equal(ada.seats[0].isClosed, true);
  assert.equal(ada.roleHistoryLabel, "lead ×1");
  assert.ok(ada.seatProjectIds.has("proj-1"));
});

test("an agent with no seats at all reads no seats recorded and roleSlugs still carries its home role", () => {
  const rows = buildAgentDirectory({
    managedAgents: [managedAgent({ homeRole: "verifier" })],
    relayAgents: [],
    seats: [],
    openSeatKeys: new Set(),
    defaultModel: "gpt",
  });
  const row = rows[0];
  assert.equal(row.roleHistoryLabel, "no seats recorded");
  assert.deepEqual(row.roleSlugs, ["verifier"]);
});

test("agentDirectoryFilter: installedOnly defaults exclude wire-only agents", () => {
  const rows = buildAgentDirectory({
    managedAgents: [managedAgent()],
    relayAgents: [relayAgent()],
    seats: [],
    openSeatKeys: new Set(),
    defaultModel: "gpt",
  });
  const filtered = agentDirectoryFilter(rows, {
    role: null,
    status: "any",
    projectId: null,
    installedOnly: true,
  });
  assert.deepEqual(
    filtered.map((row) => row.pubkey),
    [LEAD_PUBKEY],
  );
});

test("agentDirectoryFilter: status, role and project filters compose", () => {
  const rows = buildAgentDirectory({
    managedAgents: [managedAgent()],
    relayAgents: [],
    seats: [seat()],
    openSeatKeys: new Set(["chan-1/gen-1"]),
    defaultModel: "gpt",
  });
  const seatedOnly = agentDirectoryFilter(rows, {
    role: null,
    status: "seated",
    projectId: null,
    installedOnly: false,
  });
  assert.equal(seatedOnly.length, 1);

  const wrongProject = agentDirectoryFilter(rows, {
    role: null,
    status: "any",
    projectId: "some-other-project",
    installedOnly: false,
  });
  assert.equal(wrongProject.length, 0);

  const rightRole = agentDirectoryFilter(rows, {
    role: "lead",
    status: "any",
    projectId: null,
    installedOnly: false,
  });
  assert.equal(rightRole.length, 1);

  // A seat in a project's session is not membership of it.
  const seatProject = agentDirectoryFilter(rows, {
    role: "lead",
    status: "any",
    projectId: "proj-1",
    installedOnly: false,
  });
  assert.equal(seatProject.length, 0);

  const wrongRole = agentDirectoryFilter(rows, {
    role: "verifier",
    status: "any",
    projectId: null,
    installedOnly: false,
  });
  assert.equal(wrongRole.length, 0);
});

test("agentDirectoryFilter: running/stopped never match a wire-only agent", () => {
  const rows = buildAgentDirectory({
    managedAgents: [],
    relayAgents: [relayAgent()],
    seats: [],
    openSeatKeys: new Set(),
    defaultModel: "gpt",
  });
  assert.equal(
    agentDirectoryFilter(rows, {
      role: null,
      status: "running",
      projectId: null,
      installedOnly: false,
    }).length,
    0,
  );
  assert.equal(
    agentDirectoryFilter(rows, {
      role: null,
      status: "stopped",
      projectId: null,
      installedOnly: false,
    }).length,
    0,
  );
});

test("directory preserves stopped-agent errors and running restart state without reviving stale errors", () => {
  const base = {
    relayAgents: [],
    seats: [],
    openSeatKeys: new Set(),
    defaultModel: "",
  };
  const stopped = buildAgentDirectory({
    ...base,
    managedAgents: [
      managedAgent({
        status: "stopped",
        lastError: "harness exited with status 1",
        needsRestart: false,
      }),
    ],
  });
  assert.equal(stopped[0].lastError, "harness exited with status 1");
  const running = buildAgentDirectory({
    ...base,
    managedAgents: [
      managedAgent({
        status: "running",
        lastError: "stale failure",
        needsRestart: true,
      }),
    ],
  });
  assert.equal(running[0].lastError, null);
  assert.equal(running[0].needsRestart, true);
});

// ── Project membership is association ───────────────────────────────────
//
// `ManagedAgent.projectRef` places an agent under a project. A seat is a
// secondary fact, and a setup journal installation without the association
// is a warning (listed under that project so the gap is visible), never
// membership on its own.

const TANK_OWNER = "c".repeat(64);
const TANK_LOOP = {
  id: `${TANK_OWNER}:tank-loop`,
  address: `30621:${TANK_OWNER}:tank-loop`,
  name: "Tank Loop",
};
const OTHER_PROJECT = {
  id: `${TANK_OWNER}:attic`,
  address: `30621:${TANK_OWNER}:attic`,
  name: "Attic",
};
const ANY_FILTERS = {
  role: null,
  status: "any",
  projectId: null,
  installedOnly: true,
};

function directory({
  agents = [managedAgent()],
  seats = [],
  installations = new Map(),
  relayAgents = [],
} = {}) {
  return buildAgentDirectory({
    managedAgents: agents,
    relayAgents,
    seats,
    openSeatKeys: new Set(seats.map((entry) => entry.key)),
    defaultModel: "gpt",
    projects: [TANK_LOOP, OTHER_PROJECT],
    installationsByAgent: installations,
  });
}

function pubkeysUnder(rows, projectId, extra = {}) {
  return agentDirectoryFilter(rows, {
    ...ANY_FILTERS,
    projectId,
    ...extra,
  }).map((row) => row.pubkey);
}

test("an associated agent is listed under its project, and only there", () => {
  const rows = directory({
    agents: [
      managedAgent({ homeRole: "builder", projectRef: TANK_LOOP.address }),
      managedAgent({
        pubkey: WIRE_PUBKEY,
        name: "Attic Builder",
        homeRole: "builder",
        projectRef: OTHER_PROJECT.address,
      }),
    ],
  });
  assert.deepEqual(pubkeysUnder(rows, TANK_LOOP.id), [LEAD_PUBKEY]);
  assert.deepEqual(pubkeysUnder(rows, OTHER_PROJECT.id), [WIRE_PUBKEY]);
  // Either spelling of the project filters the same.
  assert.deepEqual(pubkeysUnder(rows, TANK_LOOP.address), [LEAD_PUBKEY]);
  const ada = rows.find((row) => row.pubkey === LEAD_PUBKEY);
  assert.deepEqual(ada.project, {
    projectRef: TANK_LOOP.address,
    projectId: TANK_LOOP.id,
    projectName: "Tank Loop",
  });
  assert.equal(ada.projectKnown, true);
});

test("a seat is not membership: a seated, unassociated agent is not under the project", () => {
  const rows = directory({
    agents: [managedAgent({ homeRole: "builder" })],
    seats: [seat({ projectId: TANK_LOOP.id, projectName: "Tank Loop" })],
  });
  const row = rows[0];
  assert.equal(row.project, null);
  assert.deepEqual([...row.seatProjectIds], [TANK_LOOP.id]);
  assert.ok(row.currentSeat, "the seat is still shown as a secondary fact");
  assert.deepEqual(pubkeysUnder(rows, TANK_LOOP.id), []);
});

test("an installed agent without the association is listed under that project with a warning", () => {
  const rows = directory({
    agents: [managedAgent({ homeRole: "lead", status: "stopped" })],
    installations: new Map([
      [LEAD_PUBKEY, [{ projectRef: TANK_LOOP.address, role: "lead" }]],
    ]),
  });
  const row = rows[0];
  assert.equal(row.project, null);
  assert.equal(row.unassociatedInstallations.length, 1);
  assert.deepEqual(pubkeysUnder(rows, TANK_LOOP.id), [LEAD_PUBKEY]);
  assert.deepEqual(pubkeysUnder(rows, OTHER_PROJECT.id), []);
  // It is not a project agent until associated.
  assert.deepEqual(
    pubkeysUnder(rows, TANK_LOOP.id, { status: "project-agent" }),
    [],
  );
  assert.ok(row.roleSlugs.includes("lead"), "the installed role is filterable");
});

test("once associated, an installation is no longer a warning, and another project's journal does not place it", () => {
  const rows = directory({
    agents: [managedAgent({ projectRef: TANK_LOOP.address })],
    installations: new Map([
      [LEAD_PUBKEY, [{ projectRef: OTHER_PROJECT.address, role: "lead" }]],
    ]),
  });
  const row = rows[0];
  assert.deepEqual(row.unassociatedInstallations, []);
  assert.deepEqual(pubkeysUnder(rows, TANK_LOOP.id), [LEAD_PUBKEY]);
  assert.deepEqual(pubkeysUnder(rows, OTHER_PROJECT.id), []);
});

test("rename keeps the project: association is keyed by the record, not the name", () => {
  const before = directory({
    agents: [managedAgent({ name: "Loom", projectRef: TANK_LOOP.address })],
  });
  const after = directory({
    agents: [
      managedAgent({ name: "Tank Lead", projectRef: TANK_LOOP.address }),
    ],
  });
  assert.deepEqual(before[0].project, after[0].project);
  assert.deepEqual(pubkeysUnder(after, TANK_LOOP.id), [LEAD_PUBKEY]);
});

test("an association with a project this viewer does not list keeps its ref but invents no id", () => {
  const hidden = `30621:${"d".repeat(64)}:hidden`;
  const rows = directory({ agents: [managedAgent({ projectRef: hidden })] });
  assert.deepEqual(rows[0].project, {
    projectRef: hidden,
    projectId: null,
    projectName: null,
  });
  assert.deepEqual(pubkeysUnder(rows, `${"d".repeat(64)}:hidden`), []);
  assert.deepEqual(pubkeysUnder(rows, hidden), [LEAD_PUBKEY]);
});

test("a carried digest from another computer is not membership here, and is said so", () => {
  const rows = directory({
    agents: [
      managedAgent({
        homeRole: "builder",
        carriedProjectDigest: "a".repeat(64),
      }),
      managedAgent({
        pubkey: WIRE_PUBKEY,
        name: "Associated",
        projectRef: TANK_LOOP.address,
        carriedProjectDigest: "a".repeat(64),
      }),
    ],
  });
  const carried = rows.find((row) => row.pubkey === LEAD_PUBKEY);
  assert.equal(carried.project, null);
  assert.equal(carried.carriedFromAnotherComputer, true);
  assert.deepEqual(pubkeysUnder(rows, TANK_LOOP.id), [WIRE_PUBKEY]);
  const associated = rows.find((row) => row.pubkey === WIRE_PUBKEY);
  assert.equal(associated.carriedFromAnotherComputer, false);
  const plain = directory({ agents: [managedAgent({ homeRole: "builder" })] });
  assert.equal(plain[0].carriedFromAnotherComputer, false);
});

test("a wire-only agent's project is unknown here, not none", () => {
  const rows = directory({ agents: [], relayAgents: [relayAgent()] });
  assert.equal(rows[0].project, null);
  assert.equal(rows[0].projectKnown, false);
});

test("status project-agent narrows to associated agents, and to the chosen project", () => {
  const rows = directory({
    agents: [
      managedAgent({ projectRef: TANK_LOOP.address }),
      managedAgent({ pubkey: WIRE_PUBKEY, name: "Solo" }),
    ],
  });
  assert.deepEqual(
    agentDirectoryFilter(rows, { ...ANY_FILTERS, status: "project-agent" }).map(
      (row) => row.pubkey,
    ),
    [LEAD_PUBKEY],
  );
  assert.deepEqual(
    pubkeysUnder(rows, OTHER_PROJECT.id, { status: "project-agent" }),
    [],
  );
});

test("building without installation or project input leaves every row with none (older callers)", () => {
  const rows = buildAgentDirectory({
    managedAgents: [managedAgent()],
    relayAgents: [],
    seats: [],
    openSeatKeys: new Set(),
    defaultModel: "gpt",
  });
  assert.deepEqual(rows[0].installedProjects, []);
  assert.equal(rows[0].installedProjectIds.size, 0);
  assert.equal(rows[0].project, null);
});
