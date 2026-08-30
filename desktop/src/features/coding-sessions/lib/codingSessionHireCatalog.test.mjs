import assert from "node:assert/strict";
import test from "node:test";

import { resolveCodingSessionHireCatalogSource } from "./codingSessionHireCatalog.ts";

const CHANNEL = "d87fb22f-0000-4000-8000-000000000000";
const SIGNER = "ab".repeat(32);

function entry({
  channelId = CHANNEL,
  signerPubkey = SIGNER,
  eventId = "11".repeat(32),
  revision = 7,
  projects,
} = {}) {
  return {
    channelId,
    signerPubkey,
    eventId,
    createdAt: 1_800_000_000,
    catalog: {
      schema: "buzz-coding-session-provider-catalog/v1",
      revision,
      providers: [
        {
          providerInstanceRef: "claude-primary",
          driver: "claude-agent-acp",
          runtime: "claude",
          defaultModel: "sonnet",
          allowedModels: ["sonnet", "opus[1m]"],
          capabilities: {
            threadTurnStart: true,
            threadTurnInterrupt: true,
            threadSteer: false,
            context: false,
            diff: false,
            plan: true,
          },
        },
      ],
      ...(projects === undefined ? {} : { projects }),
    },
  };
}

test("a hire cites the revision and models from the same signed channel catalog", () => {
  const source = resolveCodingSessionHireCatalogSource({
    entries: [
      entry(),
      entry({
        channelId: "another-channel",
        eventId: "22".repeat(32),
        revision: 99,
      }),
      entry({
        signerPubkey: "cd".repeat(32),
        eventId: "33".repeat(32),
        revision: 41,
      }),
    ],
    channelId: CHANNEL,
    signerPubkey: SIGNER.toUpperCase(),
    projectRef: null,
  });

  assert.ok(source);
  assert.equal(source.catalogRevision, 7);
  assert.equal(source.eventId, "11".repeat(32));
  assert.deepEqual(source.modelCatalogs.get("claude-primary"), [
    "sonnet",
    "opus[1m]",
  ]);
});

test("project narrowing and missing signed catalogs are refusals, never fallbacks", () => {
  const scoped = entry({
    projects: [
      {
        projectRef: "30621:owner:beekeeper",
        repoRef: null,
        providers: ["claude-primary"],
      },
    ],
  });
  const wrongProject = resolveCodingSessionHireCatalogSource({
    entries: [scoped],
    channelId: CHANNEL,
    signerPubkey: SIGNER,
    projectRef: "30621:owner:another",
  });
  assert.ok(wrongProject);
  assert.equal(wrongProject.modelCatalogs.size, 0);

  assert.equal(
    resolveCodingSessionHireCatalogSource({
      entries: [scoped],
      channelId: CHANNEL,
      signerPubkey: "ef".repeat(32),
      projectRef: "30621:owner:beekeeper",
    }),
    null,
  );
});

test("two source events cannot be blended into one claimed revision", () => {
  assert.equal(
    resolveCodingSessionHireCatalogSource({
      entries: [entry(), entry({ eventId: "44".repeat(32), revision: 8 })],
      channelId: CHANNEL,
      signerPubkey: SIGNER,
      projectRef: null,
    }),
    null,
  );
});
