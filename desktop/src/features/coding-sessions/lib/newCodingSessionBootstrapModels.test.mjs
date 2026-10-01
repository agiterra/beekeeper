import assert from "node:assert/strict";
import test from "node:test";

import { codingSessionProviderPickerModels } from "./codingSessionModelOptions.ts";
import { resolveNewCodingSessionTargets } from "./newCodingSessionModel.ts";

const OWN = "a".repeat(64);
const FOREIGN = "b".repeat(64);

const CAPABILITIES = {
  threadTurnStart: true,
  threadTurnInterrupt: true,
  threadSteer: false,
  context: false,
  diff: false,
  plan: true,
};

function claudeRuntime() {
  return {
    instanceRef: "claude-primary",
    runtime: "claude",
    driver: "claude-agent-acp",
    label: "Claude Code",
    authState: "ready",
    defaultModel: "default",
    allowedModels: ["default"],
    capabilities: CAPABILITIES,
  };
}

/** What the provider publishes for Claude, as the 2026-10-01 catalog did. */
function publishedClaude(overrides = {}) {
  return {
    providerInstanceRef: "claude-primary",
    driver: "claude-agent-acp",
    runtime: "claude",
    defaultModel: "default",
    allowedModels: ["default", "claude-fable-5-1[1m]", "opus"],
    capabilities: CAPABILITIES,
    models: [
      { id: "default", name: "Default (recommended)", rank: 0 },
      { id: "claude-fable-5-1[1m]", name: "Fable 5.1", rank: 2 },
      {
        id: "opus",
        name: "Opus 5.5",
        efforts: ["low", "medium", "high"],
        fastMode: true,
        rank: 1,
      },
    ],
    ...overrides,
  };
}

function entry(channelId, signerPubkey, createdAt, provider) {
  return {
    channelId,
    signerPubkey,
    eventId: "e".repeat(64),
    createdAt,
    catalog: {
      schema: "buzz-coding-session-provider-catalog/v1",
      revision: 1,
      providers: [provider],
    },
  };
}

test("a channel the provider has not joined borrows its own catalog from another", () => {
  const targets = resolveNewCodingSessionTargets({
    catalogs: [
      entry("channel-elsewhere", OWN, 1_800_000_000, publishedClaude()),
    ],
    channelId: "channel-new",
    localProvider: {
      providerPubkey: OWN,
      runtimes: [claudeRuntime()],
      // The models command's bare ids — what the picker used to show.
      modelsByInstanceRef: new Map([
        [
          "claude-primary",
          {
            defaultModel: "default",
            allowedModels: ["default", "claude-fable-5-1", "opus"],
          },
        ],
      ]),
    },
  });

  assert.equal(targets.length, 1);
  const [target] = targets;
  // Still a target in the channel being created in, still this host's.
  assert.equal(target.channelId, "channel-new");
  assert.equal(target.isLocalProvider, true);
  assert.deepEqual(target.provider.allowedModels, [
    "default",
    "claude-fable-5-1[1m]",
    "opus",
  ]);
  const picker = codingSessionProviderPickerModels(target.provider);
  assert.deepEqual(picker.models, ["default", "opus", "claude-fable-5-1"]);
  assert.equal(picker.details.get("opus").name, "Opus 5.5");
  assert.equal(picker.details.get("claude-fable-5-1").name, "Fable 5.1");
  assert.deepEqual(target.provider.models[2].efforts, [
    "low",
    "medium",
    "high",
  ]);
});

test("the newest of the provider's own catalogs wins, and nobody else's is borrowed", () => {
  const older = publishedClaude({
    defaultModel: "opus",
    allowedModels: ["opus"],
    models: [{ id: "opus", name: "Opus (old)" }],
  });
  const foreign = publishedClaude({
    defaultModel: "opus",
    allowedModels: ["opus"],
    models: [{ id: "opus", name: "Someone else's Opus" }],
  });
  const targets = resolveNewCodingSessionTargets({
    catalogs: [
      entry("channel-b", OWN, 1_800_000_100, publishedClaude()),
      entry("channel-a", OWN, 1_800_000_000, older),
      entry("channel-c", FOREIGN, 1_900_000_000, foreign),
    ],
    channelId: "channel-new",
    localProvider: { providerPubkey: OWN, runtimes: [claudeRuntime()] },
  });

  const own = targets.find((target) => target.signerPubkey === OWN);
  assert.equal(
    own.provider.models.find((m) => m.id === "opus").name,
    "Opus 5.5",
  );
});

test("with no catalog of its own anywhere, the bootstrap target is unchanged", () => {
  const [target] = resolveNewCodingSessionTargets({
    catalogs: [],
    channelId: "channel-new",
    localProvider: { providerPubkey: OWN, runtimes: [claudeRuntime()] },
  });

  assert.deepEqual(target.provider.allowedModels, ["default"]);
  assert.equal(Object.hasOwn(target.provider, "models"), false);
});
