import assert from "node:assert/strict";
import { test } from "node:test";

import {
  groupTeamReadinessFacts,
  normalizeTeamReadinessRoles,
  teamReadinessLaunchGate,
} from "./teamReadinessModel.ts";
import { teamReadinessScopeKey } from "./useProjectTeamReadiness.ts";
import { prepareProjectForTeams } from "./teamReadinessPrepare.ts";

function readiness(overrides = {}) {
  return {
    schemaVersion: 3,
    projectRef: `30621:${"a".repeat(64)}:hive`,
    generatedAt: "2026-08-30T16:00:00Z",
    readyForFirstSession: false,
    ready: false,
    status: "blocked",
    hostClass: "cold",
    keyStoreSafe: true,
    source: {},
    team: {
      selectedRoles: ["lead"],
      availableRoles: [],
      packs: [],
      identities: [],
    },
    runtimes: [],
    registry: {
      providerTargets: [],
      coveredTargets: [],
      uncoveredTargets: [],
      pendingTargets: [],
    },
    provider: {
      relayUrl: "wss://hive.example",
      provisioned: false,
      process: "not_running",
    },
    policy: { hiringPolicy: "enabled" },
    relay: { state: "awaiting_first_session", source: "wire" },
    catalog: {
      state: "awaiting_first_session",
      targets: [],
      source: "wire",
      provenance: [],
    },
    facts: [],
    blockingCodes: [],
    unknownCodes: [],
    awaitingCodes: [],
    limitedCodes: [],
    ...overrides,
  };
}

test("Unknown blocks a project team launch and preserves the exact source remedy", () => {
  const response = readiness({
    status: "unknown",
    facts: [
      {
        category: "runtime",
        code: "RUNTIME_METADATA_UNOBSERVED",
        scope: "local",
        state: "unknown",
        summary: "Runtime metadata could not be read.",
        remedy: "Repair the runtime registry, then re-read readiness.",
      },
    ],
    unknownCodes: ["RUNTIME_METADATA_UNOBSERVED"],
  });
  assert.deepEqual(
    teamReadinessLaunchGate({
      projectRef: response.projectRef,
      loading: false,
      error: null,
      readiness: response,
    }),
    {
      allowed: false,
      reason:
        "Runtime metadata could not be read. Repair the runtime registry, then re-read readiness.",
    },
  );
});

test("wire status Unknown or Blocked cannot be overruled by readyForFirstSession", () => {
  for (const status of ["unknown", "blocked"]) {
    const response = readiness({
      readyForFirstSession: true,
      status,
      provider: {
        relayUrl: "wss://hive.example",
        provisioned: true,
        process: "live",
      },
    });
    assert.equal(
      teamReadinessLaunchGate({
        projectRef: response.projectRef,
        loading: false,
        error: null,
        readiness: response,
      }).allowed,
      false,
      status,
    );
  }
});

test("one ready provider permits the first launch while diversity remains visible as Limited", () => {
  const limited = {
    category: "registry",
    code: "PROVIDER_DIVERSITY_LIMITED",
    scope: "wire",
    state: "limited",
    summary: "Only one provider is currently available.",
    remedy: "Add another provider for failure-mode diversity.",
  };
  const response = readiness({
    readyForFirstSession: true,
    status: "awaiting_first_session",
    hostClass: "prepared_for_first_session",
    provider: {
      relayUrl: "wss://hive.example",
      provisioned: true,
      process: "live",
    },
    facts: [
      limited,
      {
        category: "catalog",
        code: "CATALOG_AWAITING_FIRST_SESSION",
        scope: "wire",
        state: "awaiting_first_session",
        summary: "No signed session catalog exists yet.",
        remedy: "Launch the first session.",
      },
      {
        category: "relay",
        code: "RELAY_UNOBSERVED",
        scope: "wire",
        state: "awaiting_first_session",
        summary: "The relay has not observed this project yet.",
        remedy: "Launch the first session.",
      },
    ],
    awaitingCodes: ["CATALOG_AWAITING_FIRST_SESSION", "RELAY_UNOBSERVED"],
    limitedCodes: [limited.code],
  });
  assert.equal(
    teamReadinessLaunchGate({
      projectRef: response.projectRef,
      loading: false,
      error: null,
      readiness: response,
    }).allowed,
    true,
  );
  assert.deepEqual(groupTeamReadinessFacts(response.facts).limited, [limited]);
});

test("Awaiting permits only the backend's first-session evidence codes", () => {
  const response = readiness({
    readyForFirstSession: true,
    status: "awaiting_first_session",
    provider: {
      relayUrl: "wss://hive.example",
      provisioned: true,
      process: "live",
    },
    awaitingCodes: ["PROVIDER_START_PENDING"],
    facts: [
      {
        category: "provider",
        code: "PROVIDER_START_PENDING",
        scope: "local",
        state: "awaiting_first_session",
        summary: "Provider startup has not completed.",
        remedy: "Wait for the provider.",
      },
    ],
  });
  const gate = teamReadinessLaunchGate({
    projectRef: response.projectRef,
    loading: false,
    error: null,
    readiness: response,
  });
  assert.equal(gate.allowed, false);
  assert.match(gate.reason, /Provider startup has not completed/);

  const omittedFromSummaryCodes = readiness({
    ...response,
    awaitingCodes: [],
  });
  assert.equal(
    teamReadinessLaunchGate({
      projectRef: omittedFromSummaryCodes.projectRef,
      loading: false,
      error: null,
      readiness: omittedFromSummaryCodes,
    }).allowed,
    false,
  );
});

