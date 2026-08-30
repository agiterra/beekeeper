import { expect, test, type Locator, type Page } from "@playwright/test";
import {
  finalizeEvent,
  generateSecretKey,
  getPublicKey,
} from "nostr-tools/pure";

import { buildCodingSessionTargetKey } from "@/features/coding-sessions/lib/codingSessionCommand";
import {
  BUZZ_CODING_SESSION_METADATA_SCHEMA,
  CODING_SESSION_LIFECYCLE_RECEIPT_SCHEMA,
  CODING_SESSION_LIFECYCLE_RECEIPT_TAG_VERSION,
  CODING_SESSION_METADATA_TAG_VERSION,
  codingSessionMetadataSemanticKey,
  codingSessionReceiptSemanticKey,
} from "@/features/coding-sessions/lib/codingSessionIngressPayloads";
import {
  BUZZ_CODING_SESSION_TRANSCRIPT_SCHEMA,
  CODING_SESSION_TRANSCRIPT_TAG_VERSION,
  codingSessionTranscriptSemanticKey,
} from "@/features/coding-sessions/lib/codingSessionTranscriptPresentation";
import type { CodingSessionCommandTarget } from "@/features/coding-sessions/lib/codingSessionCommand";
import type { RelayEvent } from "@/shared/api/types";
import {
  KIND_CODING_SESSION_LIFECYCLE_RECEIPT,
  KIND_CODING_SESSION_METADATA,
  KIND_CODING_SESSION_TRANSCRIPT,
} from "@/shared/constants/kinds";
import { waitForAnimations } from "../helpers/animations";
import { installMockBridge } from "../helpers/bridge";

const CHANNEL_NAME = "engineering";
const CHANNEL_ID = "1c7e1c02-87bb-5e88-b2da-5a7a9432d0c9";
const SESSION_REF = "5b7e1c2a-90d4-4b0e-a1f3-7c2d8e6f4a10";
const BUILDER_SECRET = generateSecretKey();
const BUILDER_PROVIDER = getPublicKey(BUILDER_SECRET);
const VERIFIER_SECRET = generateSecretKey();
const VERIFIER_PROVIDER = getPublicKey(VERIFIER_SECRET);
const BUILDER_ACTOR_SECRET = generateSecretKey();
const BUILDER_ACTOR = getPublicKey(BUILDER_ACTOR_SECRET);
const VERIFIER_ACTOR_SECRET = generateSecretKey();
const VERIFIER_ACTOR = getPublicKey(VERIFIER_ACTOR_SECRET);
const BUILDER_TARGET: CodingSessionCommandTarget = {
  driver: "claude-agent-acp",
  instanceId: "builder-instance",
  sessionId: "11111111-2222-3333-4444-555555555555",
  generation: 1,
};
const VERIFIER_TARGET: CodingSessionCommandTarget = {
  driver: "codex-acp",
  instanceId: "verifier-instance",
  sessionId: "66666666-7777-8888-9999-000000000000",
  generation: 1,
};
const SCREENSHOTS = "test-results/singularity-lens";

function signedMetadata(input: {
  actor: string;
  model: string;
  role: string;
  runtime: string;
  secret: Uint8Array;
  status: "running" | "completed";
  target: CodingSessionCommandTarget;
  title: string;
  createdAt: number;
}): RelayEvent {
  const targetKey = buildCodingSessionTargetKey(input.target);
  return finalizeEvent(
    {
      kind: KIND_CODING_SESSION_METADATA,
      created_at: input.createdAt,
      tags: [
        ["h", CHANNEL_ID],
        ["csm-v", CODING_SESSION_METADATA_TAG_VERSION],
        ["cs-target", targetKey],
        ["csm-key", codingSessionMetadataSemanticKey(input.target)],
      ],
      content: JSON.stringify({
        schema: BUZZ_CODING_SESSION_METADATA_SCHEMA,
        session: input.target,
        projectRef: null,
        repoRef: null,
        title: input.title,
        agentRef: input.actor,
        provider: input.runtime,
        runtime: input.runtime,
        model: input.model,
        status: input.status,
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
        role: input.role,
      }),
    },
    input.secret,
  ) as unknown as RelayEvent;
}

