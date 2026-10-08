import { createHash } from "node:crypto";

import { expect, test, type Locator, type Page } from "@playwright/test";
import {
  finalizeEvent,
  generateSecretKey,
  getPublicKey,
} from "nostr-tools/pure";

import { buildCodingSessionTargetKey } from "@/features/coding-sessions/lib/codingSessionCommand";
import {
  BUZZ_CODING_SESSION_METADATA_SCHEMA,
  codingSessionMetadataSemanticKey,
  CODING_SESSION_METADATA_TAG_VERSION,
} from "@/features/coding-sessions/lib/codingSessionIngressPayloads";
import {
  CODING_SESSION_PROVIDER_CATALOG_SCHEMA,
  CODING_SESSION_PROVIDER_CATALOG_TAG_VERSION,
  codingSessionProviderCatalogSemanticKey,
} from "@/features/coding-sessions/lib/codingSessionProviderCatalog";
import {
  BUZZ_CODING_SESSION_TRANSCRIPT_SCHEMA,
  codingSessionTranscriptSemanticKey,
  CODING_SESSION_TRANSCRIPT_TAG_VERSION,
} from "@/features/coding-sessions/lib/codingSessionTranscriptPresentation";
import {
  KIND_CODING_SESSION_METADATA,
  KIND_CODING_SESSION_PROVIDER_CATALOG,
  KIND_CODING_SESSION_TRANSCRIPT,
} from "@/shared/constants/kinds";
import type { RelayEvent } from "@/shared/api/types";
import { waitForAnimations } from "../helpers/animations";
import { installMockBridge } from "../helpers/bridge";
import { shotPath } from "../helpers/shotPath";

// SV-35 (C3a lane D): the composer's model and traits chips switch a running
// execution's model at the next turn, and stay display-only — saying why —
// where the execution's metadata carries no `modelSwitch`. Four shots, each
// scoped to its subject and gated on distinct hashes.

const SHOTS = "test-results/coding-session-sv35";
const secret = generateSecretKey();
const pubkey = getPublicKey(secret);
const channelName = "engineering";
const channelId = "1c7e1c02-87bb-5e88-b2da-5a7a9432d0c9";
const providerInstanceRef = "claude";
const session = {
  driver: "claude-agent-acp",
  instanceId: "a1b2c3d4e5f60718",
  sessionId: "35353535-eeee-ffff-0000-111111111111",
  generation: 1,
};
const targetKey = buildCodingSessionTargetKey(session);

function signed(
  kind: number,
  createdOffset: number,
  content: string,
  tags: string[][],
): RelayEvent {
  return finalizeEvent(
    {
      kind,
      created_at: 1_800_500_000 + createdOffset,
      tags,
      content,
    },
    secret,
  ) as unknown as RelayEvent;
}

function metadata(createdOffset: number, modelSwitch: boolean): RelayEvent {
  return signed(
    KIND_CODING_SESSION_METADATA,
    createdOffset,
    JSON.stringify({
      schema: BUZZ_CODING_SESSION_METADATA_SCHEMA,
      session,
      projectRef: null,
      repoRef: null,
      title: "Switch models mid-session",
      agentRef: null,
      provider: providerInstanceRef,
      runtime: "claude",
      model: "sonnet[high]",
      status: "idle",
      branch: null,
      capabilities: {
        threadTurnStart: true,
        threadTurnInterrupt: true,
        threadSteer: false,
        context: false,
        diff: false,
        plan: false,
        promptImage: false,
        ...(modelSwitch ? { modelSwitch: true } : {}),
      },
    }),
    [
      ["h", channelId],
      ["csm-v", CODING_SESSION_METADATA_TAG_VERSION],
      ["cs-target", targetKey],
      ["csm-key", codingSessionMetadataSemanticKey(session)],
    ],
  );
}

function catalog(): RelayEvent {
  const content = JSON.stringify({
    schema: CODING_SESSION_PROVIDER_CATALOG_SCHEMA,
    revision: 1,
    providers: [
      {
        providerInstanceRef,
        driver: session.driver,
        runtime: "claude",
        defaultModel: "sonnet",
        allowedModels: ["sonnet", "opus"],
        capabilities: {
          threadTurnStart: true,
          threadTurnInterrupt: true,
          threadSteer: false,
          context: false,
          diff: false,
          plan: false,
          promptImage: false,
        },
        models: [
          {
            id: "sonnet",
            name: "Sonnet 5",
            efforts: ["low", "medium", "high"],
            rank: 1,
          },
          {
            id: "opus",
            name: "Opus 5.5",
            efforts: ["low", "medium", "high", "xhigh"],
            rank: 0,
          },
        ],
      },
    ],
  });
  return signed(KIND_CODING_SESSION_PROVIDER_CATALOG, 0, content, [
    ["h", channelId],
    ["cspc-v", CODING_SESSION_PROVIDER_CATALOG_TAG_VERSION],
    ["cspc-revision", "1"],
    [
      "cspc-key",
      codingSessionProviderCatalogSemanticKey(channelId, 1, content),
    ],
  ]);
}

function transcript(seq: number, item: unknown): RelayEvent {
  return signed(
    KIND_CODING_SESSION_TRANSCRIPT,
    seq,
    JSON.stringify({
      schema: BUZZ_CODING_SESSION_TRANSCRIPT_SCHEMA,
      session,
      eventSeq: seq,
      timestamp: 1_800_500_000_000 + seq * 1_000,
      turnId: "sv35-turn",
      item,
    }),
    [
      ["h", channelId],
      ["cst-v", CODING_SESSION_TRANSCRIPT_TAG_VERSION],
      ["cs-target", targetKey],
      ["cst-seq", String(seq)],
      ["cst-key", codingSessionTranscriptSemanticKey(session, seq)],
    ],
  );
}

