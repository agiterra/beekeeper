import assert from "node:assert/strict";
import { test } from "node:test";

import {
  groupTeamReadinessFacts,
  normalizeTeamReadinessRoles,
  teamReadinessLaunchGate,
} from "./teamReadinessModel.ts";
import { selectInitialNewCodingSessionTarget } from "./newCodingSessionModel.ts";
import { teamReadinessScopeKey } from "./useProjectTeamReadiness.ts";
import {
  prepareProjectForTeams,
  startSelectedInstalledRoleIdentities,
} from "./teamReadinessPrepare.ts";

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

test("a launch that does not use roles is not gated by project readiness", () => {
  // A session led by the person, with no seats, runs on none of the things
  // readiness checks: no checkout to record, no provider to supervise, no
  // packs to install. The card is informational then, and Start is open.
  const response = readiness({
    status: "blocked",
    blockingCodes: ["CHECKOUT_NOT_RECORDED"],
    facts: [
      {
        category: "checkout",
        code: "CHECKOUT_NOT_RECORDED",
        scope: "local",
        state: "blocked",
        summary: "No checkout is recorded for this project",
        remedy: "Choose a checkout for this project.",
      },
    ],
  });
  const input = {
    projectRef: response.projectRef,
    loading: false,
    error: null,
    readiness: response,
  };
  assert.equal(teamReadinessLaunchGate(input).allowed, false);
  assert.deepEqual(teamReadinessLaunchGate({ ...input, useRoles: false }), {
    allowed: true,
    reason: null,
  });
  // Unknown readiness is not a gate either when roles are off.
  assert.equal(
    teamReadinessLaunchGate({ ...input, readiness: null, useRoles: false })
      .allowed,
    true,
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
      startRoles: async () => calls.push("start roles"),
      provisionProvider: async () => calls.push("provision"),
      startProvider: async () => calls.push("start"),
      refreshRuntime: async () => calls.push("refresh runtime"),
      rereadReadiness: async () => {
        calls.push("reread");
        return finalReadiness;
      },
    },
  });
  assert.deepEqual(calls, [
    "install",
    "start roles",
    "provision",
    "start",
    "refresh runtime",
    "reread",
  ]);
  assert.deepEqual(
    result.steps.map(({ id, state }) => ({ id, state })),
    [
      { id: "install_roles", state: "done" },
      { id: "start_roles", state: "done" },
      { id: "provision_provider", state: "done" },
      { id: "start_provider", state: "done" },
      { id: "refresh_runtime", state: "done" },
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
      startRoles: async () => calls.push("start roles"),
      provisionProvider: async () => {
        calls.push("provision");
        throw new Error("macOS denied key access");
      },
      startProvider: async () => calls.push("start"),
      refreshRuntime: async () => calls.push("refresh runtime"),
      rereadReadiness: async () => {
        calls.push("reread");
        return readiness();
      },
    },
  });
  assert.deepEqual(calls, ["install", "start roles", "provision", "reread"]);
  assert.deepEqual(
    result.steps.map(({ id, state }) => ({ id, state })),
    [
      { id: "install_roles", state: "done" },
      { id: "start_roles", state: "done" },
      { id: "provision_provider", state: "failed" },
      { id: "start_provider", state: "pending" },
      { id: "refresh_runtime", state: "pending" },
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
    startRoles: async () => calls.push("start roles"),
    provisionProvider: async () => calls.push("provision"),
    startProvider: async () => calls.push("start"),
    refreshRuntime: async () => calls.push("refresh runtime"),
    rereadReadiness: async () => {
      calls.push("reread");
      return structuredClone(prepared);
    },
  };

  const first = await prepareProjectForTeams({ dependencies });
  const second = await prepareProjectForTeams({ dependencies });

  assert.deepEqual(calls, [
    "install",
    "start roles",
    "provision",
    "start",
    "refresh runtime",
    "reread",
    "install",
    "start roles",
    "provision",
    "start",
    "refresh runtime",
    "reread",
  ]);
  assert.deepEqual(second, first);
  assert.deepEqual(
    second.steps.map(({ id, state }) => ({ id, state })),
    [
      { id: "install_roles", state: "done" },
      { id: "start_roles", state: "done" },
      { id: "provision_provider", state: "done" },
      { id: "start_provider", state: "done" },
      { id: "refresh_runtime", state: "done" },
      { id: "reread_readiness", state: "done" },
    ],
  );
});

test("Prepare starts the selected installed identity before readiness and the real picker can launch", async () => {
  const leadPubkey = "c".repeat(64);
  const installed = [
    {
      role: "lead",
      agentPubkey: leadPubkey,
      personaId: "lead",
      personaName: "lead",
      agentName: "Helios",
      packDir: "/repo/roles/lead",
      refreshed: false,
      renamed: false,
      seated: true,
    },
  ];
  const live = new Set();
  const runtimeTarget = selectInitialNewCodingSessionTarget([
    {
      selectionKey: "target",
      channelId: "channel",
      signerPubkey: "d".repeat(64),
      provider: {
        providerInstanceRef: "codex-primary",
        runtime: "codex",
        defaultModel: "gpt-5.6-sol",
        allowedModels: ["gpt-5.6-sol"],
        capabilities: { threadTurnStart: true },
      },
      availability: { state: "ready", label: "Codex", hint: null },
      isLocalProvider: true,
    },
  ]);
  const result = await prepareProjectForTeams({
    dependencies: {
      installRoles: async () => {},
      startRoles: () =>
        startSelectedInstalledRoleIdentities({
          installed,
          selectedRoles: ["lead"],
          start: async (pubkey) => void live.add(pubkey),
        }),
      provisionProvider: async () => {},
      startProvider: async () => {},
      refreshRuntime: async () => {},
      rereadReadiness: async () =>
        readiness({
          readyForFirstSession: live.has(leadPubkey),
          status: "awaiting_first_session",
          hostClass: "prepared_for_first_session",
          provider: {
            relayUrl: "wss://hive.example",
            provisioned: true,
            process: "live",
          },
          awaitingCodes: ["CATALOG_AWAITING_FIRST_SESSION"],
        }),
    },
  });
  assert.equal(live.has(leadPubkey), true);
  assert.equal(
    teamReadinessLaunchGate({
      projectRef: result.readiness.projectRef,
      loading: false,
      error: null,
      readiness: result.readiness,
      runtimeTarget,
    }).allowed,
    true,
  );
});

test("readiness cannot allow launch without the actual picker target", () => {
  const prepared = readiness({
    readyForFirstSession: true,
    status: "awaiting_first_session",
    provider: {
      relayUrl: "wss://hive.example",
      provisioned: true,
      process: "live",
    },
    awaitingCodes: ["CATALOG_AWAITING_FIRST_SESSION"],
  });
  const gate = teamReadinessLaunchGate({
    projectRef: prepared.projectRef,
    loading: false,
    error: null,
    readiness: prepared,
    runtimeTarget: selectInitialNewCodingSessionTarget([]),
  });
  assert.equal(gate.allowed, false);
  assert.match(gate.reason, /installed and authenticated/);
});

test("selected role start fails closed when install omitted the identity", async () => {
  await assert.rejects(
    startSelectedInstalledRoleIdentities({
      installed: [],
      selectedRoles: ["lead"],
      start: async () => assert.fail("must not start an unresolved identity"),
    }),
    /could not resolve one installed managed identity/,
  );
});
