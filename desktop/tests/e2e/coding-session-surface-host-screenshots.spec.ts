import { expect, test } from "@playwright/test";
import {
  finalizeEvent,
  generateSecretKey,
  getPublicKey,
} from "nostr-tools/pure";

import { buildCodingSessionTargetKey } from "@/features/coding-sessions/lib/codingSessionCommand";
import {
  BEEKEEPER_CODING_SESSION_METADATA_SCHEMA,
  codingSessionMetadataSemanticKey,
  CODING_SESSION_METADATA_TAG_VERSION,
} from "@/features/coding-sessions/lib/codingSessionIngressPayloads";
import {
  BEEKEEPER_CODING_SESSION_TRANSCRIPT_SCHEMA,
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

/**
 * The shared right-side surface host (Slice 1 of the coding-session UI
 * convergence, reworked by SV-21): one collapsible, resizable panel whose
 * tabs are the surfaces the person opened — Agents first here, then Diff
 * added from "+" — with a single sheet presentation on narrow layouts.
 */

const SHOTS = "test-results/coding-session-surface-host";
const PROVIDER_SECRET = generateSecretKey();
const PROVIDER_PUBKEY = getPublicKey(PROVIDER_SECRET);
const CHANNEL_NAME = "engineering";
const CHANNEL_ID = "1c7e1c02-87bb-5e88-b2da-5a7a9432d0c9";
const SESSION_REF = "7d1f3b58-2a44-4c1e-9b60-8e5a2f9c4d31";
const BASE_CREATED_AT = 1_800_200_000;
const BASE_TIMESTAMP_MS = 1_800_200_000_000;

const CLAUDE_TARGET = {
  driver: "claude-agent-acp",
  instanceId: "surface-claude",
  sessionId: "aaaa1111-bbbb-2222-cccc-333344445555",
  generation: 1,
};
const CODEX_TARGET = {
  driver: "codex-acp",
  instanceId: "surface-codex",
  sessionId: "dddd6666-eeee-7777-ffff-888899990000",
  generation: 1,
};

/**
 * One execution's signed 44223 metadata.
 *
 * `status` is a parameter because this fixture used to sign `completed` for
 * every execution while asserting the surfaces read `1 working` — it encoded
 * WALK-2026-08-29 finding 2 (the strip inferred `live` from an unterminated
 * transcript and printed it over the signed word). The seat this spec calls
 * working now says so on the wire.
 */
function metadataEvent(
  target: typeof CLAUDE_TARGET,
  runtime: string,
  model: string,
  createdAt: number,
  status: string,
): RelayEvent {
  return finalizeEvent(
    {
      kind: KIND_CODING_SESSION_METADATA,
      created_at: createdAt,
      tags: [
        ["h", CHANNEL_ID],
        ["csm-v", CODING_SESSION_METADATA_TAG_VERSION],
        ["cs-target", buildCodingSessionTargetKey(target)],
        ["csm-key", codingSessionMetadataSemanticKey(target)],
      ],
      content: JSON.stringify({
        schema: BEEKEEPER_CODING_SESSION_METADATA_SCHEMA,
        session: target,
        projectRef: null,
        repoRef: null,
        title: "Converge the session surfaces",
        agentRef: null,
        provider: target.driver,
        runtime,
        model,
        status,
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
}

function transcriptEvent(
  target: typeof CLAUDE_TARGET,
  eventSeq: number,
  turnId: string,
  item: unknown,
  createdAt: number,
): RelayEvent {
  return finalizeEvent(
    {
      kind: KIND_CODING_SESSION_TRANSCRIPT,
      created_at: createdAt,
      tags: [
        ["h", CHANNEL_ID],
        ["cst-v", CODING_SESSION_TRANSCRIPT_TAG_VERSION],
        ["cs-target", buildCodingSessionTargetKey(target)],
        ["cst-seq", String(eventSeq)],
        ["cst-key", codingSessionTranscriptSemanticKey(target, eventSeq)],
      ],
      content: JSON.stringify({
        schema: BEEKEEPER_CODING_SESSION_TRANSCRIPT_SCHEMA,
        session: target,
        eventSeq,
        timestamp: BASE_TIMESTAMP_MS + eventSeq * 1_000,
        turnId,
        item,
      }),
    },
    PROVIDER_SECRET,
  ) as unknown as RelayEvent;
}

function seededEvents(): RelayEvent[] {
  return [
    metadataEvent(
      CLAUDE_TARGET,
      "claude-agent-acp",
      "sonnet",
      BASE_CREATED_AT,
      // The seat whose plan the active-work dock shows: signed `running`, and
      // its turn has no terminator, so W1 narrows it to live rather than
      // inventing that word from the transcript alone.
      "running",
    ),
    transcriptEvent(
      CLAUDE_TARGET,
      1,
      "turn-1",
      { kind: "user_prompt", content: "Converge the session surfaces." },
      BASE_CREATED_AT + 1,
    ),
    transcriptEvent(
      CLAUDE_TARGET,
      2,
      "turn-1",
      {
        kind: "tool_call",
        tool: {
          toolName: "str_replace",
          toolId: "edit-host",
          input: {
            path: "desktop/src/features/coding-sessions/ui/CodingSessionSurfaceHost.tsx",
            oldString: "const rails = 2;",
            newString: "const surfaceHost = 1;",
          },
        },
      },
      BASE_CREATED_AT + 2,
    ),
    transcriptEvent(
      CLAUDE_TARGET,
      3,
      "turn-1",
      {
        kind: "tool_result",
        toolId: "edit-host",
        toolName: "str_replace",
        content: "Edited successfully",
        isError: false,
      },
      BASE_CREATED_AT + 3,
    ),
    transcriptEvent(
      CLAUDE_TARGET,
      4,
      "turn-1",
      {
        kind: "plan",
        entries: [
          { content: "Unify the multi-agent timeline", status: "completed" },
          {
            content: "Keep agent identity visible while reading",
            status: "in_progress",
          },
          { content: "Verify focus without retargeting", status: "pending" },
        ],
      },
      BASE_CREATED_AT + 4,
    ),
    transcriptEvent(
      CLAUDE_TARGET,
      5,
      "turn-1",
      {
        kind: "assistant_text",
        text: "Both rails now share one collapsible surface host.",
      },
      BASE_CREATED_AT + 5,
    ),
    metadataEvent(
      CODEX_TARGET,
      "codex-acp",
      "gpt-5.6-sol",
      BASE_CREATED_AT + 10,
      "completed",
    ),
    transcriptEvent(
      CODEX_TARGET,
      1,
      "review-turn",
      {
        kind: "assistant_text",
        text: "The tab semantics and resize teardown look correct.",
      },
      BASE_CREATED_AT + 11,
    ),
  ];
}

async function openUmbrellaWorkspace(page: import("@playwright/test").Page) {
  await installMockBridge(page, {
    globalAgentConfig: {
      env_vars: {},
      provider: null,
      model: null,
      "allowed-bridge-pubkeys": [
        { pubkey: PROVIDER_PUBKEY, label: "Surface-host screenshots" },
      ],
    },
  });
  await page.goto("/");
  await page.getByTestId(`channel-${CHANNEL_NAME}`).click();
  await page.evaluate(
    ({ channelName, events }) => {
      const seed = window.__BEEKEEPER_E2E_SEED_MOCK_SIGNED_EVENT__;
      if (!seed) throw new Error("signed-event seeding hook is missing");
      for (const event of events) seed({ channelName, event });
    },
    { channelName: CHANNEL_NAME, events: seededEvents() },
  );

  const trigger = page.getByTestId("channel-coding-sessions-trigger");
  await expect(trigger).toHaveAttribute(
    "aria-label",
    /Coding sessions \(\d+\)/,
    { timeout: 15_000 },
  );
  await trigger.click();
  await page.getByTestId("channel-coding-session-open").first().click();
  const workspace = page.getByTestId("coding-session-umbrella-workspace");
  await expect(workspace).toBeVisible({ timeout: 15_000 });
  return workspace;
}

test("merged work focuses in place and the shared surface remains responsive", async ({
  page,
}) => {
  await page.setViewportSize({ width: 1440, height: 900 });
  const workspace = await openUmbrellaWorkspace(page);

  // 1 — the medium-width default is the complete merged narrative. Active
  // work is attached to the composer and includes only the working execution.
  const host = page.getByTestId("coding-session-surface-host");
  await expect(host).toHaveCount(0);
  const focusTrigger = page.getByTestId("coding-session-agent-focus-trigger");
  await expect(focusTrigger).toContainText("2 agents · 1 working");
  const activeWork = page.getByTestId("coding-session-active-work-dock");
  await expect(activeWork).toContainText("Keep agent identity visible");
  await expect(activeWork).not.toContainText(
    "The tab semantics and resize teardown look correct",
  );
  await waitForAnimations(page);
  await workspace.screenshot({ path: `${SHOTS}/01-merged-active-work.png` });

  // 2 — focus folds the other execution in place. It never removes that
  // execution from the chronology and never retargets the composer.
  const recipientBeforeFocus =
    (await page
      .getByTestId("coding-session-participant-picker-trigger")
      .textContent()) ?? "";
  const narrativeScroll = page.getByTestId("coding-session-narrative-scroll");
  await narrativeScroll.evaluate((element) => {
    element.scrollTop = 0;
  });
  await focusTrigger.click();
  const focusChoices = page.getByTestId("coding-session-agent-focus-chip");
  await expect(focusChoices).toHaveCount(2);
  await focusChoices.filter({ hasText: "Codex" }).click();
  await expect(
    page.getByTestId("coding-session-umbrella-folded-turn"),
  ).toHaveCount(1);
  await expect(
    page.getByTestId("coding-session-umbrella-timeline"),
  ).toContainText("Both rails now share one collapsible surface host.");
  await expect(
    page.getByTestId("coding-session-participant-picker-trigger"),
  ).toHaveText(recipientBeforeFocus);
  await expect(activeWork).toBeVisible();
  await expect(activeWork).toBeInViewport();
  await expect(page.getByTestId("coding-session-composer")).toBeInViewport();
  await expect(
    page.getByTestId("coding-session-focused-agent-notice"),
  ).toContainText("Viewing Codex");
  await expect
    .poll(async () =>
      narrativeScroll.evaluate(
        (element) =>
          element.scrollHeight - element.clientHeight - element.scrollTop,
      ),
    )
    .toBeLessThan(8);
  await waitForAnimations(page);
  const focusedClip = await workspace.boundingBox();
  if (!focusedClip) throw new Error("focused workspace geometry missing");
  await page.screenshot({
    clip: focusedClip,
    path: `${SHOTS}/02-codex-focus.png`,
  });

  // Return to All, then open the detail surface explicitly at this width.
  await page.getByTestId("coding-session-focused-agent-clear").click();
  await focusTrigger.click();
  await page.getByTestId("coding-session-agent-details-toggle").click();
  await expect(host).toBeVisible();
  await expect(host).toContainText("All agents");
  await expect(host).toContainText("Claude");
  await expect(host).toContainText("Codex");
  // D6 / walk finding 1: the rail rows, the strip and the footer tally are
  // three readers of W1 and used to disagree in one frame. Claude is signed
  // `running` with an open turn; Codex is signed `completed`. Nothing in the
  // panel may say otherwise, and `All idle` is gone with the raw recount.
  await expect(host).toContainText("1 live · 1 idle");
  await expect(host).not.toContainText("All idle");
  await expect(host).not.toContainText("Working");
  await waitForAnimations(page);
  await workspace.screenshot({ path: `${SHOTS}/03-agents-open.png` });

  // 3 — adding Diff from "+" opens it as a second tab and swaps the content
  // without closing or resizing the host (SV-21).
  const widthBeforeSwitch = (await host.boundingBox())?.width ?? 0;
  await page.getByTestId("coding-session-surface-add").click();
  await page.getByTestId("coding-session-surface-add-diff").click();
  await expect(
    page.getByTestId("coding-session-surface-tab-diff"),
  ).toHaveAttribute("aria-selected", "true");
  const changes = page.getByTestId("coding-session-changes-rail");
  await expect(changes).toBeVisible();
  await expect(changes).toContainText("CodingSessionSurfaceHost.tsx");
  await expect(changes).toContainText("Observed in transcript activity");
  expect(Math.round((await host.boundingBox())?.width ?? 0)).toBe(
    Math.round(widthBeforeSwitch),
  );
  await waitForAnimations(page);
  await workspace.screenshot({ path: `${SHOTS}/04-observed-changes.png` });

  // 4 — the host resizes via its separator; the surface only re-flows.
  const resize = page.getByRole("separator", {
    name: "Resize session surface",
  });
  const handle = await resize.boundingBox();
  if (!handle) throw new Error("surface host resize geometry missing");
  await page.mouse.move(handle.x + handle.width / 2, handle.y + 160);
  await page.mouse.down();
  await page.mouse.move(handle.x - 160, handle.y + 160);
  await page.mouse.up();
  await expect
    .poll(async () => (await host.boundingBox())?.width ?? 0)
    .toBeGreaterThan(widthBeforeSwitch + 100);
  await waitForAnimations(page);
  await workspace.screenshot({ path: `${SHOTS}/05-resized.png` });

  // 5 — one close control collapses the host entirely.
  await page.getByLabel("Close session surface").click();
  await expect(host).toHaveCount(0);
  await waitForAnimations(page);
  await workspace.screenshot({ path: `${SHOTS}/06-collapsed.png` });

  // 6 — on a narrow workspace the same host content appears in one sheet
  // with exactly one close button (the sheet's own).
  await page.setViewportSize({ width: 900, height: 900 });
  await focusTrigger.click();
  await page.getByTestId("coding-session-agent-details-toggle").click();
  const sheet = page.getByRole("dialog");
  await expect(sheet).toContainText("All agents");
  await expect(sheet).toContainText("Codex");
  // Exactly one panel close (the sheet's own); each tab's own close button
  // names its tab ("Close Agents").
  await expect(
    sheet.getByRole("button", { name: "Close", exact: true }),
  ).toHaveCount(1);
  await waitForAnimations(page);
  await page.screenshot({ path: `${SHOTS}/07-narrow-sheet.png` });

  // 7 — the compact focus and recipient controls remain usable at narrow
  // width; both popovers stay inside the viewport instead of creating another
  // horizontal interaction band.
  await sheet.getByRole("button", { name: "Close", exact: true }).click();
  await focusTrigger.click();
  await expect(page.getByTestId("coding-session-agent-focus-chip")).toHaveCount(
    2,
  );
  const focusPopover = page.getByText("Read this session").locator("..");
  const focusBox = await focusPopover.boundingBox();
  if (!focusBox) throw new Error("focus popover geometry missing");
  expect(focusBox.x).toBeGreaterThanOrEqual(0);
  expect(focusBox.x + focusBox.width).toBeLessThanOrEqual(900);
  await page.keyboard.press("Escape");

  const recipient = page.getByTestId(
    "coding-session-participant-picker-trigger",
  );
  await recipient.click();
  await expect(
    page.getByTestId("coding-session-participant-execution"),
  ).toHaveCount(2);
  const recipientPopover = page
    .getByText("Send to", { exact: true })
    .locator("..");
  const recipientBox = await recipientPopover.boundingBox();
  if (!recipientBox) throw new Error("recipient popover geometry missing");
  expect(recipientBox.x).toBeGreaterThanOrEqual(0);
  expect(recipientBox.x + recipientBox.width).toBeLessThanOrEqual(900);
  await waitForAnimations(page);
  await page.screenshot({ path: `${SHOTS}/08-narrow-recipient.png` });
});

test("an ultrawide multi-agent session opens Agents beside the narrative", async ({
  page,
}) => {
  await page.setViewportSize({ width: 2560, height: 1100 });
  const workspace = await openUmbrellaWorkspace(page);
  const host = page.getByTestId("coding-session-surface-host");
  await expect(host).toBeVisible();
  await expect(host).toContainText("All agents");
  await expect(
    page.getByTestId("coding-session-umbrella-timeline"),
  ).toBeVisible();
  await waitForAnimations(page);
  await workspace.screenshot({ path: `${SHOTS}/09-ultrawide-agents.png` });
});