async function seed(page: Page, events: RelayEvent[]): Promise<void> {
  await page.evaluate(
    ({ channelName: name, events: signedEvents }) => {
      const hook = window.__BEEKEEPER_E2E_SEED_MOCK_SIGNED_EVENT__;
      if (!hook) throw new Error("signed-event seeding hook is missing");
      for (const event of signedEvents) hook({ channelName: name, event });
    },
    { channelName, events },
  );
}

async function openSession(page: Page): Promise<Locator> {
  await page.setViewportSize({ width: 1440, height: 900 });
  await installMockBridge(page, {
    globalAgentConfig: {
      env_vars: {},
      provider: null,
      model: null,
      "allowed-bridge-pubkeys": [{ pubkey, label: "SV-35 provider" }],
    },
  });
  await page.goto("/");
  await page.getByTestId(`channel-${channelName}`).click();
  await seed(page, [
    catalog(),
    metadata(1, true),
    transcript(2, { kind: "user_prompt", content: "Summarise the plan." }),
    transcript(3, { kind: "assistant_text", text: "The plan has two steps." }),
    transcript(4, {
      kind: "result",
      subtype: "success",
      isError: false,
      durationMs: 4_000,
      result: "Done.",
    }),
  ]);
  const trigger = page.getByTestId("channel-coding-sessions-trigger");
  await expect(trigger).toHaveAttribute("aria-label", "Coding sessions (1)", {
    timeout: 15_000,
  });
  await trigger.click();
  await page.getByTestId("channel-coding-session-open").click();
  const workspace = page.getByTestId("coding-session-workspace");
  await expect(workspace).toContainText("Switch models mid-session");
  return workspace;
}

test("SV-35 model and traits chips: open, pending, display-only", async ({
  page,
}, testInfo) => {
  test.setTimeout(90_000);
  const hashes = new Map<string, string>();
  const shoot = async (name: string, locator: Locator) => {
    await expect(locator).toBeVisible();
    await waitForAnimations(page);
    const png = await locator.screenshot({
      path: shotPath(testInfo, SHOTS, name),
    });
    hashes.set(name, createHash("sha256").update(png).digest("hex"));
  };

  const workspace = await openSession(page);
  const deck = workspace.getByTestId("coding-session-control-deck");
  const modelChip = deck.getByTestId("coding-session-control-identity");
  await expect(modelChip).toContainText("Sonnet 5");

  // model-chip-open: the provider instance's own models, current one checked.
  await modelChip.click();
  const modelPopover = page.getByTestId("coding-session-model-chip-popover");
  await expect(
    modelPopover.getByTestId("coding-session-model-option-opus"),
  ).toBeVisible();
  await expect(modelPopover).not.toContainText("fixed for this execution");
  await shoot("model-chip-open", modelPopover);
  await page.keyboard.press("Escape");

  // traits-chip-open: the efforts the selected model's row names.
  await deck.getByTestId("coding-session-control-traits").click();
  const traitsPopover = page.getByTestId("coding-session-traits-chip-popover");
  await expect(
    traitsPopover.getByTestId("coding-session-effort-option-high"),
  ).toHaveAttribute("aria-pressed", "true");
  await expect(traitsPopover).toContainText("no turn reports the effort");
  await shoot("traits-chip-open", traitsPopover);
  await page.keyboard.press("Escape");

  // switch-pending: choosing Opus signs one thread.model.set and says it
  // waits for the next turn; the chip keeps metadata's model meanwhile.
  await modelChip.click();
  await page
    .getByTestId("coding-session-model-chip-popover")
    .getByTestId("coding-session-model-option-opus")
    .click();
  await page.keyboard.press("Escape");
  const note = deck.getByTestId("coding-session-model-switch-note");
  await expect(note).toHaveText(
    "Switching to Opus 5.5 · High at the next turn",
  );
  await expect(modelChip).toContainText("Sonnet 5");
  const published = await page.evaluate(() =>
    (window.__BEEKEEPER_E2E_SIGNED_EVENTS__ ?? [])
      .filter((event) => event.kind === 44220)
      .map((event) => JSON.parse(event.content).action),
  );
  expect(published).toContainEqual({
    type: "thread.model.set",
    selection: "opus[high]",
  });
  await page.mouse.move(2, 2);
  await shoot("switch-pending", deck);

  // chip-display-only: a newer 44223 for the same generation without
  // `modelSwitch` turns the chips back into the display-only identity chip.
  await seed(page, [metadata(10, false)]);
  await expect(
    deck.getByTestId("coding-session-model-switch-note"),
  ).toHaveCount(0);
  await deck.getByTestId("coding-session-control-identity").click();
  const disclosure = page.getByTestId("coding-session-model-switch-disclosure");
  await expect(disclosure).toContainText(
    "This execution's provider cannot change models mid-session.",
  );
  await shoot(
    "chip-display-only",
    disclosure.locator(
      "xpath=ancestor::*[@data-radix-popper-content-wrapper][1]",
    ),
  );

  const seen = new Map<string, string>();
  for (const [name, hash] of hashes) {
    expect(
      seen.get(hash),
      `${name} is byte-identical to ${seen.get(hash)}`,
    ).toBeUndefined();
    seen.set(hash, name);
  }
  expect(hashes.size).toBe(4);
});
