import { expect, test } from "@playwright/test";
import {
  finalizeEvent,
  generateSecretKey,
  getPublicKey,
} from "nostr-tools/pure";

import { buildCodingSessionTargetKey } from "@/features/coding-sessions/lib/codingSessionCommand";
import { buildCodingSessionGenesisEvent } from "@/features/coding-sessions/lib/codingSessionGenesis";
import { buildCodingSessionCreateEvent } from "@/features/coding-sessions/lib/codingSessionLifecycleCommand";
import {
  codingSessionMetadataSemanticKey,
  CODING_SESSION_METADATA_TAG_VERSION,
  BUZZ_CODING_SESSION_METADATA_SCHEMA,
  CODING_SESSION_LIFECYCLE_RECEIPT_SCHEMA,
  CODING_SESSION_LIFECYCLE_RECEIPT_TAG_VERSION,
  lifecycleReceiptSemanticKey,
} from "@/features/coding-sessions/lib/codingSessionIngressPayloads";
import {
  BUZZ_CODING_SESSION_TRANSCRIPT_SCHEMA,
  codingSessionTranscriptSemanticKey,
  CODING_SESSION_TRANSCRIPT_TAG_VERSION,
} from "@/features/coding-sessions/lib/codingSessionTranscriptPresentation";
import {
  KIND_CODING_SESSION_LIFECYCLE_RECEIPT,
  KIND_CODING_SESSION_METADATA,
  KIND_CODING_SESSION_TRANSCRIPT,
} from "@/shared/constants/kinds";
import type { RelayEvent } from "@/shared/api/types";
import { installMockBridge } from "../helpers/bridge";
import { waitForAnimations } from "../helpers/animations";

/**
 * The consumer's whole trust story is signature-first: an event that is not
 * signed by a configured authority never reaches the catalog, let alone the
 * screen. So this spec signs real 442xx events with a real key, declares that
 * key as the ingress authority through the mocked global agent config, and
 * seeds the signed bytes through the mock relay. Nothing about the path under
 * test is stubbed — only the relay carrying it.
 */

const PROVIDER_SECRET = generateSecretKey();
const PROVIDER_PUBKEY = getPublicKey(PROVIDER_SECRET);
const FOUNDER_SECRET = generateSecretKey();
const FOUNDER_PUBKEY = getPublicKey(FOUNDER_SECRET);
const CHANNEL_NAME = "engineering";
/** `engineering` in the mock channel fixture. The `h` tag must match exactly. */
const CHANNEL_ID = "1c7e1c02-87bb-5e88-b2da-5a7a9432d0c9";

const TARGET = {
  driver: "claude-agent-acp",
  instanceId: "0123456789abcdef",
  sessionId: "11111111-2222-3333-4444-555555555555",
  generation: 1,
};

const TARGET_KEY = buildCodingSessionTargetKey(TARGET);
const BASE_CREATED_AT = 1_800_000_000;
const BASE_TIMESTAMP_MS = 1_800_000_000_000;
const SESSION_REF = "5b7e1c2a-90d4-4b0e-a1f3-7c2d8e6f4a10";
const COMMAND_ID = "csl-founded-session";

function genesisEvent(): RelayEvent {
  const built = buildCodingSessionGenesisEvent({
    channelId: CHANNEL_ID,
    sessionRef: SESSION_REF,
  });
  return finalizeEvent(
    {
      kind: built.kind,
      created_at: BASE_CREATED_AT - 2,
      tags: built.tags,
      content: built.content,
    },
    FOUNDER_SECRET,
  ) as unknown as RelayEvent;
}

function createAndReceiptEvents(genesisRef: string): RelayEvent[] {
  const built = buildCodingSessionCreateEvent({
    channelId: CHANNEL_ID,
    commandId: COMMAND_ID,
    projectRef: null,
    repoRef: null,
    sessionRef: SESSION_REF,
    genesisRef,
    providerInstanceRef: "claude-primary",
    providerAuthorityPubkey: PROVIDER_PUBKEY,
    model: "sonnet",
    title: "Fix the reconnect bug",
    initialTurn: null,
  });
  const create = finalizeEvent(
    {
      kind: built.kind,
      created_at: BASE_CREATED_AT - 1,
      tags: built.tags,
      content: built.content,
    },
    FOUNDER_SECRET,
  ) as unknown as RelayEvent;
  const receipt = finalizeEvent(
    {
      kind: KIND_CODING_SESSION_LIFECYCLE_RECEIPT,
      created_at: BASE_CREATED_AT,
      tags: [
        ["h", CHANNEL_ID],
        ["cslr-v", CODING_SESSION_LIFECYCLE_RECEIPT_TAG_VERSION],
        ["csl-command", COMMAND_ID],
        ["csl-key", lifecycleReceiptSemanticKey(COMMAND_ID)],
      ],
      content: JSON.stringify({
        schema: CODING_SESSION_LIFECYCLE_RECEIPT_SCHEMA,
        commandId: COMMAND_ID,
        status: "created",
        session: TARGET,
        error: null,
      }),
    },
    PROVIDER_SECRET,
  ) as unknown as RelayEvent;
  return [create, receipt];
}

