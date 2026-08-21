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
import { waitForAnimations } from "../helpers/animations";
import { installMockBridge } from "../helpers/bridge";

/**
 * The shared right-side surface host (Slice 1 of the coding-session UI
 * convergence): one collapsible, resizable panel whose sibling tabs are
 * Agents and Observed changes, with a single sheet presentation on narrow
 * layouts. This spec captures the host's five canonical states.
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

function metadataEvent(
  target: typeof CLAUDE_TARGET,
  runtime: string,
  model: string,
  createdAt: number,
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
        schema: BUZZ_CODING_SESSION_METADATA_SCHEMA,
        session: target,
        projectRef: null,
        repoRef: null,
        title: "Converge the session surfaces",
        agentRef: null,
        provider: target.driver,
        runtime,
        model,
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
        schema: BUZZ_CODING_SESSION_TRANSCRIPT_SCHEMA,
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
    metadataEvent(CLAUDE_TARGET, "claude-agent-acp", "sonnet", BASE_CREATED_AT),
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
        kind: "assistant_text",
        text: "Both rails now share one collapsible surface host.",
      },
      BASE_CREATED_AT + 4,
    ),
    metadataEvent(
      CODEX_TARGET,
      "codex-acp",
      "gpt-5.6-sol",
      BASE_CREATED_AT + 10,
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
      const seed = window.__BUZZ_E2E_SEED_MOCK_SIGNED_EVENT__;
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

test("the surface host opens on Agents, switches, resizes, collapses, and sheets", async ({
  page,
}) => {
  await page.setViewportSize({ width: 1440, height: 900 });
  const workspace = await openUmbrellaWorkspace(page);

  // 1 — open on Agents by default: participants, not panel chrome per rail.
  const host = page.getByTestId("coding-session-surface-host");
  await expect(host).toBeVisible();
  await expect(host).toContainText("All agents");
  await expect(host).toContainText("Claude");
  await expect(host).toContainText("Codex");
  await waitForAnimations(page);
  await workspace.screenshot({ path: `${SHOTS}/01-agents-open.png` });

  // 2 — switching tabs swaps content without closing or resizing the host.
  const widthBeforeSwitch = (await host.boundingBox())?.width ?? 0;
  await page.getByTestId("coding-session-surface-tab-changes").click();
  const changes = page.getByTestId("coding-session-changes-rail");
  await expect(changes).toBeVisible();
  await expect(changes).toContainText("CodingSessionSurfaceHost.tsx");
  await expect(changes).toContainText("Observed in transcript activity");
  expect(Math.round((await host.boundingBox())?.width ?? 0)).toBe(
    Math.round(widthBeforeSwitch),
  );
  await waitForAnimations(page);
  await workspace.screenshot({ path: `${SHOTS}/02-observed-changes.png` });

  // 3 — the host resizes via its separator; the surface only re-flows.
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
  await workspace.screenshot({ path: `${SHOTS}/03-resized.png` });

  // 4 — one close control collapses the host entirely.
  await page.getByLabel("Close session surface").click();
  await expect(host).toHaveCount(0);
  await waitForAnimations(page);
  await workspace.screenshot({ path: `${SHOTS}/04-collapsed.png` });

  // 5 — on a narrow workspace the same host content appears in one sheet
  // with exactly one close button (the sheet's own).
  await page.setViewportSize({ width: 900, height: 900 });
  await page.getByTestId("coding-session-surface-toggle-agents").click();
  const sheet = page.getByRole("dialog");
  await expect(sheet).toContainText("All agents");
  await expect(sheet).toContainText("Codex");
  await expect(sheet.getByRole("button", { name: "Close" })).toHaveCount(1);
  await waitForAnimations(page);
  await page.screenshot({ path: `${SHOTS}/05-narrow-sheet.png` });
});
