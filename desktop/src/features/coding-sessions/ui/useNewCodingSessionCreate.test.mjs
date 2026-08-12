import assert from "node:assert/strict";
import test from "node:test";

import {
  loadCodingSessionProviderRuntimes,
  loadOrProvisionCodingSessionProvider,
} from "./useNewCodingSessionCreate.ts";

const provisioned = {
  provisioned: true,
  running: true,
  providerPubkey: "a".repeat(64),
  instanceId: "a".repeat(16),
};

test("an existing coding-session provider is reused", async () => {
  let provisionCalls = 0;
  let provisioningNotices = 0;

  const status = await loadOrProvisionCodingSessionProvider({
    getStatus: async () => provisioned,
    onProvisioning: () => {
      provisioningNotices += 1;
    },
    provision: async () => {
      provisionCalls += 1;
      return provisioned;
    },
  });

  assert.equal(status, provisioned);
  assert.equal(provisionCalls, 0);
  assert.equal(provisioningNotices, 0);
});

test("a fresh create screen provisions this computer before target selection", async () => {
  let provisionCalls = 0;
  let provisioningNotices = 0;

  const status = await loadOrProvisionCodingSessionProvider({
    getStatus: async () => ({ provisioned: false, running: false }),
    onProvisioning: () => {
      provisioningNotices += 1;
    },
    provision: async () => {
      provisionCalls += 1;
      return provisioned;
    },
  });

  assert.equal(status, provisioned);
  assert.equal(provisionCalls, 1);
  assert.equal(provisioningNotices, 1);
});

function readyRuntime(overrides = {}) {
  return {
    instanceRef: "claude-primary",
    runtime: "claude",
    driver: "claude-agent-acp",
    label: "Claude Code",
    authState: "ready",
    defaultModel: "default",
    allowedModels: ["default"],
    capabilities: {
      threadTurnStart: true,
      threadTurnInterrupt: true,
      threadSteer: false,
      context: false,
      diff: false,
      plan: true,
    },
    ...overrides,
  };
}

test("runtimes load with per-runtime models, skipping non-ready runtimes", async () => {
  const runtimes = [
    readyRuntime(),
    readyRuntime({
      instanceRef: "codex-primary",
      runtime: "codex",
      driver: "codex-acp",
      label: "Codex",
      authState: "needs_auth",
    }),
    readyRuntime({
      instanceRef: "goose-primary",
      runtime: "goose",
      driver: "goose-acp",
      label: "Goose",
    }),
  ];
  let listed = null;
  const models = new Map();
  const probed = [];

  await loadCodingSessionProviderRuntimes({
    getRuntimes: async () => runtimes,
    getModels: async (instanceRef) => {
      probed.push(instanceRef);
      return {
        instanceRef,
        defaultModel: instanceRef === "claude-primary" ? "sonnet" : "default",
        allowedModels:
          instanceRef === "claude-primary" ? ["sonnet", "opus"] : ["default"],
      };
    },
    onRuntimes: (loaded) => {
      listed = loaded;
    },
    onModels: (instanceRef, loaded) => {
      models.set(instanceRef, loaded);
    },
  });

  assert.deepEqual(listed, runtimes);
  // Models are probed only for ready runtimes — a signed-out codex must not
  // trigger its adapter.
  assert.deepEqual(probed.sort(), ["claude-primary", "goose-primary"]);
  assert.deepEqual(models.get("claude-primary"), {
    defaultModel: "sonnet",
    allowedModels: ["sonnet", "opus"],
  });
  assert.equal(models.has("codex-primary"), false);
});

test("an older backend without the runtimes command degrades to claude-only", async () => {
  let listed = null;
  const models = new Map();

  await loadCodingSessionProviderRuntimes({
    getRuntimes: async () => {
      throw new Error("unknown command coding_session_provider_runtimes");
    },
    getModels: async (instanceRef) => ({
      instanceRef,
      defaultModel: "default",
      allowedModels: ["default", "haiku"],
    }),
    onRuntimes: (loaded) => {
      listed = loaded;
    },
    onModels: (instanceRef, loaded) => {
      models.set(instanceRef, loaded);
    },
  });

  assert.equal(listed.length, 1);
  assert.equal(listed[0].instanceRef, "claude-primary");
  assert.equal(listed[0].authState, "ready");
  assert.deepEqual(models.get("claude-primary"), {
    defaultModel: "default",
    allowedModels: ["default", "haiku"],
  });
});

test("one broken adapter's model probe does not cost the others theirs", async () => {
  const models = new Map();

  await loadCodingSessionProviderRuntimes({
    getRuntimes: async () => [
      readyRuntime(),
      readyRuntime({
        instanceRef: "codex-primary",
        runtime: "codex",
        driver: "codex-acp",
        label: "Codex",
      }),
    ],
    getModels: async (instanceRef) => {
      if (instanceRef === "claude-primary") {
        throw new Error("adapter spawn failed");
      }
      return {
        instanceRef,
        defaultModel: "default",
        allowedModels: ["default"],
      };
    },
    onRuntimes: () => {},
    onModels: (instanceRef, loaded) => {
      models.set(instanceRef, loaded);
    },
  });

  assert.equal(models.has("claude-primary"), false);
  assert.deepEqual(models.get("codex-primary"), {
    defaultModel: "default",
    allowedModels: ["default"],
  });
});