function signedTranscript(input: {
  createdAt: number;
  eventSeq: number;
  item: unknown;
  secret: Uint8Array;
  target: CodingSessionCommandTarget;
  turnId: string;
}): RelayEvent {
  return finalizeEvent(
    {
      kind: KIND_CODING_SESSION_TRANSCRIPT,
      created_at: input.createdAt,
      tags: [
        ["h", CHANNEL_ID],
        ["cst-v", CODING_SESSION_TRANSCRIPT_TAG_VERSION],
        ["cs-target", buildCodingSessionTargetKey(input.target)],
        ["cst-seq", String(input.eventSeq)],
        [
          "cst-key",
          codingSessionTranscriptSemanticKey(input.target, input.eventSeq),
        ],
      ],
      content: JSON.stringify({
        schema: BUZZ_CODING_SESSION_TRANSCRIPT_SCHEMA,
        session: input.target,
        eventSeq: input.eventSeq,
        timestamp: input.createdAt * 1_000,
        turnId: input.turnId,
        item: input.item,
      }),
    },
    input.secret,
  ) as unknown as RelayEvent;
}

function signedTurnStarted(createdAt: number): RelayEvent {
  const commandId = "mission-builder-turn";
  const status = "turn_started";
  return finalizeEvent(
    {
      kind: KIND_CODING_SESSION_LIFECYCLE_RECEIPT,
      created_at: createdAt,
      tags: [
        ["h", CHANNEL_ID],
        ["cslr-v", CODING_SESSION_LIFECYCLE_RECEIPT_TAG_VERSION],
        ["csl-command", commandId],
        ["csl-key", codingSessionReceiptSemanticKey(commandId, status)],
      ],
      content: JSON.stringify({
        schema: CODING_SESSION_LIFECYCLE_RECEIPT_SCHEMA,
        commandId,
        status,
        session: BUILDER_TARGET,
        error: null,
        turnId: "builder-turn",
      }),
    },
    BUILDER_SECRET,
  ) as unknown as RelayEvent;
}

function missionEvents(): RelayEvent[] {
  const now = Math.floor(Date.now() / 1_000);
  return [
    signedMetadata({
      actor: BUILDER_ACTOR,
      model: "sonnet",
      role: "builder",
      runtime: "claude-agent-acp",
      secret: BUILDER_SECRET,
      status: "running",
      target: BUILDER_TARGET,
      title: "Portable team loop",
      createdAt: now - 8,
    }),
    signedTurnStarted(now - 5),
    signedTranscript({
      createdAt: now - 30,
      eventSeq: 1,
      item: { kind: "user_prompt", content: "Build the Mission lens." },
      secret: BUILDER_SECRET,
      target: BUILDER_TARGET,
      turnId: "builder-turn",
    }),
    signedTranscript({
      createdAt: now - 29,
      eventSeq: 2,
      item: {
        kind: "plan",
        entries: [
          { content: "Wire the participant roster", status: "completed" },
          { content: "Verify the signed live activity", status: "in_progress" },
        ],
      },
      secret: BUILDER_SECRET,
      target: BUILDER_TARGET,
      turnId: "builder-turn",
    }),
    signedTranscript({
      createdAt: now - 28,
      eventSeq: 3,
      item: {
        kind: "tool_call",
        tool: {
          toolName: "Read",
          toolId: "read-mission",
          input: { path: "CodingSessionUmbrellaWorkspace.tsx" },
        },
      },
      secret: BUILDER_SECRET,
      target: BUILDER_TARGET,
      turnId: "builder-turn",
    }),
    signedMetadata({
      actor: VERIFIER_ACTOR,
      model: "gpt-5.6-sol",
      role: "verifier",
      runtime: "codex-acp",
      secret: VERIFIER_SECRET,
      status: "completed",
      target: VERIFIER_TARGET,
      title: "Portable team loop",
      createdAt: now - 12,
    }),
    signedTranscript({
      createdAt: now - 11,
      eventSeq: 1,
      item: { kind: "assistant_text", text: "The Mission hierarchy is sound." },
      secret: VERIFIER_SECRET,
      target: VERIFIER_TARGET,
      turnId: "verifier-turn",
    }),
    signedTranscript({
      createdAt: now - 10,
      eventSeq: 2,
      item: {
        kind: "result",
        subtype: "success",
        isError: false,
        durationMs: 2_000,
        result: "Verified",
      },
      secret: VERIFIER_SECRET,
      target: VERIFIER_TARGET,
      turnId: "verifier-turn",
    }),
  ];
}

