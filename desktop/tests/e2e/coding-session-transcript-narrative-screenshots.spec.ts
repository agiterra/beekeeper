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

const SHOTS = "test-results/coding-session-transcript-narrative";
const secret = generateSecretKey();
const pubkey = getPublicKey(secret);
const channelName = "engineering";
const channelId = "1c7e1c02-87bb-5e88-b2da-5a7a9432d0c9";
const session = {
  driver: "claude-agent-acp",
  instanceId: "fedcba9876543210",
  sessionId: "aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee",
  generation: 1,
};
const targetKey = buildCodingSessionTargetKey(session);

function signed(kind: number, seq: number, content: unknown, tags: string[][]) {
  return finalizeEvent(
    {
      kind,
      created_at: 1_800_100_000 + seq,
      tags: [["h", channelId], ...tags],
      content: JSON.stringify(content),
    },
    secret,
  ) as unknown as RelayEvent;
}

function metadata(): RelayEvent {
  const payload = {
    schema: BUZZ_CODING_SESSION_METADATA_SCHEMA,
    session,
    projectRef: null,
    repoRef: null,
    title: "Make reconnect recovery observable",
    agentRef: null,
    provider: "claude-agent-acp",
    runtime: "claude-agent-acp",
    model: "sonnet",
    status: "completed",
    branch: "feature/reconnect-recovery",
    capabilities: {
      threadTurnStart: true,
      threadTurnInterrupt: true,
      threadSteer: true,
      context: false,
      diff: true,
      plan: true,
    },
  };
  return signed(KIND_CODING_SESSION_METADATA, 0, payload, [
    ["csm-v", CODING_SESSION_METADATA_TAG_VERSION],
    ["cs-target", targetKey],
    ["csm-key", codingSessionMetadataSemanticKey(session)],
  ]);
}

function transcript(seq: number, item: unknown): RelayEvent {
  const payload = {
    schema: BUZZ_CODING_SESSION_TRANSCRIPT_SCHEMA,
    session,
    eventSeq: seq,
    timestamp: 1_800_100_000_000 + seq * 1_000,
    turnId: "busy-turn",
    item,
  };
  return signed(KIND_CODING_SESSION_TRANSCRIPT, seq, payload, [
    ["cst-v", CODING_SESSION_TRANSCRIPT_TAG_VERSION],
    ["cs-target", targetKey],
    ["cst-seq", String(seq)],
    ["cst-key", codingSessionTranscriptSemanticKey(session, seq)],
  ]);
}

function events(): RelayEvent[] {
  const rows: RelayEvent[] = [
    metadata(),
    transcript(1, {
      kind: "user_prompt",
      content: "Trace the reconnect failure, fix it, and verify recovery.",
    }),
    transcript(2, {
      kind: "assistant_text",
      text: "I’ll trace the lifecycle first, then make the smallest fix and verify it.",
    }),
  ];
  const tools = [
    ["Read", "desktop/src/features/sessions/useReconnect.ts"],
    ["Grep", "reconnect attempts"],
    ["Bash", "git diff --stat"],
    ["Edit", "useReconnect.ts"],
    ["Bash", "pnpm test reconnect"],
    ["Bash", "pnpm exec tsc --noEmit"],
  ];
  let seq = 3;
  for (const [toolName, detail] of tools) {
    const toolId = `tool-${seq}`;
    rows.push(
      transcript(seq++, {
        kind: "tool_call",
        tool: { toolName, toolId, input: { command: detail } },
      }),
      transcript(seq++, {
        kind: "tool_result",
        toolId,
        toolName,
        content: `${detail}: complete`,
        isError: false,
      }),
    );
  }
  rows.push(
    transcript(seq++, {
      kind: "tool_call",
      tool: {
        toolName: "Bash",
        toolId: "failed-check",
        input: { command: "pnpm test reconnect:e2e" },
      },
    }),
    transcript(seq++, {
      kind: "tool_result",
      toolId: "failed-check",
      toolName: "Bash",
      content: "Browser fixture unavailable; unit verification remains green.",
      isError: true,
    }),
    transcript(seq++, {
      kind: "assistant_text",
      text: "Reconnect recovery is now bounded and observable. Unit and type checks pass; the unavailable browser fixture remains called out above.",
    }),
    transcript(seq, {
      kind: "result",
      subtype: "success",
      isError: false,
      durationMs: 76_400,
      result: "Reconnect recovery is now bounded and observable.",
      costUsd: 0.84,
    }),
  );
  return rows;
}

