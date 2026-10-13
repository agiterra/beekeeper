import assert from "node:assert/strict";
import test from "node:test";

import {
  validateLinkedAgentRuntimeEdit,
  submitProfilePersonaDialog,
} from "./UserProfilePanelPersonaSubmit.ts";
import { toast } from "sonner";

function agent(overrides = {}) {
  return {
    pubkey: "deadbeef".repeat(8),
    name: "Fizz",
    personaId: "persona-1",
    relayUrl: "ws://localhost:3000",
    acpCommand: "beekeeper-acp",
    agentCommand: "goose",
    agentArgs: [],
    mcpCommand: "",
    turnTimeoutSeconds: 320,
    idleTimeoutSeconds: null,
    maxTurnDurationSeconds: null,
    parallelism: 1,
    systemPrompt: "Prompt",
    avatarUrl: null,
    model: null,
    envVars: {},
    status: "stopped",
    pid: null,
    createdAt: "2026-01-01T00:00:00Z",
    updatedAt: "2026-01-01T00:00:00Z",
    lastStartedAt: null,
    lastStoppedAt: null,
    lastExitCode: null,
    lastError: null,
    logPath: null,
    startOnAppLaunch: true,
    backend: { type: "local" },
    backendAgentId: null,
    respondTo: "owner-only",
    respondToAllowlist: [],
    ...overrides,
  };
}

function persona(overrides = {}) {
  return {
    id: "persona-1",
    displayName: "Fizz",
    avatarUrl: null,
    systemPrompt: "Prompt",
    runtime: "goose",
    model: null,
    provider: null,
    namePool: [],
    isBuiltIn: false,
    isActive: true,
    envVars: {},
    createdAt: "2026-01-01T00:00:00Z",
    updatedAt: "2026-01-01T00:00:00Z",
    ...overrides,
  };
}

function updateInput(overrides = {}) {
  return {
    id: "persona-1",
    displayName: "Fizz",
    avatarUrl: undefined,
    systemPrompt: "Prompt",
    runtime: "claude",
    model: undefined,
    provider: undefined,
    namePool: [],
    ...overrides,
  };
}

function runtime(overrides = {}) {
  return {
    id: "claude",
    label: "Claude Code",
    avatarUrl: "",
    availability: "available",
    command: "claude",
    binaryPath: "/usr/local/bin/claude",
    defaultArgs: [],
    mcpCommand: null,
    installHint: "",
    installInstructionsUrl: "",
    canAutoInstall: false,
    underlyingCliPath: null,
    ...overrides,
  };
}

test("validateLinkedAgentRuntimeEdit allows available runtime changes", () => {
  assert.equal(
    validateLinkedAgentRuntimeEdit({
      input: updateInput({ runtime: "claude" }),
      managedAgent: agent(),
      previousPersona: persona({ runtime: "goose" }),
      runtimes: [runtime()],
    }),
    null,
  );
});

test("linked persona update preserves accepted-but-unconfirmed warning without claiming sync failure", async () => {
  const originalWarning = toast.warning;
  const warnings = [];
  toast.warning = (message) => warnings.push(message);
  try {
    await submitProfilePersonaDialog({
      input: updateInput({ runtime: "goose", displayName: "Updated Mira" }),
      managedAgent: agent(),
      previousPersona: persona(),
      runtimes: [],
      updatePersona: async () => persona({ displayName: "Updated Mira" }),
      updateManagedAgent: async () => ({
        agent: agent({ name: "Updated Mira" }),
        profileSyncError:
          "Configuration accepted; runtime application awaits the next supported execution boundary.",
      }),
      createPersona: async () => {
        throw new Error("unexpected create");
      },
      createManagedAgentForPersona: async () => {
        throw new Error("unexpected create");
      },
      onDone() {},
    });
    assert.equal(warnings.length, 1);
    assert.match(
      warnings[0],
      /Configuration accepted; runtime application awaits/,
    );
    assert.doesNotMatch(warnings[0], /sync failed|saved locally/);
  } finally {
    toast.warning = originalWarning;
  }
});

test("validateLinkedAgentRuntimeEdit rejects unavailable linked-agent runtime changes", () => {
  assert.equal(
    validateLinkedAgentRuntimeEdit({
      input: updateInput({ runtime: "claude" }),
      managedAgent: agent(),
      previousPersona: persona({ runtime: "goose" }),
      runtimes: [runtime({ availability: "cli_missing", command: null })],
    }),
    "Claude Code is not available. Install it before saving this linked agent.",
  );
});

test("validateLinkedAgentRuntimeEdit allows unchanged or unlinked runtime preferences", () => {
  assert.equal(
    validateLinkedAgentRuntimeEdit({
      input: updateInput({ runtime: "goose" }),
      managedAgent: agent(),
      previousPersona: persona({ runtime: "goose" }),
      runtimes: [],
    }),
    null,
  );

  assert.equal(
    validateLinkedAgentRuntimeEdit({
      input: updateInput({ runtime: "claude" }),
      managedAgent: undefined,
      previousPersona: persona({ runtime: "goose" }),
      runtimes: [],
    }),
    null,
  );
});

