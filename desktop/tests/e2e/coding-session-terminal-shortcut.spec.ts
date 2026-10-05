import { expect, test } from "@playwright/test";
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
  BUZZ_CODING_SESSION_TRANSCRIPT_SCHEMA,
  codingSessionTranscriptSemanticKey,
  CODING_SESSION_TRANSCRIPT_TAG_VERSION,
} from "@/features/coding-sessions/lib/codingSessionTranscriptPresentation";
import {
  KIND_CODING_SESSION_METADATA,
  KIND_CODING_SESSION_TRANSCRIPT,
} from "@/shared/constants/kinds";
import type { RelayEvent } from "@/shared/api/types";
import { installMockBridge } from "../helpers/bridge";

// Session-view parity Wave B, DB10: on a coding-session route the channel
// terminal (Substrate) stands down, so ⌘J toggles the session's own drawer —
// even though the route keeps its channel selected and so gives the channel
// terminal a context. The first half proves that context exists here.

const secret = generateSecretKey();
const pubkey = getPublicKey(secret);
const channelName = "engineering";
const channelId = "1c7e1c02-87bb-5e88-b2da-5a7a9432d0c9";
const session = {
  driver: "claude-agent-acp",
  instanceId: "db10db10db10db10",
  sessionId: "db100000-0000-4000-8000-000000000010",
  generation: 1,
};
const targetKey = buildCodingSessionTargetKey(session);
const DOCKED_TERM =
  '[data-terminal-owner="terminal"][data-terminal-mode="docked"]';

function signed(kind: number, seq: number, content: unknown, tags: string[][]) {
  return finalizeEvent(
    {
      kind,
      created_at: 1_800_600_000 + seq,
      tags: [["h", channelId], ...tags],
      content: JSON.stringify(content),
    },
    secret,
  ) as unknown as RelayEvent;
}

function events(): RelayEvent[] {
  const metadata = signed(
    KIND_CODING_SESSION_METADATA,
    0,
    {
      schema: BUZZ_CODING_SESSION_METADATA_SCHEMA,
      session,
      projectRef: null,
      repoRef: null,
      title: "Terminal shortcut",
      agentRef: null,
      provider: "claude-agent-acp",
      runtime: "claude-agent-acp",
      model: "sonnet",
      status: "completed",
      branch: null,
      capabilities: {
        threadTurnStart: true,
        threadTurnInterrupt: true,
        threadSteer: true,
        promptImage: false,
        context: false,
        diff: false,
        plan: false,
      },
    },
    [
      ["csm-v", CODING_SESSION_METADATA_TAG_VERSION],
      ["cs-target", targetKey],
      ["csm-key", codingSessionMetadataSemanticKey(session)],
    ],
  );
  const transcript = (seq: number, item: unknown) =>
    signed(
      KIND_CODING_SESSION_TRANSCRIPT,
      seq,
      {
        schema: BUZZ_CODING_SESSION_TRANSCRIPT_SCHEMA,
        session,
        eventSeq: seq,
        timestamp: 1_800_600_000_000 + seq * 1_000,
        turnId: "shortcut-turn",
        item,
      },
      [
        ["cst-v", CODING_SESSION_TRANSCRIPT_TAG_VERSION],
        ["cs-target", targetKey],
        ["cst-seq", String(seq)],
        ["cst-key", codingSessionTranscriptSemanticKey(session, seq)],
      ],
    );
  return [
    metadata,
    transcript(1, { kind: "user_prompt", content: "Open a terminal." }),
    transcript(2, {
      kind: "result",
      subtype: "success",
      isError: false,
      durationMs: 1_000,
      result: "Done.",
    }),
  ];
}

test("DB10: ⌘J opens the session drawer, not the channel terminal", async ({
  page,
}) => {
  test.setTimeout(90_000);
  await page.setViewportSize({ width: 1440, height: 900 });
  await installMockBridge(page, {
    globalAgentConfig: {
      env_vars: {},
      provider: null,
      model: null,
      "allowed-bridge-pubkeys": [{ pubkey, label: "Shortcut provider" }],
    },
  });
  await page.goto("/");
  await page.getByTestId(`channel-${channelName}`).click();
  await expect(page.getByTestId("chat-title")).toHaveText(channelName);

  // The channel terminal has a context on a channel: its capture listener
  // claims a ⌘J keydown there (keydown only, so its panel does not toggle).
  const claimedOnChannel = () =>
    page.evaluate(() => {
      const event = new KeyboardEvent("keydown", {
        bubbles: true,
        cancelable: true,
        code: "KeyJ",
        ctrlKey: !navigator.platform.startsWith("Mac"),
        metaKey: navigator.platform.startsWith("Mac"),
      });
      window.dispatchEvent(event);
      return event.defaultPrevented;
    });
  expect(await claimedOnChannel()).toBe(true);

  await page.evaluate(
    ({ channelName: name, events: signedEvents }) => {
      const seed = window.__BUZZ_E2E_SEED_MOCK_SIGNED_EVENT__;
      if (!seed) throw new Error("signed-event seeding hook is missing");
      for (const event of signedEvents) seed({ channelName: name, event });
    },
    { channelName, events: events() },
  );
  const trigger = page.getByTestId("channel-coding-sessions-trigger");
  await expect(trigger).toHaveAttribute("aria-label", "Coding sessions (1)", {
    timeout: 15_000,
  });
  await trigger.click();
  await page.getByTestId("channel-coding-session-open").click();
  await expect(page.getByTestId("coding-session-workspace")).toContainText(
    "Terminal shortcut",
  );

  // Same channel selected, so the channel terminal still has its context —
  // and stands down: ⌘J toggles the session drawer only.
  const drawer = page.getByTestId("coding-session-drawer-host");
  await expect(drawer).toHaveCount(0);
  await page.keyboard.press("ControlOrMeta+KeyJ");
  await expect(drawer).toBeVisible();
  await expect(page.locator(DOCKED_TERM)).toHaveCount(0);
  await page.keyboard.press("ControlOrMeta+KeyJ");
  await expect(drawer).toHaveCount(0);
  await expect(page.locator(DOCKED_TERM)).toHaveCount(0);
});
