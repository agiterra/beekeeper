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

// SV-79/SV-82: a subagent opens as its own page in the transcript pane — its
// own items only, headed by a bar (title, type, status, duration, model,
// tokens, tools, "Runs on its own", Open parent) and the prompt it was given —
// and Open parent (or Escape) returns to the parent transcript at its row.
//
// Opening goes through the rows lane's spawn card (SV-80), which calls
// `useOpenCodingSessionSubagent()` with the Task call's toolCallId. Scoped shots, gated on distinct hashes.

const SHOTS = "test-results/sv79-subagent-page";
const secret = generateSecretKey();
const pubkey = getPublicKey(secret);
const channelName = "engineering";
const channelId = "1c7e1c02-87bb-5e88-b2da-5a7a9432d0c9";
const session = {
  driver: "claude-agent-acp",
  instanceId: "b1b2c3d4e5f60779",
  sessionId: "da8d6582-0000-4000-8000-000000000079",
  generation: 1,
};
const targetKey = buildCodingSessionTargetKey(session);
const SUBAGENT_STEP = "Reading crates/buzz-session-provider/src/transcript.rs";
const LEAD_ANSWER = "fit_item has three callers; all bound before publishing.";
const PROMPT =
  "Find every caller of fit_item and say whether it bounds input first.";

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