test("local profile edit stages a single actor update and never saves definition before acceptance", async () => {
  const requests = [];
  let done = 0;
  await submitProfilePersonaDialog({
    input: updateInput({ runtime: "goose", systemPrompt: "new instructions" }),
    managedAgent: agent(),
    previousPersona: persona({ runtime: "goose" }),
    runtimes: [],
    updatePersona: async () => {
      throw new Error("definition must not be saved first");
    },
    updateManagedAgent: async (request) => {
      requests.push(request);
      return { agent: agent(), profileSyncError: null };
    },
    createPersona: async () => {
      throw new Error("unexpected create");
    },
    createManagedAgentForPersona: async () => {
      throw new Error("unexpected mint");
    },
    onDone() {
      done++;
    },
  });
  assert.equal(requests.length, 1);
  assert.equal(
    requests[0].ownedPersonaUpdate.expectedUpdatedAt,
    persona().updatedAt,
  );
  assert.equal(
    requests[0].ownedPersonaUpdate.definition.systemPrompt,
    "new instructions",
  );
  assert.equal(requests[0].pubkey, agent().pubkey);
  assert.equal(done, 1);
});

test("failed or lost actor-update reply leaves local definition untouched and retries same explicit payload", async () => {
  const requests = [];
  let saves = 0;
  let done = 0;
  const options = {
    input: updateInput({ runtime: "goose", systemPrompt: "new instructions" }),
    managedAgent: agent(),
    previousPersona: persona({ runtime: "goose" }),
    runtimes: [],
    updatePersona: async () => {
      saves++;
      return persona();
    },
    updateManagedAgent: async (request) => {
      requests.push(request);
      if (requests.length === 1)
        throw new Error("publication unavailable or reply lost");
      return { agent: agent(), profileSyncError: null };
    },
    createPersona: async () => {
      throw new Error("unexpected create");
    },
    createManagedAgentForPersona: async () => {
      throw new Error("unexpected mint");
    },
    onDone() {
      done++;
    },
  };
  await submitProfilePersonaDialog(options);
  assert.equal(done, 0);
  assert.equal(saves, 0);
  await submitProfilePersonaDialog(options);
  assert.equal(done, 1);
  assert.equal(saves, 0);
  assert.deepEqual(requests[0], requests[1]);
});

test("remote profile edit retains original provider path and sends no local owned edit field", async () => {
  const calls = [];
  await submitProfilePersonaDialog({
    input: updateInput({ runtime: "goose", systemPrompt: "new instructions" }),
    managedAgent: agent({
      backend: { type: "provider", id: "remote", config: {} },
    }),
    previousPersona: persona({ runtime: "goose" }),
    runtimes: [],
    updatePersona: async () => {
      calls.push("definition");
      return persona({ runtime: "goose", systemPrompt: "new instructions" });
    },
    updateManagedAgent: async (request) => {
      calls.push("actor");
      assert.equal(request.ownedPersonaUpdate, undefined);
      return { agent: agent(), profileSyncError: null };
    },
    createPersona: async () => {
      throw new Error("unexpected create");
    },
    createManagedAgentForPersona: async () => {
      throw new Error("unexpected mint");
    },
    onDone() {
      calls.push("done");
    },
  });
  assert.deepEqual(calls, ["definition", "actor", "done"]);
});

for (const policy of ["owner-only", "allowlist"]) {
  test(`unrelated local prompt edit preserves ${policy} instance access over definition default`, async () => {
    const instance = agent({
      respondTo: policy,
      respondToAllowlist: ["a".repeat(64)],
    });
    const requests = [];
    await submitProfilePersonaDialog({
      input: updateInput({
        runtime: "goose",
        systemPrompt: "new instructions",
      }),
      managedAgent: instance,
      previousPersona: persona({ respondTo: "anyone", respondToAllowlist: [] }),
      updateManagedAgent: async (request) => {
        requests.push(request);
        return { agent: instance, profileSyncError: null };
      },
      updatePersona: async () => {
        throw new Error("unexpected definition save");
      },
      onDone() {},
    });
    assert.equal(requests.length, 1);
    assert.equal(requests[0].respondTo, undefined);
    assert.equal(requests[0].respondToAllowlist, undefined);
    assert.equal(requests[0].ownedPersonaUpdate.definition.behavior, undefined);
  });
}

test("an explicit local access edit still updates the exact instance", async () => {
  let request;
  const behavior = {
    respondTo: "allowlist",
    respondToAllowlist: ["a".repeat(64)],
  };
  await submitProfilePersonaDialog({
    input: updateInput({ runtime: "goose", behavior }),
    managedAgent: agent(),
    previousPersona: persona({ respondTo: "anyone", respondToAllowlist: [] }),
    updateManagedAgent: async (input) => {
      request = input;
      return { agent: agent(), profileSyncError: null };
    },
    updatePersona: async () => {
      throw new Error("unexpected definition save");
    },
    onDone() {},
  });
  assert.equal(request.respondTo, "allowlist");
  assert.deepEqual(request.respondToAllowlist, behavior.respondToAllowlist);
  assert.deepEqual(request.ownedPersonaUpdate.definition.behavior, behavior);
});
