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

// SV-80/SV-81 (lane rows): a subagent's card in the transcript and its row in
// the Agents surface's Direct spawns open the subagent's own page; the card
// carries a hover card (also on keyboard focus) with model, time, status,
// tokens, tools and the result; a finished subagent draws its finish card
// after the group, frozen at its outcome and duration.

const SHOTS = "test-results/subagent-rows";
const secret = generateSecretKey();
const pubkey = getPublicKey(secret);
const channelName = "engineering";
const channelId = "1c7e1c02-87bb-5e88-b2da-5a7a9432d0c9";
const session = {
  driver: "claude-agent-acp",
  instanceId: "c1b2c3d4e5f60719",
  sessionId: "eeeeeeee-ffff-0000-1111-333333333333",
  generation: 1,
};
const targetKey = buildCodingSessionTargetKey(session);
const ANSWER = "The backoff now has an upper bound of thirty seconds.";

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
      title: "Subagent rows open their page",
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
        promptImage: true,
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

function transcript(
  seq: number,
  turnId: string | null,
  item: unknown,
): RelayEvent {
  return signed(
    KIND_CODING_SESSION_TRANSCRIPT,
    seq,
    {
      schema: BUZZ_CODING_SESSION_TRANSCRIPT_SCHEMA,
      session,
      eventSeq: seq,
      timestamp: 1_800_500_000_000 + seq * 1_000,
      turnId,
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
  const turn = "subagent-rows-turn";
  let seq = 1;
  return [
    metadata(),
    transcript(seq++, turn, {
      kind: "user_prompt",
      content: "Check the backoff",
    }),
    transcript(seq++, turn, {
      kind: "tool_call",
      tool: {
        toolName: "Task",
        toolKind: "think",
        toolId: "task-1",
        input: {
          description: "Review the backoff bounds",
          prompt: "Look at the retry loop",
          subagent_type: "Explore",
        },
      },
    }),
    transcript(seq++, turn, {
      kind: "assistant_text",
      text: "Reading the retry loop.",
      parentToolId: "task-1",
    }),
    transcript(seq++, turn, {
      kind: "tool_call",
      parentToolId: "task-1",
      tool: {
        toolName: "Read",
        toolKind: "read",
        toolId: "read-1",
        input: { file_path: "src/retry.ts" },
      },
    }),
    transcript(seq++, turn, {
      kind: "tool_result",
      parentToolId: "task-1",
      toolId: "read-1",
      toolName: "Read",
      content: "export const MAX_BACKOFF = Infinity;",
      isError: false,
    }),
    transcript(seq++, turn, {
      kind: "tool_result",
      toolId: "task-1",
      toolName: "Task",
      content: "The backoff has no upper bound.\nSee src/retry.ts.",
      isError: false,
      subagent: {
        type: "Explore",
        model: "claude-sonnet-4-5",
        totalTokens: 48_200,
        durationMs: 62_000,
        toolUseCount: 1,
      },
    }),
    transcript(seq++, turn, { kind: "assistant_text", text: ANSWER }),
    transcript(seq, turn, {
      kind: "result",
      subtype: "success",
      isError: false,
      durationMs: 70_000,
      result: "Bounded.",
      costUsd: 0.1,
    }),
  ];
}

async function openSession(page: Page): Promise<Locator> {
  await page.setViewportSize({ width: 1440, height: 900 });
  await installMockBridge(page, {
    globalAgentConfig: {
      env_vars: {},
      provider: null,
      model: null,
      "allowed-bridge-pubkeys": [{ pubkey, label: "Rows provider" }],
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
    { channelName, events: events() },
  );
  const trigger = page.getByTestId("channel-coding-sessions-trigger");
  await expect(trigger).toHaveAttribute("aria-label", "Coding sessions (1)", {
    timeout: 15_000,
  });
  await trigger.click();
  await page.getByTestId("channel-coding-session-open").click();
  const workspace = page.getByTestId("coding-session-workspace");
  await expect(workspace).toContainText("upper bound of thirty seconds");
  return workspace;
}

test("subagent cards open the page, carry a hover card and freeze when finished", async ({
  page,
}) => {
  test.setTimeout(90_000);
  const hashes = new Map<string, string>();
  const shoot = async (name: string, locator: Locator) => {
    await expect(locator).toBeVisible();
    await waitForAnimations(page);
    const png = await locator.screenshot({ path: `${SHOTS}/${name}.png` });
    hashes.set(name, createHash("sha256").update(png).digest("hex"));
  };
  const rest = () => page.mouse.move(2, 2);

  await openSession(page);
  const block = page.getByTestId("coding-session-subagents-block");
  await expect(block).toHaveCount(1);

  // SV-81: the settled subagent is drawn as its finish card, frozen at its
  // outcome and the report's duration — no ticking clock.
  const finish = block.getByTestId("coding-session-subagent-finish");
  await expect(finish).toHaveCount(1);
  await expect(finish).toHaveAttribute("data-phase", "finished");
  await expect(finish).toHaveAttribute("data-status", "done");
  await expect(finish).toContainText("Review the backoff bounds");
  await expect(finish).toContainText("Finished");
  const clock = finish.getByTestId("coding-session-subagent-elapsed");
  await expect(clock).toHaveAttribute("data-live", "false");
  await expect(clock).toHaveText("1m 2s");
  await expect(
    finish.getByTestId("coding-session-subagent-status-dot"),
  ).toHaveAttribute("data-status", "done");
  await expect(block.getByTestId("coding-session-subagent-link")).toHaveCount(
    0,
  );
  await rest();
  await shoot("SV81-finish-card", block);

  // SV-81: the hover card — on hover, and on keyboard focus.
  await finish.hover();
  const hover = page.getByTestId("coding-session-subagent-hover-card");
  await expect(hover).toBeVisible();
  await expect(hover).toContainText("claude-sonnet-4-5");
  await expect(hover).toContainText("1m 2s");
  await expect(hover).toContainText("Finished");
  await expect(hover).toContainText("48.2k tok");
  await expect(hover).toContainText("Result: The backoff has no upper bound.");
  await shoot("SV81-hover-card", hover);
  await rest();
  await expect(hover).toHaveCount(0);
  await finish.focus();
  await expect(
    page.getByTestId("coding-session-subagent-hover-card"),
  ).toBeVisible();
  await page.keyboard.press("Escape");

  // SV-80: the card's primary click opens the subagent's page.
  await finish.click();
  const subagentPage = page.getByTestId("coding-session-subagent-page");
  await expect(subagentPage).toBeVisible();
  await expect(
    page.getByTestId("coding-session-subagent-bar-title"),
  ).toContainText("Review the backoff bounds");
  await shoot("SV80-page-from-stream", subagentPage);
  await page.getByTestId("coding-session-subagent-open-parent").click();
  await expect(subagentPage).toHaveCount(0);

  // SV-80: the Agents surface's Direct spawns row opens the same page.
  await page.getByTestId("coding-session-subagents").locator("summary").click();
  const row = page
    .getByTestId("coding-session-agents-direct-spawns")
    .getByTestId("coding-session-agents-spawn-row");
  await expect(row).toHaveCount(1);
  await expect(row).toHaveAttribute("data-parent-tool-id", "task-1");
  await expect(row).toHaveAttribute("data-status", "done");
  await rest();
  await shoot("SV80-direct-spawn-row", row);
  await row.click();
  await expect(page.getByTestId("coding-session-subagent-page")).toBeVisible();
  await expect(
    page.getByTestId("coding-session-subagent-bar-title"),
  ).toContainText("Review the backoff bounds");

  const seen = new Map<string, string>();
  for (const [name, hash] of hashes) {
    expect(
      seen.get(hash),
      `${name} is byte-identical to ${seen.get(hash)}`,
    ).toBeUndefined();
    seen.set(hash, name);
  }
  expect(hashes.size).toBe(4);
});