async function seedAndOpen(page: Page) {
  await page.getByTestId(`channel-${CHANNEL_NAME}`).click();
  await page.evaluate(
    ({ channelName, events }) => {
      const seed = window.__BUZZ_E2E_SEED_MOCK_SIGNED_EVENT__;
      if (!seed) throw new Error("signed-event seeding hook is missing");
      for (const event of events) seed({ channelName, event });
    },
    { channelName: CHANNEL_NAME, events: missionEvents() },
  );
  const trigger = page.getByTestId("channel-coding-sessions-trigger");
  await expect(trigger).toHaveAttribute("aria-label", "Coding sessions (1)", {
    timeout: 15_000,
  });
  await trigger.click();
  await page.getByTestId("channel-coding-session-open").first().click();
  await expect(
    page.getByTestId("coding-session-umbrella-workspace"),
  ).toBeVisible({ timeout: 15_000 });
}

async function openMockApp(
  page: Page,
  input: {
    reducedMotion: "no-preference" | "reduce";
    theme: "buzz" | "buzz-dark";
  },
) {
  await page.emulateMedia({ reducedMotion: input.reducedMotion });
  await page.addInitScript(({ theme }) => {
    window.localStorage.setItem("buzz-theme", theme);
    window.localStorage.setItem("buzz:text-scale", "1.25");
  }, input);
  await installMockBridge(page, {
    globalAgentConfig: {
      env_vars: {},
      provider: null,
      model: null,
      "allowed-bridge-pubkeys": [
        { pubkey: BUILDER_PROVIDER, label: "Builder provider" },
        { pubkey: VERIFIER_PROVIDER, label: "Verifier provider" },
      ],
    },
    searchProfiles: [
      { pubkey: BUILDER_ACTOR, displayName: "Bob" },
      { pubkey: VERIFIER_ACTOR, displayName: "Parallax" },
    ],
  });
  await page.setViewportSize({ width: 1100, height: 800 });
  await page.goto("/");
}

async function elapsedSeconds(locator: Locator): Promise<number> {
  const text = (await locator.textContent()) ?? "";
  const match = text.match(/(\d+(?:\.\d+)?)s/);
  if (!match) throw new Error(`elapsed seconds missing from: ${text}`);
  return Number(match[1]);
}

test("Conversation and Mission are explicit persistent lenses over one signed session", async ({
  page,
}) => {
  await openMockApp(page, {
    reducedMotion: "no-preference",
    theme: "buzz",
  });
  await seedAndOpen(page);
  await expect
    .poll(() =>
      page.evaluate(() => getComputedStyle(document.documentElement).fontSize),
    )
    .toBe("20px");
  await expect(page.locator("html")).toHaveAttribute("data-buzz-theme", "buzz");

  const conversation = page.getByRole("button", {
    name: "Conversation lens",
  });
  const mission = page.getByRole("button", { name: "Mission lens" });
  await expect(conversation).toHaveAttribute("aria-pressed", "true");
  await expect(mission).toHaveAttribute("aria-pressed", "false");
  await expect(
    page.getByTestId("coding-session-disposition-strip"),
  ).toBeVisible();
  await expect(
    page.getByTestId("coding-session-active-work-dock"),
  ).toBeVisible();
  await expect(page.getByTestId("coding-session-participant-bar")).toHaveCount(
    0,
  );
  await expect(
    page.getByTestId("coding-session-umbrella-timeline"),
  ).toBeVisible();
  await expect(
    page.getByTestId("coding-session-umbrella-composer"),
  ).toBeVisible();
  await waitForAnimations(page);
  await page.getByTestId("coding-session-umbrella-workspace").screenshot({
    path: `${SCREENSHOTS}/conversation.png`,
  });

  await mission.focus();
  await page.keyboard.press("Enter");
  await expect(mission).toHaveAttribute("aria-pressed", "true");
  await expect(page.getByTestId("coding-session-participant-chip")).toHaveCount(
    2,
  );
  await expect(
    page.getByTestId("coding-session-participant-bar"),
  ).toContainText("Bob · Builder");
  await expect(
    page.getByTestId("coding-session-participant-bar"),
  ).toContainText("Parallax · Verifier");
  const live = page.getByTestId("coding-session-live-activity-bar");
  await expect(live).toBeVisible();
  await expect(live).toContainText("Verify the signed live activity");
  await expect(live).toContainText("1 tool this turn");
  await expect(live).toContainText(/\d+(?:\.\d+)?s/);
  const firstElapsed = await elapsedSeconds(live);
  await expect
    .poll(() => elapsedSeconds(live), { timeout: 4_000 })
    .toBeGreaterThan(firstElapsed);
  await expect(
    page.getByTestId("coding-session-disposition-strip"),
  ).toHaveCount(0);
  await expect(page.getByTestId("coding-session-agent-focus")).toHaveCount(0);
  await expect(page.getByTestId("coding-session-active-work-dock")).toHaveCount(
    0,
  );
  await page.getByRole("button", { name: /Focus Parallax · Verifier/ }).click();
  await expect(
    page.getByTestId("coding-session-focused-agent-notice"),
  ).toContainText("Parallax");
  await waitForAnimations(page);
  await page.getByTestId("coding-session-umbrella-workspace").screenshot({
    path: `${SCREENSHOTS}/mission.png`,
  });

  await page.reload();
  await seedAndOpen(page);
  await expect(
    page.getByRole("button", { name: "Mission lens" }),
  ).toHaveAttribute("aria-pressed", "true");
  await expect(
    page.getByTestId("coding-session-participant-bar"),
  ).toBeVisible();

  const restoredConversation = page.getByRole("button", {
    name: "Conversation lens",
  });
  await restoredConversation.focus();
  await page.keyboard.press("Enter");
  await expect(restoredConversation).toHaveAttribute("aria-pressed", "true");
  await expect(page.getByTestId("coding-session-participant-bar")).toHaveCount(
    0,
  );
  await expect(
    page.getByTestId("coding-session-disposition-strip"),
  ).toBeVisible();
  await expect(
    page.getByTestId("coding-session-umbrella-timeline"),
  ).toContainText("The Mission hierarchy is sound.");
});

