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

test("provisioning announces the trust mutation after the backend seeded it", async () => {
  // Provisioning appends the first `allowed-bridge-pubkeys` entry from Rust
  // (`session_provider/trust.rs`). The infinity-stale config query only
  // refetches when this callback fires, so a missing or early notification
  // leaves the trusted ingress armed against the empty pre-provision trust
  // set — the first-run "waiting for the provider forever" race.
  const order = [];

  await loadOrProvisionCodingSessionProvider({
    getStatus: async () => ({ provisioned: false, running: false }),
    provision: async () => {
      order.push("provision");
      return provisioned;
    },
    onTrustMutated: () => {
      order.push("trust-mutated");
    },
  });

  // Exactly once, and only after provision resolved — refetching before the
  // backend wrote the entry would just re-cache the empty list.
  assert.deepEqual(order, ["provision", "trust-mutated"]);
});

test("a reused provider does not announce a trust mutation", async () => {
  let trustMutations = 0;

  await loadOrProvisionCodingSessionProvider({
    getStatus: async () => provisioned,
    provision: async () => provisioned,
    onTrustMutated: () => {
      trustMutations += 1;
    },
  });

  assert.equal(trustMutations, 0);
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

test("every new create draft mints a fresh canonical umbrella sessionRef", async () => {
  const { buildNewCodingSessionCreateInput } = await import(
    "./useNewCodingSessionCreate.ts"
  );
  const { isCodingSessionSessionRef } = await import(
    "../lib/codingSessionWireDecode.ts"
  );
  const base = {
    channelId: "channel-1",
    commandId: "csc-1",
    providerInstanceRef: "claude-primary",
    providerAuthorityPubkey: "a".repeat(64),
    model: null,
    title: "Advance Buzz live sessions",
    initialTurn: null,
  };
  const first = buildNewCodingSessionCreateInput(base);
  const second = buildNewCodingSessionCreateInput(base);
  assert.equal(isCodingSessionSessionRef(first.sessionRef), true);
  assert.equal(isCodingSessionSessionRef(second.sessionRef), true);
  assert.notEqual(first.sessionRef, second.sessionRef);
  // The rest of the durable-create input is byte-for-byte the historical set.
  assert.deepEqual(
    { ...first, sessionRef: undefined },
    {
      ...base,
      projectRef: null,
      repoRef: null,
      sessionRef: undefined,
    },
  );
  // A future "add a provider" entry point reuses an umbrella's existing ref.
  const joined = buildNewCodingSessionCreateInput({
    ...base,
    sessionRef: first.sessionRef,
  });
  assert.equal(joined.sessionRef, first.sessionRef);
});
