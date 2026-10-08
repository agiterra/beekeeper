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

// Session-view parity Wave B, lane B0: the surface registry (SV-38), the
// "Open a surface" launcher and its tabs (SV-21), and dimmed surfaces with
// their reasons (SV-23 mechanics). Every shot is scoped to its subject and
// the set is gated on distinct hashes.

const SHOTS = "test-results/session-parity-b";
const secret = generateSecretKey();
const pubkey = getPublicKey(secret);
const channelName = "engineering";
const channelId = "1c7e1c02-87bb-5e88-b2da-5a7a9432d0c9";
const session = {
  driver: "claude-agent-acp",
  instanceId: "b0b0b0b0b0b0b0b0",
  sessionId: "b0000000-0000-4000-8000-000000000021",
  generation: 1,
};
const targetKey = buildCodingSessionTargetKey(session);

/** Every built-in the Conversation lens lists, with its DB2 letter. */
const BUILT_INS: ReadonlyArray<readonly [string, string]> = [
  ["agents", "A"],
  ["diff", "D"],
  ["terminal", "T"],
  ["files", "F"],
  ["plan", "P"],
  ["landing", "L"],
  ["people", "E"],
  ["pulse", "U"],
  ["browser", "B"],
  ["device", "M"],
];

function signed(kind: number, seq: number, content: unknown, tags: string[][]) {
  return finalizeEvent(
    {
      kind,
      created_at: 1_800_500_000 + seq,
      tags: [["h", channelId], ...tags],
      content: JSON.stringify(content),
    },
    secret,
  ) as unknown as RelayEvent;
}