test("Mission remains accessible in dark, narrow, reduced-motion layout", async ({
  page,
}) => {
  await openMockApp(page, {
    reducedMotion: "reduce",
    theme: "buzz-dark",
  });
  await seedAndOpen(page);
  await expect(page.locator("html")).toHaveAttribute(
    "data-buzz-theme",
    "buzz-dark",
  );
  await expect
    .poll(() =>
      page.evaluate(
        () => window.matchMedia("(prefers-reduced-motion: reduce)").matches,
      ),
    )
    .toBe(true);

  await page.setViewportSize({ width: 480, height: 760 });
  const lens = page.getByRole("group", { name: "Session lens" });
  const conversation = lens.getByRole("button", {
    name: "Conversation lens",
  });
  const mission = lens.getByRole("button", { name: "Mission lens" });
  await expect(conversation).toHaveAttribute("aria-pressed", "true");
  await mission.focus();
  await expect(mission).toBeFocused();
  await page.keyboard.press("Enter");
  await expect(mission).toHaveAttribute("aria-pressed", "true");

  const participants = page.getByRole("navigation", {
    name: "Session participants",
  });
  await expect(participants).toBeVisible();
  const overflow = await participants.evaluate((element) => ({
    clientWidth: element.clientWidth,
    overflowX: getComputedStyle(element).overflowX,
    scrollWidth: element.scrollWidth,
  }));
  expect(overflow.scrollWidth).toBeGreaterThan(overflow.clientWidth);
  expect(["auto", "scroll"]).toContain(overflow.overflowX);
  await expect
    .poll(() =>
      page.evaluate(() => document.documentElement.scrollWidth <= innerWidth),
    )
    .toBe(true);

  const bob = participants.getByRole("button", {
    name: /Focus Bob · Builder/,
  });
  await bob.focus();
  await expect(bob).toBeFocused();
  await page.keyboard.press("Enter");
  await expect(
    page.getByTestId("coding-session-focused-agent-notice"),
  ).toContainText("Bob");
  await waitForAnimations(page);
  await participants.screenshot({
    path: `${SCREENSHOTS}/mission-dark-narrow-reduced-participants.png`,
  });
  await page.getByTestId("coding-session-live-activity-bar").screenshot({
    path: `${SCREENSHOTS}/mission-dark-narrow-reduced-activity.png`,
  });

  await conversation.focus();
  await page.keyboard.press("Enter");
  await expect(conversation).toHaveAttribute("aria-pressed", "true");
  await expect(participants).toHaveCount(0);
  await expect(
    page.getByTestId("coding-session-disposition-strip"),
  ).toBeVisible();
  await expect(
    page.getByTestId("coding-session-umbrella-timeline"),
  ).toContainText("The Mission hierarchy is sound.");
  await expect(
    page.getByTestId("coding-session-umbrella-composer"),
  ).toBeVisible();
});
