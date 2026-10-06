import { expect, test, type Page } from "@playwright/test";
import {
  finalizeEvent,
  generateSecretKey,
  getPublicKey,
} from "nostr-tools/pure";

import { buildCodingSessionTargetKey } from "@/features/coding-sessions/lib/codingSessionCommand";
import {
  BUZZ_CODING_SESSION_METADATA_SCHEMA,
  CODING_SESSION_METADATA_TAG_VERSION,
  codingSessionMetadataSemanticKey,
  lifecycleReceiptSemanticKey,
} from "@/features/coding-sessions/lib/codingSessionIngressPayloads";
import { buildCodingSessionCreateEvent } from "@/features/coding-sessions/lib/codingSessionLifecycleCommand";
import type { RelayEvent } from "@/shared/api/types";
import {
  KIND_CODING_SESSION_LEASE,
  KIND_CODING_SESSION_LIFECYCLE_RECEIPT,
  KIND_CODING_SESSION_METADATA,
} from "@/shared/constants/kinds";

import { installMockBridge } from "../helpers/bridge";

/**
 * §2 item 41 — the header that read IDLE over a provider that had quit.
 *
 * Brian's prod app exited at 21:17 without publishing `disconnected`. For two
 * hours the session header showed IDLE and the composer offered Send and Stop;
 * nothing could answer either. The signed 44223 metadata was not wrong — it
 * said what the provider last said — it was being rendered as the current
 * condition.
 *
 * Both cases below seed the *same* signed session, two hours stale, and differ
 * in exactly one fact: whether an unexpired kind-24223 lease exists. Nothing
 * about the path under test is stubbed; the fixture signs real facts and seeds
 * the bytes.
 */

const PROVIDER_SECRET = generateSecretKey();
const PROVIDER_PUBKEY = getPublicKey(PROVIDER_SECRET);
const OPERATOR_SECRET = generateSecretKey();

/** `general` in the mock channel fixture; the `h` tag must match exactly. */
const CHANNEL_ID = "9a1657ac-f7aa-5db0-b632-d8bbeb6dfb50";
const CHANNEL_NAME = "general";
const SESSION_REF = "3f0a5c9e-2b71-4d88-9a6f-5c1e0b7d4a23";
const COMMAND_ID = "csl-3f0a5c9e-2b71-4d88-9a6f-5c1e0b7d4a23";
const TARGET = {
  driver: "claude-agent-acp",
  instanceId: "reachability-instance",
  sessionId: "6d2b8f14-9c30-4a57-b8e1-0f7a2c5d6e94",
  generation: 1,
};
/** The gap Brian actually sat through. */
const REPORTED_SECONDS_AGO = 7_200;

function nowSeconds(): number {
  return Math.floor(Date.now() / 1_000);
}

function authorityEvents(): RelayEvent[] {
  const command = buildCodingSessionCreateEvent({
    channelId: CHANNEL_ID,
    commandId: COMMAND_ID,
    projectRef: null,
    repoRef: null,
    sessionRef: SESSION_REF,
    providerInstanceRef: TARGET.instanceId,
    providerAuthorityPubkey: PROVIDER_PUBKEY,
    model: "sonnet",
    title: "Fix the reconnect bug",
    initialTurn: null,
  });
  return [
    finalizeEvent(
      {
        kind: command.kind,
        created_at: nowSeconds() - REPORTED_SECONDS_AGO - 60,
        tags: command.tags,
        content: command.content,
      },
      OPERATOR_SECRET,
    ) as unknown as RelayEvent,
    finalizeEvent(
      {
        kind: KIND_CODING_SESSION_LIFECYCLE_RECEIPT,
        created_at: nowSeconds() - REPORTED_SECONDS_AGO - 50,
        tags: [
          ["h", CHANNEL_ID],
          ["cslr-v", "cslr1-1"],
          ["csl-command", COMMAND_ID],
          ["csl-key", lifecycleReceiptSemanticKey(COMMAND_ID)],
        ],
        content: JSON.stringify({
          schema: "buzz-coding-session-lifecycle-receipt/v1",
          commandId: COMMAND_ID,
          status: "created",
          session: TARGET,
          error: null,
        }),
      },
      PROVIDER_SECRET,
    ) as unknown as RelayEvent,
  ];
}