function metadata(status: string, seq: number): RelayEvent {
  return signed(
    KIND_CODING_SESSION_METADATA,
    seq,
    {
      schema: BUZZ_CODING_SESSION_METADATA_SCHEMA,
      session,
      projectRef: null,
      repoRef: null,
      title: "Audit: subagent page",
      agentRef: null,
      provider: "claude-agent-acp",
      runtime: "claude-agent-acp",
      model: "sonnet",
      status,
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

function transcript(seq: number, turnId: string, item: unknown): RelayEvent {
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

/** One turn: the lead spawns a subagent, which reads a file and reports. */
function spawnTurn(): RelayEvent[] {
  const turn = "turn-subagent-1";
  return [
    metadata("idle", 0),
    transcript(1, turn, {
      kind: "user_prompt",
      content: "Who calls fit_item?",
    }),
    transcript(2, turn, {
      kind: "tool_call",
      tool: {
        toolName: "Map the call sites",
        toolKind: "think",
        toolId: "task-sv79",
        input: {
          description: "Map the call sites",
          prompt: PROMPT,
          subagent_type: "Explore",
        },
      },
    }),
    transcript(3, turn, {
      kind: "assistant_text",
      text: SUBAGENT_STEP,
      parentToolId: "task-sv79",
    }),
    transcript(4, turn, {
      kind: "tool_call",
      tool: {
        toolName: "Read",
        toolKind: "read",
        toolId: "read-sv79",
        input: { path: "crates/buzz-session-provider/src/transcript.rs" },
      },
      parentToolId: "task-sv79",
    }),
    transcript(5, turn, {
      kind: "tool_result",
      toolId: "read-sv79",
      toolName: "Read",
      content: "pub fn fit_item(item: Value) -> Value { … }",
      parentToolId: "task-sv79",
    }),
    transcript(6, turn, {
      kind: "tool_result",
      toolId: "task-sv79",
      toolName: "Task",
      content: "Three callers, all bounded.",
      isError: false,
      subagent: {
        type: "Explore",
        model: "claude-haiku-4-5",
        totalTokens: 48_210,
        durationMs: 9_000,
        toolUseCount: 1,
      },
    }),
    transcript(7, turn, { kind: "assistant_text", text: LEAD_ANSWER }),
    transcript(8, turn, {
      kind: "result",
      subtype: "success",
      isError: false,
      durationMs: 14_000,
      result: "",
      costUsd: 0.02,
    }),
    metadata("idle", 9),
  ];
}

async function seed(page: Page, events: RelayEvent[]) {
  await page.evaluate(
    ({ channelName: name, events: signedEvents }) => {
      const seedEvent = window.__BUZZ_E2E_SEED_MOCK_SIGNED_EVENT__;
      if (!seedEvent) throw new Error("signed-event seeding hook is missing");
      for (const event of signedEvents) seedEvent({ channelName: name, event });
    },
    { channelName, events },
  );
}

async function openSession(page: Page): Promise<Locator> {
  await page.setViewportSize({ width: 1440, height: 900 });
  await installMockBridge(page, {
    globalAgentConfig: {
      env_vars: {},
      provider: null,
      model: null,
      "allowed-bridge-pubkeys": [{ pubkey, label: "Subagent provider" }],
    },
  });
  await page.goto("/");
  await page.getByTestId(`channel-${channelName}`).click();
  await seed(page, spawnTurn());
  const trigger = page.getByTestId("channel-coding-sessions-trigger");
  await expect(trigger).toHaveAttribute("aria-label", "Coding sessions (1)", {
    timeout: 15_000,
  });
  await trigger.click();
  await page.getByTestId("channel-coding-session-open").click();
  const workspace = page.getByTestId("coding-session-workspace");
  await expect(workspace).toContainText(LEAD_ANSWER);
  return workspace;
}

/**
 * The rows lane's opener (SV-80): the spawn's card in the parent transcript —
 * the finish card once settled, the link card while it runs. Both carry the
 * call's item id as `data-subagent-call-id`.
 */
function spawnCard(workspace: Locator): Locator {
  return workspace
    .getByTestId("coding-session-transcript")
    .locator(
      '[data-testid="coding-session-subagent-finish"], [data-testid="coding-session-subagent-link"]',
    )
    .first();
}

async function openSubagentFromRow(workspace: Locator) {
  const card = spawnCard(workspace);
  await expect(card).toBeVisible();
  await card.click();
}

test("SV-79/SV-82: a subagent opens as its own page and Open parent returns", async ({
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

  const workspace = await openSession(page);
  const pane = workspace.getByTestId("coding-session-transcript-pane");
  await rest();
  await shoot("SV-79-parent-before", pane);

  await openSubagentFromRow(workspace);
  const subagentPage = workspace.getByTestId("coding-session-subagent-page");
  await expect(subagentPage).toBeVisible();

  // The bar: what it is, how it ended, what it spent, and the way back.
  const bar = subagentPage.getByTestId("coding-session-subagent-bar");
  await expect(bar).toHaveAttribute("data-status", "done");
  await expect(bar.getByTestId("coding-session-subagent-bar-title")).toHaveText(
    "Map the call sites",
  );
  await expect(bar).toContainText("Explore");
  await expect(
    bar.getByTestId("coding-session-subagent-bar-status"),
  ).toHaveText("Completed");
  await expect(
    bar.getByTestId("coding-session-subagent-bar-elapsed"),
  ).toHaveText("9.0s");
  await expect(bar.getByTestId("coding-session-subagent-bar-meta")).toHaveText(
    "claude-haiku-4-5 · 48.2k tok · 1 tool",
  );
  await expect(bar).toContainText("Runs on its own");

  // SV-82: the prompt it was given comes first.
  const prompt = subagentPage.getByTestId(
    "coding-session-subagent-page-prompt",
  );
  await expect(prompt).toHaveAttribute("data-prompt", "given");
  await expect(prompt).toContainText(PROMPT);

  // Its own items only, drawn with the transcript's components.
  await expect(
    subagentPage.getByTestId("coding-session-transcript"),
  ).toContainText(SUBAGENT_STEP);
  await expect(subagentPage).not.toContainText(LEAD_ANSWER);
  await expect(
    subagentPage.getByTestId("coding-session-subagent-page-result"),
  ).toContainText("Three callers, all bounded.");
  await rest();
  await shoot("SV-79-subagent-page", pane);

  // Open parent returns to the parent transcript, at the subagent's row.
  await bar.getByTestId("coding-session-subagent-open-parent").click();
  await expect(subagentPage).toHaveCount(0);
  await expect(spawnCard(workspace)).toBeInViewport();
  await expect(workspace).toContainText(LEAD_ANSWER);

  // Escape does the same.
  await openSubagentFromRow(workspace);
  await expect(subagentPage).toBeVisible();
  await page.keyboard.press("Escape");
  await expect(subagentPage).toHaveCount(0);

  const seen = new Map<string, string>();
  for (const [name, hash] of hashes) {
    expect(
      seen.get(hash),
      `${name} is byte-identical to ${seen.get(hash)}`,
    ).toBeUndefined();
    seen.set(hash, name);
  }
  expect(hashes.size).toBe(2);
});