function metadata(): RelayEvent {
  return signed(
    KIND_CODING_SESSION_METADATA,
    0,
    {
      schema: BUZZ_CODING_SESSION_METADATA_SCHEMA,
      session,
      projectRef: null,
      repoRef: null,
      title: "Registry and launcher",
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
}

function transcript(seq: number, item: unknown): RelayEvent {
  return signed(
    KIND_CODING_SESSION_TRANSCRIPT,
    seq,
    {
      schema: BUZZ_CODING_SESSION_TRANSCRIPT_SCHEMA,
      session,
      eventSeq: seq,
      timestamp: 1_800_500_000_000 + seq * 1_000,
      turnId: "registry-turn",
      item,
    },
    [
      ["cst-v", CODING_SESSION_TRANSCRIPT_TAG_VERSION],
      ["cs-target", targetKey],
      ["cst-seq", String(seq)],
      ["cst-key", codingSessionTranscriptSemanticKey(session, seq)],
    ],
  );
}

function events(): RelayEvent[] {
  return [
    metadata(),
    transcript(1, {
      kind: "user_prompt",
      content: "Fix the reconnect stall and tell me what changed.",
    }),
    transcript(2, {
      kind: "tool_call",
      tool: {
        toolName: "Task",
        toolKind: "think",
        toolId: "task-1",
        input: {
          description: "Map the reconnect call sites",
          prompt: "Look around",
          subagent_type: "Explore",
        },
      },
    }),
    transcript(3, {
      kind: "tool_result",
      toolId: "task-1",
      toolName: "Task",
      content: "Found three call sites.",
      isError: false,
    }),
    transcript(4, {
      kind: "tool_call",
      tool: {
        toolName: "Edit",
        toolKind: "edit",
        toolId: "edit-1",
        input: {
          file_path: "src/useReconnect.ts",
          old_string: "attempts += 1;",
          new_string: "attempts = 0;",
        },
      },
    }),
    transcript(5, {
      kind: "tool_result",
      toolId: "edit-1",
      toolName: "Edit",
      content: "Edited src/useReconnect.ts",
      isError: false,
    }),
    transcript(6, {
      kind: "assistant_text",
      text: "The retry counter now resets when the socket closes.",
    }),
    transcript(7, {
      kind: "result",
      subtype: "success",
      isError: false,
      durationMs: 12_000,
      result: "Reconnect recovery is bounded.",
    }),
  ];
}

/** Open the seeded session; after a reload the relay mock is re-seeded. */
async function openSession(page: Page): Promise<Locator> {
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
  const workspace = page.getByTestId("coding-session-workspace");
  await expect(workspace).toContainText("Registry and launcher");
  return workspace;
}

test("SV-21, SV-23 and SV-38: launcher, tabs, dimmed reasons, a fake surface", async ({
  page,
}) => {
  test.setTimeout(120_000);
  const hashes = new Map<string, string>();
  const shoot = async (name: string, locator: Locator) => {
    await expect(locator).toBeVisible();
    await waitForAnimations(page);
    const png = await locator.screenshot({ path: `${SHOTS}/${name}.png` });
    hashes.set(name, createHash("sha256").update(png).digest("hex"));
  };
  /** The host plus whatever tooltip floats over it, in one clip. */
  const shootHostWithTooltip = async (name: string, tooltip: Locator) => {
    await expect(tooltip).toBeVisible();
    await waitForAnimations(page);
    const hostBox = await page
      .getByTestId("coding-session-surface-host")
      .boundingBox();
    const tipBox = await tooltip.boundingBox();
    if (!hostBox || !tipBox)
      throw new Error("host or tooltip geometry missing");
    const x = Math.min(hostBox.x, tipBox.x);
    const y = Math.min(hostBox.y, tipBox.y);
    const png = await page.screenshot({
      clip: {
        x,
        y,
        width: Math.max(hostBox.x + hostBox.width, tipBox.x + tipBox.width) - x,
        height:
          Math.max(hostBox.y + hostBox.height, tipBox.y + tipBox.height) - y,
      },
      path: `${SHOTS}/${name}.png`,
    });
    hashes.set(name, createHash("sha256").update(png).digest("hex"));
  };
  // Two moves, as a real pointer makes: Radix clears a closing tooltip's
  // "pointer in transit" flag only on a document pointermove after the
  // trigger's pointerleave, so a single synthetic move leaves it set and the
  // next trigger's hover never opens its tooltip.
  const rest = async () => {
    await page.mouse.move(2, 2);
    await page.mouse.move(3, 3);
  };

  await page.setViewportSize({ width: 1440, height: 900 });
  // SV-38: a test-only Memory surface, declared as data before the app loads
  // and read only by a `--mode e2e` build. No product file names it.
  await page.addInitScript(() => {
    (
      window as Window & { __BEEKEEPER_E2E_EXTRA_SURFACES__?: unknown }
    ).__BEEKEEPER_E2E_EXTRA_SURFACES__ = [
      {
        id: "memory",
        label: "Memory",
        shortcut: "Y",
        order: 950,
        badgeCount: 2,
        unavailableReason: "Not built",
      },
    ];
  });
  await installMockBridge(page, {
    globalAgentConfig: {
      env_vars: {},
      provider: null,
      model: null,
      "allowed-bridge-pubkeys": [{ pubkey, label: "Registry provider" }],
    },
  });
  const workspace = await openSession(page);
  const host = page.getByTestId("coding-session-surface-host");
  const transcriptPane = page.getByTestId("coding-session-transcript-pane");
  await expect(host).toHaveCount(0);

  // SV-21: ⌘⌥B opens the right panel on the launcher.
  await page.keyboard.press("ControlOrMeta+Alt+KeyB");
  const launcher = page.getByTestId("coding-session-surface-launcher");
  await expect(launcher).toBeVisible();
  await expect(launcher).toContainText("Open a surface");
  for (const [id, letter] of BUILT_INS) {
    const row = page.getByTestId(`coding-session-surface-launcher-row-${id}`);
    await expect(row).toBeVisible();
    await expect(row.locator("kbd")).toHaveText(letter);
  }
  const memoryRow = page.getByTestId(
    "coding-session-surface-launcher-row-memory",
  );
  await expect(memoryRow.locator("kbd")).toHaveText("Y");
  await rest();
  await shoot("SV21-launcher", host);

  // SV-23: no built-in is dimmed any more — the Browser is live since C4
  // (`session-preview-local.spec.ts`) and the Device since C5
  // (`coding-session-device.spec.ts`). The dimmed-row tooltip is still
  // covered below by the fake surface (SV-38).
  await expect(
    page.getByTestId("coding-session-surface-launcher-row-device"),
  ).not.toHaveAttribute("aria-disabled", "true");
  // SV-38: the fake surface's row, dimmed reason and activity badge.
  await expect(memoryRow).toHaveAttribute("aria-disabled", "true");
  await expect(
    page.getByTestId("coding-session-surface-badge-memory").first(),
  ).toHaveText("2");
  await memoryRow.hover();
  const memoryTip = page
    .getByTestId("coding-session-surface-reason-tooltip-memory")
    .first();
  await expect(memoryTip).toContainText("Not built");
  await shootHostWithTooltip("SV38-fake-surface", memoryTip);
  await rest();
  // An unavailable surface's letter opens nothing.
  await page.keyboard.press("y");
  await expect(launcher).toBeVisible();
  await expect(
    page.getByTestId("coding-session-surface-tab-memory"),
  ).toHaveCount(0);

  // SV-21: D opens Diff as a tab.
  await page.keyboard.press("d");
  const diffTab = page.getByTestId("coding-session-surface-tab-diff");
  await expect(diffTab).toHaveAttribute("aria-selected", "true");
  await expect(launcher).toHaveCount(0);
  await expect(page.getByTestId("coding-session-changes-rail")).toBeVisible();

  // "+" then A adds Agents.
  await page.getByTestId("coding-session-surface-add").click();
  await expect(
    page.getByTestId("coding-session-surface-add-menu"),
  ).toBeVisible();
  await page.keyboard.press("a");
  const agentsTab = page.getByTestId("coding-session-surface-tab-agents");
  await expect(agentsTab).toHaveAttribute("aria-selected", "true");
  await expect(diffTab).toHaveAttribute("aria-selected", "false");
  await rest();
  await shoot("SV21-tabs", host);

  // Closing a tab hands the panel to its neighbour.
  await page.getByTestId("coding-session-surface-tab-close-agents").click();
  await expect(agentsTab).toHaveCount(0);
  await expect(diffTab).toHaveAttribute("aria-selected", "true");

  // Expand hides the transcript; restore brings it back.
  await page.getByTestId("coding-session-surface-expand").click();
  await expect(transcriptPane).toBeHidden();
  await expect(host).toHaveAttribute("data-expanded", "true");
  await rest();
  await shoot("SV21-expanded", workspace);
  await page.getByTestId("coding-session-surface-expand").click();
  await expect(transcriptPane).toBeVisible();

  // ⌘J while expanded restores the panel, so the drawer it opens is seen:
  // the drawer lives under the transcript and never opens invisibly.
  await page.getByTestId("coding-session-surface-expand").click();
  await expect(transcriptPane).toBeHidden();
  await page.keyboard.press("ControlOrMeta+KeyJ");
  await expect(transcriptPane).toBeVisible();
  await expect(host).not.toHaveAttribute("data-expanded", "true");
  await expect(page.getByTestId("coding-session-drawer-host")).toBeVisible();
  await page.keyboard.press("ControlOrMeta+KeyJ");
  await expect(page.getByTestId("coding-session-drawer-host")).toHaveCount(0);

  // A reload reopens the same session with the same tabs.
  await page.getByTestId("coding-session-surface-add").click();
  await page.getByTestId("coding-session-surface-add-agents").click();
  await expect(agentsTab).toHaveAttribute("aria-selected", "true");
  await page.reload();
  await openSession(page);
  await expect(
    page.getByTestId("coding-session-surface-tab-diff"),
  ).toBeVisible();
  await expect(
    page.getByTestId("coding-session-surface-tab-agents"),
  ).toHaveAttribute("aria-selected", "true");

  // Closing the last tab closes the panel (T3); ⌘⌥B brings the launcher back.
  await page.getByTestId("coding-session-surface-tab-close-agents").click();
  await page.getByTestId("coding-session-surface-tab-close-diff").click();
  await expect(host).toHaveCount(0);
  await page.keyboard.press("ControlOrMeta+Alt+KeyB");
  await expect(launcher).toBeVisible();

  // Typing D in the composer opens nothing: it is a typing context.
  const textarea = page
    .getByTestId("coding-session-composer")
    .locator("textarea");
  await expect(textarea).toBeEditable();
  await textarea.focus();
  await page.keyboard.press("d");
  await expect(textarea).toHaveValue("d");
  await expect(launcher).toBeVisible();
  await expect(page.getByTestId("coding-session-surface-tab-diff")).toHaveCount(
    0,
  );
  await textarea.fill("");

  // ⌘⌥B hides the panel; ⌘J toggles the drawer surface.
  await page.keyboard.press("ControlOrMeta+Alt+KeyB");
  await expect(host).toHaveCount(0);
  const drawer = page.getByTestId("coding-session-drawer-host");
  await expect(drawer).toHaveCount(0);
  await page.keyboard.press("ControlOrMeta+KeyJ");
  await expect(drawer).toBeVisible();
  await expect(drawer).toContainText("Terminal");
  // The drawer opens beneath the composer, never over it (T3's
  // ThreadTerminalDrawer): the textarea stays visible and the two boxes do
  // not intersect.
  await expect(textarea).toBeVisible();
  const composerBox = await page
    .getByTestId("coding-session-composer-dock")
    .boundingBox();
  const drawerBox = await drawer.boundingBox();
  if (!composerBox || !drawerBox) {
    throw new Error("composer or drawer geometry missing");
  }
  expect(composerBox.y + composerBox.height).toBeLessThanOrEqual(
    drawerBox.y + 1,
  );
  // ⌘J on a session route is the drawer's alone: the channel terminal
  // (Substrate) stays closed (DB10).
  await expect(
    page.locator(
      '[data-terminal-owner="terminal"][data-terminal-mode="docked"]',
    ),
  ).toHaveCount(0);
  await rest();
  await shoot("SV21-drawer", transcriptPane);
  await page.keyboard.press("ControlOrMeta+KeyJ");
  await expect(drawer).toHaveCount(0);
  await expect(page.getByTestId("coding-session-minimap-slot")).toBeAttached();

  // Every shot proves a different state.
  const unique = new Set(hashes.values());
  expect(
    unique.size,
    `screenshot hashes must be distinct: ${JSON.stringify([...hashes])}`,
  ).toBe(hashes.size);
  expect([...hashes.keys()].sort()).toEqual([
    "SV21-drawer",
    "SV21-expanded",
    "SV21-launcher",
    "SV21-tabs",
    "SV38-fake-surface",
  ]);
});
