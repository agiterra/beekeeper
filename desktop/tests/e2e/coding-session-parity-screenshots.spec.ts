import { createHash } from "node:crypto";

import { expect, test, type Locator, type Page } from "@playwright/test";
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

// Session-view parity, wave A (plans/SESSION_VIEW_PARITY_PLAN.md rule 4):
// every UI ID gets a screenshot, scoped to its subject with
// `locator.screenshot`, and the set is gated on distinct hashes so two IDs
// can never be "proven" by the same pixels. Files are named `svNN-…` to sit
// beside the T3 references in `plans/archive/2026-10-04-session-parity/ref/`.

const SHOTS = "test-results/coding-session-parity";
const secret = generateSecretKey();
const pubkey = getPublicKey(secret);
const channelName = "engineering";
const channelId = "1c7e1c02-87bb-5e88-b2da-5a7a9432d0c9";
const session = {
  driver: "claude-agent-acp",
  instanceId: "a1b2c3d4e5f60718",
  sessionId: "dddddddd-eeee-ffff-0000-111111111111",
  generation: 1,
};
const targetKey = buildCodingSessionTargetKey(session);

const LONG_PROMPT = Array.from(
  { length: 14 },
  (_, index) =>
    `${index + 1}. Check the reconnect path step ${index + 1} and note what it does.`,
).join("\n");

const ANSWER = [
  "## Reconnect recovery",
  "",
  "The retry state lived in `useReconnect` and was never cleared after the socket closed.",
  "",
  "| File | Change | Tests |",
  "| --- | --- | --- |",
  "| `useReconnect.ts` | clear retry state on close | 4 passed |",
  "| `relaySocket.ts` | bound the backoff | 2 passed |",
  "",
  "```ts",
  "export function resetRetry(state: RetryState): RetryState {",
  "  return { ...state, attempts: 0, nextDelayMs: INITIAL_DELAY_MS };",
  "}",
  "```",
].join("\n");

