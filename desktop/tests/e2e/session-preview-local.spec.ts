import { createHash } from "node:crypto";
import { readFileSync } from "node:fs";

import {
  expect,
  type Locator,
  type Page,
  type TestInfo,
  test,
} from "@playwright/test";
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
import { waitForAnimations } from "../helpers/animations";
import { installMockBridge } from "../helpers/bridge";
import { shotPath } from "../helpers/shotPath";

// SV-33 S1/S2 lane L2: the session's local Browser surface, in the mock
// bridge. The native WKWebView is NOT here: `e2eBridgeSessionPreview.ts`
// draws a box labelled "native webview (mock)" where Rust would place it, so
// these shots prove layout, binding and the honesty text, never native
// stacking, focus or input (BRIEF § 7: those are checked in the real app).

test.describe.configure({ mode: "serial" });

const SHOTS = "test-results/session-preview";
const SHOT_NAMES = [
  "browser-empty-state",
  "browser-docked",
  "browser-floating",
  "browser-driving-strip",
  "browser-unavailable",
] as const;

const secret = generateSecretKey();
const pubkey = getPublicKey(secret);
const channelName = "engineering";
const channelId = "1c7e1c02-87bb-5e88-b2da-5a7a9432d0c9";
const session = {
  driver: "claude-agent-acp",
  instanceId: "c4c4c4c4c4c4c4c4",
  sessionId: "c4000000-0000-4000-8000-000000000033",
  generation: 1,
};
const targetKey = buildCodingSessionTargetKey(session);
const SERVERS = [
  {
    port: 5173,
    url: "http://localhost:5173/",
    address: "127.0.0.1",
    pid: 4242,
    process: "node",
    title: "Vite App",
  },
  {
    port: 8000,
    url: "http://localhost:8000/",
    address: "::1",
    pid: 4343,
    process: "Python",
    title: "Directory listing for /",
  },
];

