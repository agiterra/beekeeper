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
    projectId: "proj-1",
    installedOnly: false,
  });
  assert.equal(rightRole.length, 1);

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

// ── Installed for a project (Tank Loop walkthrough §2) ─────────────────────
//
// Installation is recorded in the setup journals, not in seats. An agent a
// project's setup installed a minute ago has no seat yet, and must still be
// found under that project — stopped or running — without that fact being
// written into `seatProjectIds`.

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

function installedDirectory({ status = "stopped", seats = [], ref } = {}) {
  return buildAgentDirectory({
    managedAgents: [managedAgent({ homeRole: null, status })],
    relayAgents: [],
    seats,
    openSeatKeys: new Set(seats.map((entry) => entry.key)),
    defaultModel: "gpt",
    projects: [TANK_LOOP, OTHER_PROJECT],
    installationsByAgent: new Map([
      [LEAD_PUBKEY, [{ projectRef: ref ?? TANK_LOOP.address, role: "lead" }]],
    ]),
  });
}

test("an installed agent with no seat appears under its project's filter", () => {
  const rows = installedDirectory();
  const row = rows[0];
  assert.equal(row.seatProjectIds.size, 0, "installation is not a seat");
  assert.deepEqual([...row.installedProjectIds], [TANK_LOOP.id]);
  assert.deepEqual(row.installedProjects, [
    {
      projectRef: TANK_LOOP.address,
      projectId: TANK_LOOP.id,
      projectName: "Tank Loop",
      role: "lead",
    },
  ]);
  assert.equal(
    agentDirectoryFilter(rows, { ...ANY_FILTERS, projectId: TANK_LOOP.id })
      .length,
    1,
  );
  assert.equal(
    agentDirectoryFilter(rows, { ...ANY_FILTERS, projectId: OTHER_PROJECT.id })
      .length,
    0,
  );
  assert.ok(row.roleSlugs.includes("lead"), "the installed role is filterable");
});

test("a stopped installed agent stays visible under the project filter with status any", () => {
  const rows = installedDirectory({ status: "stopped" });
  assert.equal(rows[0].isStopped, true);
  const visible = agentDirectoryFilter(rows, {
    ...ANY_FILTERS,
    projectId: TANK_LOOP.id,
  });
  assert.deepEqual(
    visible.map((row) => row.pubkey),
    [LEAD_PUBKEY],
  );
  // "Running" keeps its meaning: an installed, stopped agent is not running.
  assert.equal(
    agentDirectoryFilter(rows, {
      ...ANY_FILTERS,
      projectId: TANK_LOOP.id,
      status: "running",
    }).length,
    0,
  );
});

test("project identity: the journal's address resolves to the container id, case-folded, and either spelling filters", () => {
  const rows = installedDirectory({
    ref: `30621:${TANK_OWNER.toUpperCase()}:tank-loop`,
  });
  assert.equal(rows[0].installedProjects[0].projectId, TANK_LOOP.id);
  assert.equal(
    agentDirectoryFilter(rows, { ...ANY_FILTERS, projectId: TANK_LOOP.id })
      .length,
    1,
    "filter by project.id",
  );
  assert.equal(
    agentDirectoryFilter(rows, {
      ...ANY_FILTERS,
      projectId: TANK_LOOP.address,
    }).length,
    1,
    "filter by project address",
  );
});

test("an installation for a project that is not listed keeps its ref but invents no id", () => {
  const rows = installedDirectory({ ref: `30621:${"d".repeat(64)}:hidden` });
  const [installation] = rows[0].installedProjects;
  assert.equal(installation.projectId, null);
  assert.equal(installation.projectName, null);
  assert.equal(rows[0].installedProjectIds.size, 0);
  assert.equal(
    agentDirectoryFilter(rows, {
      ...ANY_FILTERS,
      projectId: `${"d".repeat(64)}:hidden`,
    }).length,
    0,
    "no string surgery turns an address into an id",
  );
});

test("seat meaning is unchanged: a seat-only agent still matches its project and has no installation", () => {
  const rows = buildAgentDirectory({
    managedAgents: [managedAgent()],
    relayAgents: [],
    seats: [seat()],
    openSeatKeys: new Set(["chan-1/gen-1"]),
    defaultModel: "gpt",
    projects: [TANK_LOOP],
    installationsByAgent: new Map(),
  });
  const row = rows[0];
  assert.deepEqual([...row.seatProjectIds], ["proj-1"]);
  assert.deepEqual(row.installedProjects, []);
  assert.equal(
    agentDirectoryFilter(rows, { ...ANY_FILTERS, projectId: "proj-1" }).length,
    1,
  );
  assert.equal(
    agentDirectoryFilter(rows, {
      ...ANY_FILTERS,
      status: "installed-for-project",
    }).length,
    0,
  );
});

test("status installed-for-project narrows to the chosen project's installations", () => {
  const rows = installedDirectory({ status: "running" });
  assert.equal(
    agentDirectoryFilter(rows, {
      ...ANY_FILTERS,
      status: "installed-for-project",
    }).length,
    1,
  );
  assert.equal(
    agentDirectoryFilter(rows, {
      ...ANY_FILTERS,
      status: "installed-for-project",
      projectId: TANK_LOOP.id,
    }).length,
    1,
  );
  assert.equal(
    agentDirectoryFilter(rows, {
      ...ANY_FILTERS,
      status: "installed-for-project",
      projectId: OTHER_PROJECT.id,
    }).length,
    0,
  );
});

test("building without installation input leaves every row with none (older callers)", () => {
  const rows = buildAgentDirectory({
    managedAgents: [managedAgent()],
    relayAgents: [],
    seats: [],
    openSeatKeys: new Set(),
    defaultModel: "gpt",
  });
  assert.deepEqual(rows[0].installedProjects, []);
  assert.equal(rows[0].installedProjectIds.size, 0);
});