function signed(kind: number, seq: number, content: unknown, tags: string[][]) {
  return finalizeEvent(
    {
      kind,
      created_at: 1_800_400_000 + seq,
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
      schema: BEEKEEPER_CODING_SESSION_METADATA_SCHEMA,
      session,
      projectRef: null,
      repoRef: null,
      title: "Make reconnect recovery observable",
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
      schema: BEEKEEPER_CODING_SESSION_TRANSCRIPT_SCHEMA,
      session,
      eventSeq: seq,
      timestamp: 1_800_400_000_000 + seq * 1_000,
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
  const turn = "parity-turn";
  let seq = 1;
  const rows: RelayEvent[] = [
    metadata(),
    transcript(seq++, null, {
      kind: "status",
      status: "execution_boundary_enforced",
      reason: "macos-seatbelt",
    }),
    transcript(seq++, turn, { kind: "user_prompt", content: LONG_PROMPT }),
    transcript(seq++, turn, {
      kind: "reasoning",
      text: "The retry counter survives a close; that explains the stall.",
    }),
  ];
  for (const [toolId, description] of [
    ["task-1", "Map the reconnect call sites"],
    ["task-2", "Review the backoff bounds"],
  ]) {
    rows.push(
      transcript(seq++, turn, {
        kind: "tool_call",
        tool: {
          toolName: "Task",
          toolKind: "think",
          toolId,
          input: {
            description,
            prompt: "Look around",
            subagent_type: "Explore",
          },
        },
      }),
      transcript(seq++, turn, {
        kind: "tool_result",
        toolId,
        toolName: "Task",
        content: "Found three call sites.",
        isError: false,
      }),
    );
  }
  for (const command of ["pnpm test reconnect", "pnpm exec tsc --noEmit"]) {
    const toolId = `bash-${seq}`;
    rows.push(
      transcript(seq++, turn, {
        kind: "tool_call",
        tool: { toolName: "Bash", toolId, input: { command } },
      }),
      transcript(seq++, turn, {
        kind: "tool_result",
        toolId,
        toolName: "Bash",
        content: `${command}: ok`,
        isError: false,
      }),
    );
  }
  rows.push(
    transcript(seq++, turn, { kind: "assistant_text", text: ANSWER }),
    transcript(seq, turn, {
      kind: "result",
      subtype: "success",
      isError: false,
      durationMs: 64_000,
      result: "Reconnect recovery is bounded.",
      costUsd: 0.42,
    }),
  );
  return rows;
}

async function openSession(page: Page): Promise<Locator> {
  await page.setViewportSize({ width: 1440, height: 900 });
  await installMockBridge(page, {
    globalAgentConfig: {
      env_vars: {},
      provider: null,
      model: null,
      "allowed-bridge-pubkeys": [{ pubkey, label: "Parity provider" }],
    },
  });
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
  await expect(workspace).toContainText("Reconnect recovery");
  return workspace;
}

test("captures each wave A UI ID, hash-distinct", async ({ page }) => {
  test.setTimeout(90_000);
  const hashes = new Map<string, string>();
  const shoot = async (name: string, locator: Locator) => {
    await expect(locator).toBeVisible();
    await waitForAnimations(page);
    const png = await locator.screenshot({ path: `${SHOTS}/${name}.png` });
    hashes.set(name, createHash("sha256").update(png).digest("hex"));
  };
  // Move the pointer somewhere inert so no hover state leaks between shots.
  const rest = () => page.mouse.move(2, 2);

  const workspace = await openSession(page);
  const answer = page.getByTestId("coding-session-answer-block");

  // SV-15: a long prompt clamps behind "Show full message".
  const toggle = page.getByTestId("coding-session-user-message-toggle");
  await expect(toggle).toContainText("Show full message");
  const userMessage = toggle.locator(
    "xpath=ancestor::*[.//*[@data-testid='coding-session-user-message-body']][1]",
  );
  await shoot("sv15-user-message-clamped", userMessage);

  // SV-01 / SV-07: the fold row fills on hover and shows its time there.
  const foldRow = page.getByTestId("coding-session-worked-fold-row");
  await rest();
  await shoot("sv01-fold-row-rest", foldRow);
  await foldRow.hover();
  await shoot("sv01-fold-row-hover", foldRow);

  // SV-07: copy and time under the answer, on hover.
  await rest();
  await shoot("sv07-answer-rest", answer);
  await answer.hover();
  await expect(page.getByTestId("coding-session-turn-copy")).toBeVisible();
  await shoot("sv07-answer-hover", answer);
  await rest();

  // SV-10 / SV-11 / SV-12: inline code pill, table chrome, code block chrome.
  await shoot(
    "sv10-inline-code",
    answer.locator("p", { has: page.locator("code") }).first(),
  );
  await expect(page.getByTestId("markdown-table-copy")).toBeVisible();
  await shoot(
    "sv11-table",
    page
      .getByTestId("markdown-table-copy")
      .locator("xpath=ancestor::*[.//table][1]"),
  );
  await expect(page.getByTestId("code-block-wrap-toggle")).toBeVisible();
  await shoot(
    "sv12-code-block",
    page.getByTestId("code-block-copy").locator("xpath=ancestor::*[.//pre][1]"),
  );

  // SV-05 / SV-06: inside the fold, the thought row and the subagents row.
  await page.getByTestId("coding-session-worked-fold").click();
  const thought = page.getByTestId("transcript-thought-item");
  await expect(thought).toBeVisible();
  await rest();
  await shoot("sv05-thought", thought);
  const subagents = page.getByTestId("coding-session-subagents");
  await expect(subagents).toContainText("2 subagents");
  await shoot("sv06-subagents-row", subagents);

  // SV-17 / SV-18 / SV-19: the composer's sandbox chip, provider mark and
  // attach control.
  const chip = workspace.getByTestId("coding-session-control-sandbox");
  await expect(chip).toContainText("Sandboxed");
  await shoot("sv17-sandbox-chip", chip);
  await shoot(
    "sv18-provider-chip",
    workspace.getByTestId("coding-session-control-identity"),
  );
  await expect(
    workspace.getByTestId("coding-session-composer-attach"),
  ).toBeVisible();
  await shoot(
    "sv19-composer",
    workspace.getByTestId("coding-session-composer"),
  );

  // Rule 4: no two IDs proven by the same pixels.
  const seen = new Map<string, string>();
  for (const [name, hash] of hashes) {
    expect(
      seen.get(hash),
      `${name} is byte-identical to ${seen.get(hash)}`,
    ).toBeUndefined();
    seen.set(hash, name);
  }
  expect(hashes.size).toBe(13);
});