function signed(kind: number, seq: number, content: unknown, tags: string[][]) {
  return finalizeEvent(
    {
      kind,
      created_at: 1_800_900_000 + seq,
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
      title: "Local preview",
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
        timestamp: 1_800_900_000_000 + seq * 1_000,
        turnId: "preview-turn",
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
    transcript(1, {
      kind: "user_prompt",
      content: "Start the dev server and check the sign-in form.",
    }),
    transcript(2, {
      kind: "assistant_text",
      text: "Vite is up on port 5173.",
    }),
  ];
}

async function declarePreviewMock(
  page: Page,
  unavailable: { code: string; sentence: string } | null = null,
) {
  await page.addInitScript(
    ({ servers, unavailable: reason }) => {
      // The Browser is macOS-only; pin the platform so a Linux runner shows
      // the surface rather than its "macOS" reason.
      Object.defineProperty(window.navigator, "platform", {
        configurable: true,
        get: () => "MacIntel",
      });
      window.__BEEKEEPER_E2E_SESSION_PREVIEW__ = {
        servers,
        unavailable: reason,
      };
    },
    { servers: SERVERS, unavailable },
  );
}

async function openBrowser(page: Page): Promise<Locator> {
  await page.goto("/");
  await page.getByTestId(`channel-${channelName}`).click();
  await page.evaluate(
    ({ channelName: name, events: signedEvents }) => {
      const seed = window.__BEEKEEPER_E2E_SEED_MOCK_SIGNED_EVENT__;
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
    "Local preview",
  );
  // From the launcher, by its row (acceptance 1: "opens from the launcher").
  await page.keyboard.press("ControlOrMeta+Alt+KeyB");
  const row = page.getByTestId("coding-session-surface-launcher-row-browser");
  await expect(row).toHaveAttribute("data-available", "true");
  await row.click();
  const panel = page.getByTestId("coding-session-surface-panel-browser");
  await expect(panel).toBeVisible();
  return panel;
}

async function shoot(
  page: Page,
  testInfo: TestInfo,
  name: string,
  locator: Locator,
) {
  await expect(locator).toBeVisible();
  await page.mouse.move(2, 2);
  await waitForAnimations(page);
  await locator.screenshot({ path: shotPath(testInfo, SHOTS, name) });
}

test("S1: empty state, docked, floating, an overlay freezes it, and the driving strip", async ({
  page,
}, testInfo) => {
  test.setTimeout(120_000);
  await page.setViewportSize({ width: 1440, height: 900 });
  await declarePreviewMock(page);
  await installMockBridge(page, {
    codingSessionProviderStatus: {
      provisioned: true,
      running: true,
      providerPubkey: pubkey,
    },
  });
  const panel = await openBrowser(page);

  // Empty state: this computer's servers and the always-on local label.
  const empty = panel.getByTestId("session-preview-empty");
  await expect(empty.getByTestId("session-preview-server")).toHaveCount(2);
  await expect(empty).toContainText("Local servers");
  await expect(empty).toContainText("Recently used");
  await expect(panel.getByTestId("session-preview-local-only")).toHaveText(
    "Local only · not shared yet",
  );
  await shoot(page, testInfo, "browser-empty-state", panel);

  // Click docks it, bound to this session (the composer's target).
  await empty.getByTestId("session-preview-server").first().click();
  const surface = panel.getByTestId("session-preview-surface");
  await expect(surface).toHaveAttribute("data-placement", "docked");
  const standIn = page.getByTestId("e2e-mock-native-webview");
  await expect(standIn).toBeVisible();
  const navigations = await page.evaluate(
    () => window.__BEEKEEPER_E2E_SESSION_PREVIEW__?.navigations ?? [],
  );
  expect(navigations.at(-1)).toMatchObject({
    url: "http://localhost:5173/",
    target: { sessionId: session.sessionId, instanceId: session.instanceId },
  });
  // The stand-in sits on the slot (inset 2 px), as Rust places the view.
  const slotBox = await panel.getByTestId("session-preview-slot").boundingBox();
  const standInBox = await standIn.boundingBox();
  expect(slotBox && standInBox).toBeTruthy();
  if (slotBox && standInBox) {
    expect(Math.abs(standInBox.x - (slotBox.x + 2))).toBeLessThanOrEqual(1);
    expect(
      Math.abs(standInBox.width - (slotBox.width - 4)),
    ).toBeLessThanOrEqual(1);
  }
  await shoot(page, testInfo, "browser-docked", panel);

  // An overlay over the slot hides the view and paints the freeze frame.
  await panel.getByTestId("session-preview-overflow").click();
  await expect(page.getByTestId("session-preview-toggle-float")).toBeVisible();
  // The menu opens below the toolbar, over the slot.
  await expect(panel.getByTestId("session-preview-slot")).toHaveAttribute(
    "data-frozen",
    "true",
  );
  await expect(standIn).toBeHidden();
  await expect(panel.getByTestId("session-preview-freeze-frame")).toBeVisible();

  // Float it over the transcript; the stand-in follows the floating slot.
  await page.getByTestId("session-preview-toggle-float").click();
  const floating = page.getByTestId("session-preview-floating");
  await expect(floating).toBeVisible();
  await expect(surface).toHaveAttribute("data-placement", "floating");
  await expect(standIn).toBeVisible();
  const before = await floating.boundingBox();
  const handle = page.getByTestId("session-preview-floating-handle");
  const handleBox = await handle.boundingBox();
  if (before && handleBox) {
    await page.mouse.move(handleBox.x + 40, handleBox.y + 10);
    await page.mouse.down();
    await page.mouse.move(handleBox.x - 160, handleBox.y - 90, { steps: 6 });
    await page.mouse.up();
    const after = await floating.boundingBox();
    expect(after && after.x < before.x).toBeTruthy();
  }
  await expect
    .poll(async () => {
      const slot = await floating
        .getByTestId("session-preview-slot")
        .boundingBox();
      const box = await standIn.boundingBox();
      return slot && box ? Math.round(Math.abs(box.x - (slot.x + 2))) : 99;
    })
    .toBeLessThanOrEqual(1);
  await shoot(page, testInfo, "browser-floating", floating);

  // Dock it back, then an agent drives it: the strip names the agent.
  await page.getByTestId("session-preview-dock").click();
  await expect(surface).toHaveAttribute("data-placement", "docked");
  await page.evaluate(
    ({ channelId: id, sessionId }) =>
      window.__BEEKEEPER_E2E_SESSION_PREVIEW_DRIVE__?.({
        channelId: id,
        sessionId,
        executionId: `exec-${sessionId}`,
        op: "click",
      }),
    { channelId, sessionId: session.sessionId },
  );
  const driving = panel.getByTestId("session-preview-driving");
  await expect(driving).toHaveText(
    /^Agent \(.+\) is driving · synthetic input$/,
  );
  await shoot(page, testInfo, "browser-driving-strip", panel);

  // Pop out and back, then close: the empty state again, with its note.
  await panel.getByTestId("session-preview-popout").click();
  await expect(surface).toHaveAttribute("data-placement", "popped_out");
  await expect(standIn).toBeHidden();
  await panel.getByTestId("session-preview-bring-back").click();
  await expect(surface).toHaveAttribute("data-placement", "docked");
  await panel.getByTestId("session-preview-overflow").click();
  await page.getByTestId("session-preview-close").click();
  await expect(panel.getByTestId("session-preview-empty")).toBeVisible();
  await expect(panel.getByTestId("session-preview-recent")).toHaveCount(1);
  await expect(standIn).toBeHidden();

  // An external address is refused in the broker's sentence.
  await panel.getByTestId("session-preview-server").first().click();
  await panel.getByTestId("session-preview-url").fill("https://example.com");
  await panel.getByTestId("session-preview-url").press("Enter");
  await expect(panel.getByTestId("session-preview-refusal")).toHaveText(
    "The Browser only opens pages on this computer (localhost, 127.0.0.1, [::1]).",
  );
});

test("S1: an unavailable Browser says why", async ({ page }, testInfo) => {
  await page.setViewportSize({ width: 1440, height: 900 });
  await declarePreviewMock(page, {
    code: "content_filter_failed",
    sentence:
      "The Browser could not start its local-only filter, so it stays off.",
  });
  await installMockBridge(page, {
    codingSessionProviderStatus: {
      provisioned: true,
      running: true,
      providerPubkey: pubkey,
    },
  });
  const panel = await openBrowser(page);
  await expect(panel.getByTestId("session-preview-notice")).toHaveText(
    "The Browser could not start its local-only filter, so it stays off.",
  );
  await shoot(page, testInfo, "browser-unavailable", panel);
});

test("every shot is a distinct state", async ({ page: _page }, testInfo) => {
  const hashes = SHOT_NAMES.map((name) =>
    createHash("sha256")
      .update(readFileSync(shotPath(testInfo, SHOTS, name)))
      .digest("hex"),
  );
  expect(new Set(hashes).size).toBe(SHOT_NAMES.length);
});
