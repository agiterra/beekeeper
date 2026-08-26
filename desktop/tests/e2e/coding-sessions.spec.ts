import { expect, test } from "@playwright/test";
import {
  finalizeEvent,
  generateSecretKey,
  getPublicKey,
} from "nostr-tools/pure";

import { buildCodingSessionTargetKey } from "@/features/coding-sessions/lib/codingSessionCommand";
import { buildCodingSessionGenesisEvent } from "@/features/coding-sessions/lib/codingSessionGenesis";
import { buildCodingSessionGoalEvent } from "@/features/coding-sessions/lib/codingSessionGoal";
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
const REVIEW_TARGET = {
  driver: "codex-acp",
  instanceId: "reviewer-instance",
  sessionId: "66666666-7777-8888-9999-000000000000",
  generation: 1,
};
const REVIEW_TARGET_KEY = buildCodingSessionTargetKey(REVIEW_TARGET);
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

function reviewEvents(): RelayEvent[] {
  const metadata = finalizeEvent(
    {
      kind: KIND_CODING_SESSION_METADATA,
      created_at: BASE_CREATED_AT + 20,
      tags: [
        ["h", CHANNEL_ID],
        ["csm-v", CODING_SESSION_METADATA_TAG_VERSION],
        ["cs-target", REVIEW_TARGET_KEY],
        ["csm-key", codingSessionMetadataSemanticKey(REVIEW_TARGET)],
      ],
      content: JSON.stringify({
        schema: BUZZ_CODING_SESSION_METADATA_SCHEMA,
        session: REVIEW_TARGET,
        projectRef: null,
        repoRef: null,
        title: "Fix the reconnect bug",
        agentRef: null,
        provider: "codex-acp",
        runtime: "codex-acp",
        model: "gpt-5.6-sol",
        status: "completed",
        branch: null,
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
  const transcript = finalizeEvent(
    {
      kind: KIND_CODING_SESSION_TRANSCRIPT,
      created_at: BASE_CREATED_AT + 21,
      tags: [
        ["h", CHANNEL_ID],
        ["cst-v", CODING_SESSION_TRANSCRIPT_TAG_VERSION],
        ["cs-target", REVIEW_TARGET_KEY],
        ["cst-seq", "1"],
        ["cst-key", codingSessionTranscriptSemanticKey(REVIEW_TARGET, 1)],
      ],
      content: JSON.stringify({
        schema: BUZZ_CODING_SESSION_TRANSCRIPT_SCHEMA,
        session: REVIEW_TARGET,
        eventSeq: 1,
        timestamp: BASE_TIMESTAMP_MS + 21_000,
        turnId: "review-turn",
        item: {
          kind: "assistant_text",
          text: "The reconnect fix is focused and ready to merge.",
        },
      }),
    },
    PROVIDER_SECRET,
  ) as unknown as RelayEvent;
  return [metadata, transcript];
}

/**
 * A resume: the provider reconnects to its native session and publishes
 * under the SAME `cs-target` minus generation — one execution, generation 2.
 * The gen-1 turn above is then collapsed history, not a second execution.
 */
const RESUMED_TARGET = { ...TARGET, generation: 2 };
const RESUMED_TARGET_KEY = buildCodingSessionTargetKey(RESUMED_TARGET);

function resumedGenerationEvents(): RelayEvent[] {
  const metadata = finalizeEvent(
    {
      kind: KIND_CODING_SESSION_METADATA,
      created_at: BASE_CREATED_AT + 30,
      tags: [
        ["h", CHANNEL_ID],
        ["csm-v", CODING_SESSION_METADATA_TAG_VERSION],
        ["cs-target", RESUMED_TARGET_KEY],
        ["csm-key", codingSessionMetadataSemanticKey(RESUMED_TARGET)],
      ],
      content: JSON.stringify({
        schema: BUZZ_CODING_SESSION_METADATA_SCHEMA,
        session: RESUMED_TARGET,
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
      }),
    },
    PROVIDER_SECRET,
  ) as unknown as RelayEvent;
  const transcript = finalizeEvent(
    {
      kind: KIND_CODING_SESSION_TRANSCRIPT,
      created_at: BASE_CREATED_AT + 31,
      tags: [
        ["h", CHANNEL_ID],
        ["cst-v", CODING_SESSION_TRANSCRIPT_TAG_VERSION],
        ["cs-target", RESUMED_TARGET_KEY],
        ["cst-seq", "1"],
        ["cst-key", codingSessionTranscriptSemanticKey(RESUMED_TARGET, 1)],
      ],
      content: JSON.stringify({
        schema: BUZZ_CODING_SESSION_TRANSCRIPT_SCHEMA,
        session: RESUMED_TARGET,
        eventSeq: 1,
        timestamp: BASE_TIMESTAMP_MS + 31_000,
        turnId: "resumed-turn",
        item: {
          kind: "user_prompt",
          content: "What was the first input I sent this session?",
        },
      }),
    },
    PROVIDER_SECRET,
  ) as unknown as RelayEvent;
  return [metadata, transcript];
}

function goalEvent(): RelayEvent {
  const built = buildCodingSessionGoalEvent({
    channelId: CHANNEL_ID,
    content: "Make authority visible at every decision point",
    sessionRef: SESSION_REF,
  });
  return finalizeEvent(
    {
      kind: built.kind,
      created_at: BASE_CREATED_AT + 10,
      tags: built.tags,
      content: built.content,
    },
    FOUNDER_SECRET,
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
    goalEvent(),
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

function legacyUngovernedEvents(): RelayEvent[] {
  return [
    metadataEvent(),
    transcriptEvent(1, {
      kind: "user_prompt",
      content: "Keep this legacy execution moving",
    }),
    transcriptEvent(2, {
      kind: "assistant_text",
      text: "Legacy execution is ready for another instruction.",
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
  // The row's name is the session's, so the agent is named beside it. This
  // assertion used to read "Claude Agent Acp" — the raw driver id — and had
  // been red on `main` since the row started showing the human label
  // (§2 item 37).
  await expect(entry).toContainText("Claude Code");
  await expect(page.getByTestId("coding-session-goal-catalog")).toContainText(
    "Make authority visible at every decision point",
  );
  await waitForAnimations(page);
  await entry.screenshot({
    path: "test-results/screenshots/bite2-goal-catalog.png",
  });

  await page.getByTestId("channel-coding-session-open").click();

  const workspace = page.getByTestId("coding-session-workspace");
  await expect(workspace).toBeVisible({ timeout: 15_000 });
  await expect(page.getByTestId("coding-session-header")).toContainText(
    "Fix the reconnect bug",
  );
  const foundedBy = page.getByTestId("coding-session-founded-by");
  await expect(foundedBy).toHaveText("Founded by Alice Rivera");
  await expect(foundedBy).toHaveAttribute("data-genesis-ref", /^[0-9a-f]{64}$/);
  await expect(page.getByTestId("coding-session-goal-workspace")).toContainText(
    "Make authority visible at every decision point",
  );
  await waitForAnimations(page);
  await page.getByTestId("coding-session-authority-summary").screenshot({
    path: "test-results/screenshots/bite1-founded-by.png",
  });
  await page.getByTestId("coding-session-goal-workspace").screenshot({
    path: "test-results/screenshots/bite2-goal-workspace.png",
  });

  const transcript = page.getByTestId("coding-session-transcript");
  await expect(transcript).toBeVisible();
  await expect(page.getByTestId("coding-session-user-message")).toContainText(
    "Fix the reconnect bug",
  );
  await expect(
    page.getByTestId("coding-session-assistant-message"),
  ).toContainText("Reconnect now recovers cleanly.");
  // Recent consequential work remains part of the settled turn's readable
  // narrative. Only an older prefix is eligible for progressive disclosure.
  await expect(page.getByTestId("transcript-tool-item").first()).toBeVisible();

  const completion = page.getByTestId("coding-session-turn-completion");
  await expect(completion).toHaveAttribute("data-turn-state", "completed");
  await expect(completion).toContainText("Worked for 3.6s");
  await expect(completion).toContainText("$0.32");

  await expect(page.getByTestId("coding-session-composer")).toBeVisible();
  const gatedComposer = page.getByTestId("coding-session-composer");
  // The gate's copy became the roster-aware hint when grant/revoke landed
  // (`8bb80ff7`); the founder-only sentence is now only the fallback for a
  // composer with no resolved authority reason. Second stale assertion of the
  // same class as §2 item 37 — this spec had been red before it ever got here.
  const instruction = page.getByLabel("Coding-session instruction");
  await expect(instruction).toHaveAttribute(
    "placeholder",
    "View only — ask for collaborator access.",
  );
  await expect(instruction).toBeDisabled();
  await waitForAnimations(page);
  await gatedComposer.screenshot({
    path: "test-results/screenshots/bite3-gated-composer.png",
  });
});

test("a legacy session stays usable through the unified composer", async ({
  page,
}) => {
  await page.getByTestId(`channel-${CHANNEL_NAME}`).click();
  await page.evaluate(
    async ({ channelName, events }) => {
      const seed = window.__BUZZ_E2E_SEED_MOCK_SIGNED_EVENT__;
      if (!seed) throw new Error("signed-event seeding hook is missing");
      for (const event of events) seed({ channelName, event });
    },
    { channelName: CHANNEL_NAME, events: legacyUngovernedEvents() },
  );
  const trigger = page.getByTestId("channel-coding-sessions-trigger");
  await expect(trigger).toHaveAttribute("aria-label", "Coding sessions (1)", {
    timeout: 15_000,
  });
  await trigger.click();
  await page.getByTestId("channel-coding-session-open").click();

  const instruction = page.getByLabel("Coding-session instruction");
  await expect(instruction).toHaveAttribute(
    "placeholder",
    "Steer this coding session…",
  );
  await expect(instruction).toBeEnabled();
  await waitForAnimations(page);
  await page.getByTestId("coding-session-composer").screenshot({
    path: "test-results/screenshots/bite3-ungoverned-session.png",
  });
});

test("a multi-provider session exposes a resizable and collapsible agent rail", async ({
  page,
}) => {
  await page.setViewportSize({ width: 1440, height: 900 });
  await page.getByTestId(`channel-${CHANNEL_NAME}`).click();
  await page.evaluate(
    ({ channelName, events }) => {
      const seed = window.__BUZZ_E2E_SEED_MOCK_SIGNED_EVENT__;
      if (!seed) throw new Error("signed-event seeding hook is missing");
      for (const event of events) seed({ channelName, event });
    },
    {
      channelName: CHANNEL_NAME,
      events: [...seededEvents(), ...reviewEvents()],
    },
  );

  const trigger = page.getByTestId("channel-coding-sessions-trigger");
  // One durable session, however many providers joined it (§2 item 38). The
  // row says how many rather than becoming two rows onto the same umbrella.
  await expect(trigger).toHaveAttribute("aria-label", "Coding sessions (1)", {
    timeout: 15_000,
  });
  await trigger.click();
  await expect(
    page.getByTestId("channel-coding-session-history").first(),
  ).toContainText("2 providers");
  await page.getByTestId("channel-coding-session-open").first().click();

  const workspace = page.getByTestId("coding-session-umbrella-workspace");
  const host = page.getByTestId("coding-session-surface-host");
  await expect(workspace).toBeVisible({ timeout: 15_000 });
  // A shared session opens as a clean narrative; secondary surfaces contract
  // it only when the person asks for one.
  await expect(host).toHaveCount(0);
  await expect(page.getByTestId("coding-session-mention-hint")).toHaveCount(0);
  const recipient = page.getByTestId(
    "coding-session-participant-picker-trigger",
  );
  const initialRecipient = (await recipient.textContent()) ?? "";
  await expect(recipient).toContainText(/Claude|Codex/);
  await recipient.click();
  const executionRecipients = page.getByTestId(
    "coding-session-participant-execution",
  );
  await expect(executionRecipients).toHaveCount(2);
  await expect(executionRecipients.filter({ hasText: "Claude" })).toContainText(
    "Claude Code · Sonnet",
  );
  await expect(executionRecipients.filter({ hasText: "Codex" })).toContainText(
    "Codex · GPT-5.6 Sol",
  );
  // This fixture is deliberately a viewer: targets that imply agent control
  // stay visible but disabled, while the truthful session lane remains usable.
  await expect(executionRecipients.nth(0)).toBeDisabled();
  await expect(executionRecipients.nth(1)).toBeDisabled();
  await page.keyboard.press("Escape");
  await expect(recipient).toHaveText(initialRecipient);
  await waitForAnimations(page);
  await workspace.screenshot({
    path: "test-results/screenshots/session-composer-wide.png",
  });

  await recipient.click();
  await page.getByTestId("coding-session-participant-session").click();
  await expect(recipient).toContainText("Session");

  await page.getByTestId("coding-session-surface-toggle-agents").click();
  await expect(host).toContainText("All agents");
  await expect(host).toContainText("Claude");
  await expect(host).toContainText("Codex");
  await waitForAnimations(page);
  await workspace.screenshot({
    path: "test-results/screenshots/session-agents-open.png",
  });

  const resize = page.getByRole("separator", {
    name: "Resize session surface",
  });
  const before = await host.boundingBox();
  const handle = await resize.boundingBox();
  if (!before || !handle)
    throw new Error("surface host resize geometry missing");
  await page.mouse.move(handle.x + handle.width / 2, handle.y + 120);
  await page.mouse.down();
  await page.mouse.move(handle.x - 120, handle.y + 120);
  await page.mouse.up();
  await expect
    .poll(async () => (await host.boundingBox())?.width ?? 0)
    .toBeGreaterThan(before.width + 80);
  await waitForAnimations(page);
  await workspace.screenshot({
    path: "test-results/screenshots/session-agents-resized.png",
  });

  await page.getByLabel("Close session surface").click();
  await expect(host).toHaveCount(0);
  await waitForAnimations(page);
  await workspace.screenshot({
    path: "test-results/screenshots/session-agents-collapsed.png",
  });
});

test("a resumed session renders every earlier generation, not just the newest", async ({
  page,
}) => {
  await page.getByTestId(`channel-${CHANNEL_NAME}`).click();
  await page.evaluate(
    ({ channelName, events }) => {
      const seed = window.__BUZZ_E2E_SEED_MOCK_SIGNED_EVENT__;
      if (!seed) throw new Error("signed-event seeding hook is missing");
      for (const event of events) seed({ channelName, event });
    },
    {
      channelName: CHANNEL_NAME,
      events: [...seededEvents(), ...resumedGenerationEvents()],
    },
  );

  // A resume is one session with two generations, and the trigger counts
  // sessions (§2 item 38). The generations are disclosed on the row rather
  // than becoming a second row onto the same umbrella; the grouping is proven
  // again below by which surface opens.
  const trigger = page.getByTestId("channel-coding-sessions-trigger");
  await expect(trigger).toHaveAttribute("aria-label", "Coding sessions (1)", {
    timeout: 15_000,
  });
  await trigger.click();
  await expect(page.getByTestId("channel-coding-session-entry")).toHaveCount(1);
  await expect(
    page.getByTestId("channel-coding-session-history"),
  ).toContainText("2 generations");
  await page.getByTestId("channel-coding-session-open").first().click();

  // One execution with collapsed history routes to the umbrella surface —
  // the only view that renders prior generations. The flat tree used to win
  // here and showed generation 2 alone over an empty timeline.
  const workspace = page.getByTestId("coding-session-umbrella-workspace");
  await expect(workspace).toBeVisible({ timeout: 15_000 });
  await page.getByTestId("coding-session-provenance-toggle").click();
  await expect(page.getByRole("dialog")).toContainText(
    "generation 2 · 1 earlier",
  );
  await page.keyboard.press("Escape");

  const timeline = page.getByTestId("coding-session-umbrella-timeline");
  await expect(timeline).toContainText("Fix the reconnect bug");
  await expect(timeline).toContainText("Reconnect now recovers cleanly.");
  await expect(timeline).toContainText(
    "What was the first input I sent this session?",
  );
  const blocks = page.getByTestId("coding-session-umbrella-turn-block");
  await expect(blocks).toHaveCount(2);
  await expect(blocks.first()).toContainText("generation 1");
  await expect(blocks.last()).toContainText("generation 2");
  await waitForAnimations(page);
  await workspace.screenshot({
    path: "test-results/screenshots/session-resumed-history.png",
  });
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