/** The provider's last word: idle, two hours ago. */
function metadataEvent(): RelayEvent {
  return finalizeEvent(
    {
      kind: KIND_CODING_SESSION_METADATA,
      created_at: nowSeconds() - REPORTED_SECONDS_AGO,
      tags: [
        ["h", CHANNEL_ID],
        ["csm-v", CODING_SESSION_METADATA_TAG_VERSION],
        ["cs-target", buildCodingSessionTargetKey(TARGET)],
        ["csm-key", codingSessionMetadataSemanticKey(TARGET)],
      ],
      content: JSON.stringify({
        schema: BUZZ_CODING_SESSION_METADATA_SCHEMA,
        session: TARGET,
        projectRef: null,
        repoRef: null,
        title: "Fix the reconnect bug",
        agentRef: null,
        provider: "claude-agent-acp",
        runtime: "claude-agent-acp",
        model: "sonnet",
        status: "idle",
        branch: "fix/reconnect",
        capabilities: {
          threadTurnStart: true,
          threadTurnInterrupt: true,
          threadSteer: true,
          context: false,
          diff: true,
          plan: true,
        },
        sessionRef: SESSION_REF,
      }),
    },
    PROVIDER_SECRET,
  ) as unknown as RelayEvent;
}

/** A current lease: this provider is answering right now. */
function liveLeaseEvent(): RelayEvent {
  return finalizeEvent(
    {
      kind: KIND_CODING_SESSION_LEASE,
      created_at: nowSeconds() - 5,
      tags: [
        ["h", CHANNEL_ID],
        ["cslease-v", "cslease1-1"],
        ["cs-target", buildCodingSessionTargetKey(TARGET)],
        ["csl-command", COMMAND_ID],
        ["cslease-seq", "1"],
      ],
      content: JSON.stringify({
        schema: "buzz-coding-session-lease/v1",
        target: TARGET,
        state: "live",
        leaseSequence: 1,
      }),
    },
    PROVIDER_SECRET,
  ) as unknown as RelayEvent;
}

async function seedAndOpen(page: Page, events: RelayEvent[]) {
  await installMockBridge(page);
  await page.goto("/", { waitUntil: "domcontentloaded" });
  await page.getByTestId(`channel-${CHANNEL_NAME}`).click();
  await page.evaluate(
    ({ channelName, seeds }) => {
      const seed = window.__BEEKEEPER_E2E_SEED_MOCK_SIGNED_EVENT__;
      if (!seed) throw new Error("mock signed-event seam is missing");
      for (const event of seeds as never[]) seed({ channelName, event });
    },
    { channelName: CHANNEL_NAME, seeds: events as never },
  );
  const trigger = page.getByTestId("channel-coding-sessions-trigger");
  await expect(trigger).toHaveAttribute("aria-label", "Coding sessions (1)", {
    timeout: 15_000,
  });
  await trigger.click();
  await page.getByTestId("channel-coding-session-open").first().click();
  await expect(page.getByTestId("coding-session-header")).toBeVisible({
    timeout: 15_000,
  });
}

test("with no live lease the header states the age of the report, not the report", async ({
  page,
}) => {
  await seedAndOpen(page, [...authorityEvents(), metadataEvent()]);

  const badge = page.getByTestId("coding-session-status-badge");
  await expect(badge).toContainText("No provider answering", {
    timeout: 15_000,
  });
  await expect(badge).toContainText("last reported Idle 2h ago");

  // And the composer says why it will not send, instead of a dead button.
  await expect(
    page.getByTestId("coding-session-composer-unreachable"),
  ).toContainText("No provider is answering for this execution");
  await expect(page.getByLabel("Coding-session instruction")).toBeDisabled();
});

test("an unexpired lease leaves the reported status exactly as it was", async ({
  page,
}) => {
  await seedAndOpen(page, [
    ...authorityEvents(),
    metadataEvent(),
    liveLeaseEvent(),
  ]);

  const badge = page.getByTestId("coding-session-status-badge");
  await expect(badge).toContainText("Idle", { timeout: 15_000 });
  await expect(badge).not.toContainText("No provider answering");
  await expect(
    page.getByTestId("coding-session-composer-unreachable"),
  ).toHaveCount(0);
});
