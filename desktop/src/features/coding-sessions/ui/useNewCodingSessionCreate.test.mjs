import assert from "node:assert/strict";
import test from "node:test";

import {
  CODING_SESSION_CREATE_UNNAMED_MODEL_DISCLOSURE,
  clearAbandonedCodingSessionCreate,
  codingSessionCreateModelDisclosure,
  loadCodingSessionProviderRuntimes,
  loadOrProvisionCodingSessionProvider,
  resolveCodingSessionCreateModel,
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

test("founding publishes genesis and name before preparing the linked 10-key create", async () => {
  const { prepareNewCodingSessionCreate } = await import(
    "./useNewCodingSessionCreate.ts"
  );
  const order = [];
  const genesisRef = "c".repeat(64);
  let preparedInput = null;
  const result = await prepareNewCodingSessionCreate(
    "channel-1",
    {
      channelId: "channel-1",
      commandId: "csc-1",
      providerInstanceRef: "claude-primary",
      providerAuthorityPubkey: "a".repeat(64),
      model: null,
      title: "Advance Buzz live sessions",
      initialTurn: null,
    },
    {
      publishGenesis: async (input) => {
        order.push("genesis");
        assert.match(input.sessionRef, /^[0-9a-f-]{36}$/);
        return { eventId: genesisRef, kind: 44226 };
      },
      publishName: async (input) => {
        order.push("name");
        assert.equal(input.content, "Advance Buzz live sessions");
        assert.equal(input.sessionRef.length, 36);
        return { id: "f".repeat(64), kind: 44229 };
      },
      prepareCreate: async (_scopeId, input) => {
        order.push("create");
        preparedInput = input;
        return { ok: false, errorMessage: "test sentinel" };
      },
    },
  );
  assert.deepEqual(order, ["genesis", "name", "create"]);
  assert.equal(preparedInput.genesisRef, genesisRef);
  assert.equal(preparedInput.sessionRef.length, 36);
  assert.deepEqual(result, { ok: false, errorMessage: "test sentinel" });
});

test("joining reuses authority without publishing a second genesis", async () => {
  const { prepareNewCodingSessionCreate } = await import(
    "./useNewCodingSessionCreate.ts"
  );
  const sessionRef = "5b7e1c2a-90d4-4b0e-a1f3-7c2d8e6f4a10";
  const genesisRef = "d".repeat(64);
  let genesisPublishes = 0;
  let preparedInput = null;
  await prepareNewCodingSessionCreate(
    "channel-1",
    {
      channelId: "channel-1",
      commandId: "csc-join",
      providerInstanceRef: "codex-primary",
      providerAuthorityPubkey: "a".repeat(64),
      model: null,
      title: null,
      initialTurn: null,
      sessionRef,
      genesisRef,
    },
    {
      publishGenesis: async () => {
        genesisPublishes += 1;
        return { eventId: "e".repeat(64), kind: 44226 };
      },
      publishName: async () => {
        throw new Error("joining must not publish a session name");
      },
      prepareCreate: async (_scopeId, input) => {
        preparedInput = input;
        return { ok: false, errorMessage: "test sentinel" };
      },
    },
  );
  assert.equal(genesisPublishes, 0);
  assert.equal(preparedInput.sessionRef, sessionRef);
  assert.equal(preparedInput.genesisRef, genesisRef);
});

/**
 * "Start fresh" abandons a wedged create. It already drops the working-directory
 * hint the provider would have read; the seat's key material is the same kind of
 * host-local fact under the same `commandId`, and leaving it behind is an `nsec`
 * at rest that nothing will ever consume.
 */
test("abandoning a seated create drops its key material with its workdir hint", async () => {
  const calls = [];
  await clearAbandonedCodingSessionCreate({
    commandId: "csl-9",
    actor: "aa".repeat(32),
    clearHint: async (commandId) => {
      calls.push(["clearHint", commandId]);
    },
    clearSeat: async (commandId) => {
      calls.push(["clearSeat", commandId]);
    },
  });
  assert.deepEqual(calls, [
    ["clearHint", "csl-9"],
    ["clearSeat", "csl-9"],
  ]);
});

test("abandoning an unseated create touches no custody at all", async () => {
  const calls = [];
  await clearAbandonedCodingSessionCreate({
    commandId: "csl-10",
    actor: null,
    clearHint: async (commandId) => {
      calls.push(["clearHint", commandId]);
    },
    clearSeat: async (commandId) => {
      calls.push(["clearSeat", commandId]);
    },
  });
  assert.deepEqual(calls, [["clearHint", "csl-10"]]);
});

test("a custody clear that fails does not stop the screen from resetting", async () => {
  await clearAbandonedCodingSessionCreate({
    commandId: "csl-11",
    actor: "aa".repeat(32),
    clearHint: async () => {
      throw new Error("hint file is read-only");
    },
    clearSeat: async () => {
      throw new Error("seat file is read-only");
    },
  });
});

// --- item 88(b): the One-session create wrote the picker's label ------------

test("the picker's `default` label is resolved to the catalog's own model id", () => {
  // Live 2026-08-28: the lead's create carried model "default" — the label the
  // picker shows when nothing is chosen, not the model the runtime ran.
  assert.equal(
    resolveCodingSessionCreateModel({
      model: "default",
      catalog: {
        defaultModel: "claude-fable-5[1m]",
        allowedModels: ["default", "claude-fable-5[1m]", "sonnet"],
      },
    }),
    "claude-fable-5[1m]",
  );
});

test("a model the person actually chose is written byte for byte", () => {
  assert.equal(
    resolveCodingSessionCreateModel({
      model: "sonnet",
      catalog: {
        defaultModel: "claude-fable-5[1m]",
        allowedModels: ["default", "sonnet"],
      },
    }),
    "sonnet",
  );
  assert.equal(resolveCodingSessionCreateModel({ model: null }), null);
  assert.equal(resolveCodingSessionCreateModel({ model: "   " }), null);
});

test("a catalog whose own default is `default` writes `default` and says so", () => {
  // The Claude adapter really does publish an id called `default`, so there is
  // nothing concrete to resolve to. The record then cannot name the model —
  // and the dialog has to admit that rather than imply it did.
  const catalog = {
    defaultModel: "default",
    allowedModels: ["default", "sonnet"],
  };
  assert.equal(
    resolveCodingSessionCreateModel({ model: "default", catalog }),
    "default",
  );
  assert.equal(
    codingSessionCreateModelDisclosure({ model: "default", catalog }),
    CODING_SESSION_CREATE_UNNAMED_MODEL_DISCLOSURE,
  );
  assert.equal(
    CODING_SESSION_CREATE_UNNAMED_MODEL_DISCLOSURE,
    "Runs the runtime's default model — the record will not name it.",
  );
});

test("a resolved model needs no disclosure, and an unread catalog still discloses", () => {
  assert.equal(
    codingSessionCreateModelDisclosure({
      model: "default",
      catalog: {
        defaultModel: "claude-fable-5[1m]",
        allowedModels: ["default", "claude-fable-5[1m]"],
      },
    }),
    null,
  );
  assert.equal(codingSessionCreateModelDisclosure({ model: "sonnet" }), null);
  // No catalog read: `default` is still all the record will carry.
  assert.equal(
    codingSessionCreateModelDisclosure({ model: "default" }),
    CODING_SESSION_CREATE_UNNAMED_MODEL_DISCLOSURE,
  );
});