function planEvents(): RelayEvent[] {
  return [
    metadata(),
    transcript(1, {
      kind: "user_prompt",
      content: "Make reconnect recovery observable.",
    }),
    transcript(2, {
      kind: "plan",
      entries: [
        { content: "Trace the reconnect lifecycle", status: "in_progress" },
        { content: "Bound retry state", status: "pending" },
        { content: "Add focused verification", status: "pending" },
        { content: "Capture the final UI", status: "pending" },
      ],
    }),
    transcript(3, {
      kind: "assistant_text",
      text: "The lifecycle trace identified stale retry state after the socket closes.",
    }),
    transcript(4, {
      kind: "plan",
      entries: [
        { content: "Trace the reconnect lifecycle", status: "completed" },
        { content: "Bound retry state", status: "completed" },
        { content: "Add focused verification", status: "in_progress" },
        { content: "Capture the final UI", status: "pending" },
      ],
    }),
  ];
}

async function openSeededSession(
  page: import("@playwright/test").Page,
  seeded: RelayEvent[],
) {
  await page.setViewportSize({ width: 1440, height: 900 });
  await installMockBridge(page, {
    globalAgentConfig: {
      env_vars: {},
      provider: null,
      model: null,
      "allowed-bridge-pubkeys": [{ pubkey, label: "Screenshot provider" }],
    },
  });
  await page.goto("/");
  await page.getByTestId(`channel-${channelName}`).click();
  await page.evaluate(
    ({ channelName: name, events: signedEvents }) => {
      const seed = window.__BUZZ_E2E_SEED_MOCK_SIGNED_EVENT__;
      if (!seed) throw new Error("signed-event seeding hook is missing");
      for (const event of signedEvents) seed({ channelName: name, event });
    },
    { channelName, events: seeded },
  );

  const trigger = page.getByTestId("channel-coding-sessions-trigger");
  await expect(trigger).toHaveAttribute("aria-label", "Coding sessions (1)", {
    timeout: 15_000,
  });
  await trigger.click();
  await page.getByTestId("channel-coding-session-open").click();
  return page.getByTestId("coding-session-workspace");
}

test("captures the busy transcript narrative collapsed and expanded", async ({
  page,
}) => {
  const workspace = await openSeededSession(page, events());
  const disclosure = page.getByText("+3 previous tool calls");
  await expect(disclosure).toBeVisible();
  await expect(
    page.getByText("Tool call failed", { exact: false }),
  ).toBeVisible();
  await waitForAnimations(page);
  await workspace.screenshot({ path: `${SHOTS}/01-collapsed.png` });

  await disclosure.click();
  await expect(page.getByText("Show fewer tool calls")).toBeVisible();
  await waitForAnimations(page);
  await workspace.screenshot({ path: `${SHOTS}/02-expanded.png` });
});

test("captures the latest plan snapshot collapsed and expanded", async ({
  page,
}) => {
  const workspace = await openSeededSession(page, planEvents());
  const plan = page.getByTestId("coding-session-inline-plan");
  await expect(plan).toHaveCount(1);
  await expect(plan).toContainText("Add focused verification");
  await expect(plan).toContainText("2/4");
  await waitForAnimations(page);
  await workspace.screenshot({ path: `${SHOTS}/03-plan-collapsed.png` });

  await plan.locator("summary").click();
  await expect(page.getByRole("list", { name: "Plan steps" })).toBeVisible();
  await waitForAnimations(page);
  await workspace.screenshot({ path: `${SHOTS}/04-plan-expanded.png` });
});