function metadataEvent(): RelayEvent {
  const payload = {
    schema: BUZZ_CODING_SESSION_METADATA_SCHEMA,
    session: TARGET,
    projectRef: null,
    repoRef: null,
    title: "Fix the reconnect bug",
    agentRef: null,
    provider: "claude-agent-acp",
    runtime: "claude-agent-acp",
    model: "sonnet",
    status: "running",
    branch: null,
    capabilities: {
      threadTurnStart: true,
      threadTurnInterrupt: true,
      threadSteer: true,
      context: false,
      diff: false,
      plan: true,
    },
    sessionRef: SESSION_REF,
  };
  return finalizeEvent(
    {
      kind: KIND_CODING_SESSION_METADATA,
      created_at: BASE_CREATED_AT,
      tags: [
        ["h", CHANNEL_ID],
        ["csm-v", CODING_SESSION_METADATA_TAG_VERSION],
        ["cs-target", TARGET_KEY],
        ["csm-key", codingSessionMetadataSemanticKey(TARGET)],
      ],
      content: JSON.stringify(payload),
    },
    PROVIDER_SECRET,
  ) as unknown as RelayEvent;
}

function transcriptEvent(eventSeq: number, item: unknown): RelayEvent {
  const payload = {
    schema: BUZZ_CODING_SESSION_TRANSCRIPT_SCHEMA,
    session: TARGET,
    eventSeq,
    timestamp: BASE_TIMESTAMP_MS + eventSeq * 1_000,
    turnId: "turn-1",
    item,
  };
  return finalizeEvent(
    {
      kind: KIND_CODING_SESSION_TRANSCRIPT,
      created_at: BASE_CREATED_AT + eventSeq,
      tags: [
        ["h", CHANNEL_ID],
        ["cst-v", CODING_SESSION_TRANSCRIPT_TAG_VERSION],
        ["cs-target", TARGET_KEY],
        ["cst-seq", String(eventSeq)],
        ["cst-key", codingSessionTranscriptSemanticKey(TARGET, eventSeq)],
      ],
      content: JSON.stringify(payload),
    },
    PROVIDER_SECRET,
  ) as unknown as RelayEvent;
}

/**
 * One complete turn: a prompt, a tool call and its result, an assistant
 * answer, and the terminal result the completion footer is derived from.
 */
function seededEvents(): RelayEvent[] {
  const genesis = genesisEvent();
  return [
    genesis,
    ...createAndReceiptEvents(genesis.id),
    metadataEvent(),
    transcriptEvent(1, {
      kind: "user_prompt",
      content: "Fix the reconnect bug",
    }),
    transcriptEvent(2, {
      kind: "tool_call",
      tool: {
        toolName: "Bash",
        toolId: "tool-1",
        input: { command: "cargo test" },
      },
    }),
    transcriptEvent(3, {
      kind: "tool_result",
      toolId: "tool-1",
      toolName: "Bash",
      content: "10 passed",
      isError: false,
    }),
    transcriptEvent(4, {
      kind: "assistant_text",
      text: "Reconnect now recovers cleanly.",
    }),
    transcriptEvent(5, {
      kind: "result",
      subtype: "success",
      isError: false,
      durationMs: 3_557,
      result: "Reconnect now recovers cleanly.",
      costUsd: 0.3209,
    }),
  ];
}

async function seedCodingSession(page: import("@playwright/test").Page) {
  await page.evaluate(
    async ({ channelName, events }) => {
      const seed = window.__BUZZ_E2E_SEED_MOCK_SIGNED_EVENT__;
      if (!seed) throw new Error("signed-event seeding hook is missing");
      for (const event of events) {
        seed({ channelName, event });
      }
    },
    { channelName: CHANNEL_NAME, events: seededEvents() },
  );
}