test("role and scope normalization are stable and include every authority input", () => {
  assert.deepEqual(
    normalizeTeamReadinessRoles([" Lead ", "builder", "LEAD", ""]),
    ["builder", "lead"],
  );
  const base = {
    communityId: "community-a",
    relayUrl: "wss://relay-a.example/",
    channelIds: ["channel-b", "channel-a", "channel-a"],
    projectRef: " project ",
    checkoutPath: " /repo ",
    selectedRoles: ["Lead", "builder", "lead"],
    hiringPolicyEnabled: true,
  };
  assert.equal(
    teamReadinessScopeKey(base),
    teamReadinessScopeKey({
      ...base,
      projectRef: "project",
      checkoutPath: "/repo",
      selectedRoles: ["builder", "lead"],
    }),
  );
  assert.notEqual(
    teamReadinessScopeKey(base),
    teamReadinessScopeKey({ ...base, checkoutPath: "/other" }),
  );
  assert.notEqual(
    teamReadinessScopeKey(base),
    teamReadinessScopeKey({ ...base, selectedRoles: ["lead"] }),
  );
  assert.notEqual(
    teamReadinessScopeKey(base),
    teamReadinessScopeKey({ ...base, hiringPolicyEnabled: false }),
  );
  assert.notEqual(
    teamReadinessScopeKey(base),
    teamReadinessScopeKey({ ...base, communityId: "community-b" }),
  );
  assert.notEqual(
    teamReadinessScopeKey(base),
    teamReadinessScopeKey({ ...base, relayUrl: "wss://relay-b.example" }),
  );
  assert.notEqual(
    teamReadinessScopeKey(base),
    teamReadinessScopeKey({ ...base, channelIds: ["channel-b"] }),
  );
});

test("zero live providers blocks even an inconsistent ready response", () => {
  const response = readiness({
    readyForFirstSession: true,
    status: "ready",
    provider: {
      relayUrl: "wss://hive.example",
      provisioned: false,
      process: "not_supervised",
    },
  });
  const gate = teamReadinessLaunchGate({
    projectRef: response.projectRef,
    loading: false,
    error: null,
    readiness: response,
  });
  assert.equal(gate.allowed, false);
  assert.match(gate.reason, /no live provider/);
});

test("standalone team creation is not hijacked by project readiness", () => {
  assert.deepEqual(
    teamReadinessLaunchGate({
      projectRef: null,
      loading: false,
      error: "backend unavailable",
      readiness: null,
    }),
    { allowed: true, reason: null },
  );
});

test("Prepare invokes every named mutation in order and finishes with a fresh read", async () => {
  const calls = [];
  const finalReadiness = readiness({ readyForFirstSession: true });
  const result = await prepareProjectForTeams({
    dependencies: {
      installRoles: async () => calls.push("install"),
      provisionProvider: async () => calls.push("provision"),
      startProvider: async () => calls.push("start"),
      rereadReadiness: async () => {
        calls.push("reread");
        return finalReadiness;
      },
    },
  });
  assert.deepEqual(calls, ["install", "provision", "start", "reread"]);
  assert.deepEqual(
    result.steps.map(({ id, state }) => ({ id, state })),
    [
      { id: "install_roles", state: "done" },
      { id: "provision_provider", state: "done" },
      { id: "start_provider", state: "done" },
      { id: "reread_readiness", state: "done" },
    ],
  );
  assert.equal(result.readiness, finalReadiness);
  assert.equal(result.error, null);
});

test("Prepare preserves completed steps after partial failure and still re-reads", async () => {
  const calls = [];
  const result = await prepareProjectForTeams({
    dependencies: {
      installRoles: async () => calls.push("install"),
      provisionProvider: async () => {
        calls.push("provision");
        throw new Error("macOS denied key access");
      },
      startProvider: async () => calls.push("start"),
      rereadReadiness: async () => {
        calls.push("reread");
        return readiness();
      },
    },
  });
  assert.deepEqual(calls, ["install", "provision", "reread"]);
  assert.deepEqual(
    result.steps.map(({ id, state }) => ({ id, state })),
    [
      { id: "install_roles", state: "done" },
      { id: "provision_provider", state: "failed" },
      { id: "start_provider", state: "pending" },
      { id: "reread_readiness", state: "done" },
    ],
  );
  assert.equal(result.error, "macOS denied key access");
});

test("two full Prepare runs repeat named idempotent mutations and re-read without drift", async () => {
  const calls = [];
  const prepared = readiness({
    readyForFirstSession: true,
    status: "awaiting_first_session",
    hostClass: "prepared_for_first_session",
    provider: {
      relayUrl: "wss://hive.example",
      provisioned: true,
      process: "live",
    },
    awaitingCodes: ["CATALOG_AWAITING_FIRST_SESSION"],
    facts: [
      {
        category: "catalog",
        code: "CATALOG_AWAITING_FIRST_SESSION",
        scope: "wire",
        state: "awaiting_first_session",
        summary: "No signed session catalog exists yet.",
      },
    ],
  });
  const dependencies = {
    installRoles: async () => calls.push("install"),
    provisionProvider: async () => calls.push("provision"),
    startProvider: async () => calls.push("start"),
    rereadReadiness: async () => {
      calls.push("reread");
      return structuredClone(prepared);
    },
  };

  const first = await prepareProjectForTeams({ dependencies });
  const second = await prepareProjectForTeams({ dependencies });

  assert.deepEqual(calls, [
    "install",
    "provision",
    "start",
    "reread",
    "install",
    "provision",
    "start",
    "reread",
  ]);
  assert.deepEqual(second, first);
  assert.deepEqual(
    second.steps.map(({ id, state }) => ({ id, state })),
    [
      { id: "install_roles", state: "done" },
      { id: "provision_provider", state: "done" },
      { id: "start_provider", state: "done" },
      { id: "reread_readiness", state: "done" },
    ],
  );
});
