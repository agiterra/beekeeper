import assert from "node:assert/strict";
import { test } from "node:test";

import {
  groupTeamReadinessFacts,
  normalizeTeamReadinessRoles,
  teamReadinessBlockerCopy,
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

test("an unanswering agent host blocks launch in two clean sentences", () => {
  // 2026-10-05: the gate read "The agent host could not answer for the
  // provider the agent host is not responding (no answer in 1000ms)" — the
  // summary has no period and the host's words start lower-case.
  const response = readiness({
    status: "blocked",
    blockingCodes: ["PROVIDER_HOST_UNREACHABLE"],
    facts: [
      {
        category: "provider",
        code: "PROVIDER_HOST_UNREACHABLE",
        scope: "local",
        state: "blocked",
        summary: "The agent host could not answer for the provider",
        remedy:
          "the agent host did not answer within 5s — the computer may be busy",
      },
    ],
  });
  const gate = teamReadinessLaunchGate({
    projectRef: response.projectRef,
    loading: false,
    error: null,
    readiness: response,
  });
  assert.equal(gate.allowed, false, "blocked, never passable");
  assert.equal(
    gate.reason,
    "The agent host could not answer for the provider. The agent host did not answer within 5s — the computer may be busy.",
  );
});

test("a launch that does not use roles is still refused CHECKOUT_NOT_RECORDED", () => {
  // The first RPG Test team session (2026-09-19) started with "Use roles"
  // off, which used to open the gate entirely; the lead ran in another
  // project's checkout and could not hire (HIRE_CHECKOUT_NOT_RECORDED).
  // Roles off bypasses the role and pack facts, never the checkout.
  const checkoutFact = {
    category: "checkout",
    code: "CHECKOUT_NOT_RECORDED",
    scope: "local",
    state: "blocked",
    summary: "No checkout is recorded for this project.",
    remedy: "Finish repository setup, or choose a checkout for this project.",
  };
  const response = readiness({
    status: "blocked",
    blockingCodes: ["CHECKOUT_NOT_RECORDED"],
    facts: [checkoutFact],
  });
  const input = {
    projectRef: response.projectRef,
    loading: false,
    error: null,
    readiness: response,
  };
  assert.equal(teamReadinessLaunchGate(input).allowed, false);
  assert.deepEqual(teamReadinessLaunchGate({ ...input, useRoles: false }), {
    allowed: false,
    reason: `${checkoutFact.summary} ${checkoutFact.remedy}`,
  });
  // Readiness that cannot vouch for the folder is not an open gate either:
  // loading, errored and absent all fail closed, naming the checkout.
  assert.equal(
    teamReadinessLaunchGate({ ...input, readiness: null, useRoles: false })
      .allowed,
    false,
  );
  assert.match(
    teamReadinessLaunchGate({ ...input, loading: true, useRoles: false })
      .reason,
    /where this project's code lives/,
  );
  assert.match(
    teamReadinessLaunchGate({ ...input, error: "boom", useRoles: false })
      .reason,
    /could not confirm where this project's code lives: boom/,
  );
});

test("a launch that does not use roles bypasses every fact but the checkout", () => {
  // A session led by the person, with no seats, needs no prepared roles:
  // the role and pack facts are informational then, and Start is open —
  // provided this computer knows where the project's code lives.
  const response = readiness({
    status: "blocked",
    blockingCodes: ["ROLE_PACKS_MISSING"],
    facts: [
      {
        category: "checkout",
        code: "CHECKOUT_RECORDED",
        scope: "local",
        state: "ready",
        summary: "Checkout recorded at /Users/x/Code/rpg-test.",
        remedy: null,
      },
      {
        category: "roles",
        code: "ROLE_PACKS_MISSING",
        scope: "local",
        state: "blocked",
        summary: "No role packs.",
        remedy: "Restore personas/roles.",
      },
      {
        category: "provider",
        code: "PROVIDER_NOT_RUNNING",
        scope: "local",
        state: "unknown",
        summary: "No provider.",
        remedy: null,
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
  // No project, no gate — unchanged.
  assert.deepEqual(
    teamReadinessLaunchGate({ ...input, projectRef: null, useRoles: false }),
    { allowed: true, reason: null },
  );
});

// Ledger 140: this test used to loop `["unknown", "blocked"]` and assert
// both always block regardless of `readyForFirstSession`. That was true for
// "blocked" but wrong for "unknown" once a real Unknown fact carries a
// `scope`: `CATALOG_TARGETS_UNCOVERED` is a `wire` fact about *full*
// readiness (a signed provider catalog does not exist until a first session
// publishes one), and ledger 137 left it blocking the very first session it
// cannot yet exist for. The split is now explicit — a `local`-scope Unknown
// (this computer's own inventory) still blocks; a `wire`-scope Unknown
// (something only the relay can answer) warns instead. Legacy payloads that
// report `status: "unknown"`/a code in `unknownCodes` with no matching fact
// still fail closed, which is the "unknown" half of the original loop with
// no facts attached — it is retained via the top-level `status` case below.
test("Blocked status cannot be overruled by readyForFirstSession", () => {
  const response = readiness({
    readyForFirstSession: true,
    status: "blocked",
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
  );
});

test("an unknownCodes entry with no matching fact still fails closed", () => {
  // A malformed or legacy payload that names a code in `unknownCodes` but
  // carries no fact for it cannot be scoped local vs. wire, so the gate
  // refuses to guess it was only ever a wire one.
  const response = readiness({
    readyForFirstSession: true,
    status: "unknown",
    provider: {
      relayUrl: "wss://hive.example",
      provisioned: true,
      process: "live",
    },
    unknownCodes: ["SOME_UNTRANSLATED_CODE"],
  });
  assert.equal(
    teamReadinessLaunchGate({
      projectRef: response.projectRef,
      loading: false,
      error: null,
      readiness: response,
    }).allowed,
    false,
  );
});

test("a wire-scope Unknown catalog fact warns but does not block the first session (ledger 137, 140)", () => {
  const catalogUncovered = {
    category: "catalog",
    code: "CATALOG_TARGETS_UNCOVERED",
    scope: "wire",
    state: "unknown",
    summary:
      "No signed provider target covers this project's ready local registry.",
    remedy: null,
  };
  const response = readiness({
    readyForFirstSession: true,
    status: "unknown",
    provider: {
      relayUrl: "wss://hive.example",
      provisioned: true,
      process: "live",
    },
    facts: [catalogUncovered],
    unknownCodes: ["CATALOG_TARGETS_UNCOVERED"],
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
});

test("a local-scope Unknown fact still blocks the first session even when the status also carries an optional wire Unknown", () => {
  const localUnknown = {
    category: "identity",
    code: "SELECTED_ROLE_KEY_UNVERIFIED",
    scope: "local",
    state: "unknown",
    summary: "A role's signing key has not been checked on this computer.",
    remedy: "Use Prepare below to start that identity.",
  };
  const catalogUncovered = {
    category: "catalog",
    code: "CATALOG_TARGETS_UNCOVERED",
    scope: "wire",
    state: "unknown",
    summary:
      "No signed provider target covers this project's ready local registry.",
    remedy: null,
  };
  const response = readiness({
    readyForFirstSession: false,
    status: "unknown",
    provider: {
      relayUrl: "wss://hive.example",
      provisioned: true,
      process: "live",
    },
    facts: [localUnknown, catalogUncovered],
    unknownCodes: ["SELECTED_ROLE_KEY_UNVERIFIED", "CATALOG_TARGETS_UNCOVERED"],
  });
  const gate = teamReadinessLaunchGate({
    projectRef: response.projectRef,
    loading: false,
    error: null,
    readiness: response,
  });
  assert.equal(gate.allowed, false);
  assert.match(gate.reason, /signing key has not been checked/);
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

/**
 * The project-action delegation fact (ledger 186, finding 178(f)). It is about
 * what the *lead* will be allowed to do once the session is running, so it
 * must warn rather than gate: a founder who is not a project owner can still
 * run a perfectly good session, and blocking Start would make the remedy
 * ("a project owner signs the delegation") impossible to reach from here.
 */
test("the project-actions codes carry plain-language copy naming the remedy", () => {
  const notDelegable = teamReadinessBlockerCopy(
    "PROJECT_ACTIONS_NOT_DELEGABLE",
  );
  assert.ok(notDelegable, "PROJECT_ACTIONS_NOT_DELEGABLE has no copy");
  assert.match(notDelegable.action, /project owner/);
  assert.equal(notDelegable.title.endsWith("."), false);
  const unknown = teamReadinessBlockerCopy("PROJECT_ACTIONS_AUTHORITY_UNKNOWN");
  assert.ok(unknown, "PROJECT_ACTIONS_AUTHORITY_UNKNOWN has no copy");
  assert.match(unknown.action, /project owner/);
  assert.match(unknown.action, /not blocked/);
});

test("a wire-scope Unknown about project-action authority warns and does not gate Start", () => {
  const response = readiness({
    readyForFirstSession: true,
    status: "unknown",
    provider: {
      relayUrl: "wss://hive.example",
      provisioned: true,
      process: "live",
    },
    facts: [
      {
        category: "actions",
        code: "PROJECT_ACTIONS_AUTHORITY_UNKNOWN",
        scope: "wire",
        state: "unknown",
        summary: "The relay could not answer who owns this project.",
        remedy: null,
      },
    ],
    unknownCodes: ["PROJECT_ACTIONS_AUTHORITY_UNKNOWN"],
  });
  assert.deepEqual(
    teamReadinessLaunchGate({
      projectRef: response.projectRef,
      loading: false,
      error: null,
      readiness: response,
    }),
    { allowed: true, reason: null },
  );
});

test("a Limited project-actions fact does not gate Start either", () => {
  const response = readiness({
    readyForFirstSession: true,
    status: "limited",
    provider: {
      relayUrl: "wss://hive.example",
      provisioned: true,
      process: "live",
    },
    facts: [
      {
        category: "actions",
        code: "PROJECT_ACTIONS_NOT_DELEGABLE",
        scope: "wire",
        state: "limited",
        summary:
          "This session's lead will not be able to publish or trigger this project's actions.",
        remedy: "A project owner signs the delegation for the session's lead.",
      },
    ],
    limitedCodes: ["PROJECT_ACTIONS_NOT_DELEGABLE"],
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
});