test.beforeEach(async ({ page }) => {
  await installMockBridge(page, {
    globalAgentConfig: {
      env_vars: {},
      provider: null,
      model: null,
      // Without this the consumer is fail-closed and every seeded event is
      // rejected as unauthored — which is the correct default, and exactly
      // what makes seeding it here meaningful.
      "allowed-bridge-pubkeys": [
        { pubkey: PROVIDER_PUBKEY, label: "This computer (coding sessions)" },
      ],
    },
    searchProfiles: [
      {
        pubkey: FOUNDER_PUBKEY,
        displayName: "Alice Rivera",
      },
    ],
  });
  await page.goto("/");
});

test("a seeded signed session is discoverable, opens, and renders its turn", async ({
  page,
}) => {
  await page.getByTestId(`channel-${CHANNEL_NAME}`).click();
  await expect(
    page.getByTestId("channel-coding-sessions-trigger"),
  ).toBeVisible();

  await seedCodingSession(page);

  // The menu counts what the trusted catalog holds, so a non-zero count is
  // itself the assertion that ingress accepted the signed events.
  const trigger = page.getByTestId("channel-coding-sessions-trigger");
  await expect(trigger).toHaveAttribute("aria-label", "Coding sessions (1)", {
    timeout: 15_000,
  });

  await trigger.click();
  const entry = page.getByTestId("channel-coding-session-entry");
  await expect(entry).toHaveCount(1);
  await expect(entry).toContainText("Claude Agent Acp");

  await page.getByTestId("channel-coding-session-open").click();

  const workspace = page.getByTestId("coding-session-workspace");
  await expect(workspace).toBeVisible({ timeout: 15_000 });
  await expect(page.getByTestId("coding-session-header")).toContainText(
    "Fix the reconnect bug",
  );
  const foundedBy = page.getByTestId("coding-session-founded-by");
  await expect(foundedBy).toHaveText("Founded by Alice Rivera");
  await expect(foundedBy).toHaveAttribute("data-genesis-ref", /^[0-9a-f]{64}$/);
  await waitForAnimations(page);
  await page.getByTestId("coding-session-authority-summary").screenshot({
    path: "test-results/screenshots/bite1-founded-by.png",
  });

  const transcript = page.getByTestId("coding-session-transcript");
  await expect(transcript).toBeVisible();
  await expect(page.getByTestId("coding-session-user-message")).toContainText(
    "Fix the reconnect bug",
  );
  await expect(
    page.getByTestId("coding-session-assistant-message"),
  ).toContainText("Reconnect now recovers cleanly.");
  // A settled turn folds its finished work away, so the tool call is present
  // but collapsed until the fold is opened. Asserting both halves keeps the
  // fold honest: the work is there, and it is hidden on purpose.
  const workedFold = page.getByTestId("coding-session-worked-fold");
  await expect(workedFold).toContainText("Worked for 3.6s");
  await expect(page.getByTestId("transcript-tool-item").first()).toBeHidden();
  await workedFold.locator("> summary").click();
  await expect(page.getByTestId("transcript-tool-item").first()).toBeVisible();

  // The duration already lives in the fold's summary, so the footer states
  // the outcome and the cost rather than repeating it.
  const completion = page.getByTestId("coding-session-turn-completion");
  await expect(completion).toHaveAttribute("data-turn-state", "completed");
  await expect(completion).toContainText("Completed");
  await expect(completion).toContainText("$0.32");

  await expect(page.getByTestId("coding-session-composer")).toBeVisible();
});

test("the channel timeline never renders coding-session kinds", async ({
  page,
}) => {
  await page.getByTestId(`channel-${CHANNEL_NAME}`).click();
  await expect(page.getByTestId("message-timeline")).toBeVisible();

  await seedCodingSession(page);
  await expect(
    page.getByTestId("channel-coding-sessions-trigger"),
  ).toHaveAttribute("aria-label", "Coding sessions (1)", { timeout: 15_000 });

  // The 442xx events are in this channel's relay store. They must stay out of
  // the chat timeline entirely — the kinds are deliberately absent from
  // CHANNEL_TIMELINE_CONTENT_KINDS, and this is that law seen from the UI.
  await expect(page.getByTestId("message-timeline")).not.toContainText(
    "Fix the reconnect bug",
  );
  await expect(page.getByTestId("message-timeline")).not.toContainText(
    "buzz-coding-session",
  );
});
