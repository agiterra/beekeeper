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

// SV-78 (ledger 336): Brian's audit session backgrounded `sleep 150`, ended
// its turn at 20:48:56Z, and Claude Code woke it on the task's notification at
// 20:51:18Z. Two shots: the ended turn while the task is still outstanding
// ("Worked for 41s · … · 1 background task running"), then the turn the
// notification woke — its own turn under a wake row, never a prompt bubble —
// with the first turn's clause cleared. Tool-result and notification wording
// is Claude Code 2.1.x's own. Scoped shots, gated on distinct hashes.

const SHOTS = "test-results/sv78-background-task";
const secret = generateSecretKey();
const pubkey = getPublicKey(secret);
const channelName = "engineering";
const channelId = "1c7e1c02-87bb-5e88-b2da-5a7a9432d0c9";
const session = {
  driver: "claude-agent-acp",
  instanceId: "b1b2c3d4e5f60778",
  sessionId: "da8d6582-0000-4000-8000-000000000078",
  generation: 1,
};
const targetKey = buildCodingSessionTargetKey(session);
const TASK = "bqlyvw89h";
const FIRST_ANSWER =
  "Steps 1–5 done. The sleep is running in the background; I'll be notified when it finishes and continue with steps 6–8.";
const SECOND_ANSWER = "Steps 6–8 done: a, b and c each ran once.";

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
      title: "Audit: background wake",
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

/** The prompted turn that backgrounds the sleep and ends. */
function endedTurn(): RelayEvent[] {
  const turn = "turn-audit-1";
  return [
    metadata("idle", 0),
    transcript(1, turn, {
      kind: "user_prompt",
      content: "Run audit steps 1–8. Step 5 sleeps 150 seconds.",
    }),
    transcript(2, turn, {
      kind: "tool_call",
      tool: {
        toolName: "Bash",
        toolId: "bash-sleep",
        input: { command: "sleep 150", run_in_background: true },
      },
    }),
    transcript(3, turn, {
      kind: "tool_result",
      toolId: "bash-sleep",
      toolName: "Bash",
      content: `Command running in background with ID: ${TASK}. Output is being written to: /private/tmp/claude-502/audit/tasks/${TASK}.output. You will be notified when it completes. To check interim output, use Read on that file path.`,
      isError: false,
    }),
    transcript(4, turn, { kind: "assistant_text", text: FIRST_ANSWER }),
    transcript(5, turn, {
      kind: "result",
      subtype: "success",
      isError: false,
      durationMs: 41_000,
      result: "",
      costUsd: 0.04,
    }),
  ];
}

/** The turn the notification woke: nobody prompted it. */
function wokenTurn(): RelayEvent[] {
  const turn = "turn-audit-2";
  return [
    transcript(6, turn, {
      kind: "user_prompt",
      content: [
        "<task-notification>",
        `<task-id>${TASK}</task-id>`,
        "<tool-use-id>toolu_01audit</tool-use-id>",
        `<output-file>/private/tmp/claude-502/audit/tasks/${TASK}.output</output-file>`,
        "<status>completed</status>",
        '<summary>Background command "sleep 150" completed (exit code 0)</summary>',
        "</task-notification>",
      ].join("\n"),
    }),
    transcript(7, turn, {
      kind: "tool_call",
      tool: {
        toolName: "Bash",
        toolId: "bash-a",
        input: { command: "echo a" },
      },
    }),
    transcript(8, turn, {
      kind: "tool_result",
      toolId: "bash-a",
      toolName: "Bash",
      content: "a",
      isError: false,
    }),
    transcript(9, turn, { kind: "assistant_text", text: SECOND_ANSWER }),
    transcript(10, turn, {
      kind: "result",
      subtype: "success",
      isError: false,
      durationMs: 69_000,
      result: "",
      costUsd: 0.03,
    }),
    metadata("idle", 11),
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
      "allowed-bridge-pubkeys": [{ pubkey, label: "Audit provider" }],
    },
  });
  await page.goto("/");
  await page.getByTestId(`channel-${channelName}`).click();
  await seed(page, endedTurn());
  const trigger = page.getByTestId("channel-coding-sessions-trigger");
  await expect(trigger).toHaveAttribute("aria-label", "Coding sessions (1)", {
    timeout: 15_000,
  });
  await trigger.click();
  await page.getByTestId("channel-coding-session-open").click();
  const workspace = page.getByTestId("coding-session-workspace");
  await expect(workspace).toContainText("Steps 1–5 done");
  return workspace;
}

test("SV-78: an ended turn with a running background task, then the turn it woke", async ({
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
  const turns = workspace.getByTestId("coding-session-turn");
  await expect(turns).toHaveCount(1);
  const first = turns.first();

  // The turn ended, but its background task never reported: the Worked-for
  // row says so, always on screen.
  const fold = first.getByTestId("coding-session-worked-fold");
  await expect(fold).toContainText("Worked for 41s");
  const clause = fold.getByTestId("coding-session-turn-background");
  await expect(clause).toContainText("1 background task running");
  await expect(clause).toHaveAttribute(
    "title",
    `Background task ${TASK}: no completion shown in this transcript`,
  );
  await rest();
  await shoot("SV-78-ended-turn-task-running", first);

  // The notification wakes the agent into a turn of its own.
  await seed(page, wokenTurn());
  await expect(turns).toHaveCount(2);
  const woken = turns.nth(1);
  const wake = woken.getByTestId("coding-session-background-wake");
  await expect(wake).toContainText(`Woke on background task ${TASK}`);
  await expect(wake).toContainText("completed");
  await expect(wake).toHaveAttribute("data-opens-turn", "");
  // Nobody typed it: no prompt bubble, no raw XML.
  await expect(woken.getByTestId("coding-session-user-message")).toHaveCount(0);
  await expect(workspace).not.toContainText("<task-notification>");
  // And the first turn no longer claims the task is running.
  await expect(first.getByTestId("coding-session-turn-background")).toHaveCount(
    0,
  );
  await expect(woken).toContainText(SECOND_ANSWER);
  await rest();
  await shoot("SV-78-woken-turn", woken);
  await shoot("SV-78-ended-turn-cleared", first);

  const seen = new Map<string, string>();
  for (const [name, hash] of hashes) {
    expect(
      seen.get(hash),
      `${name} is byte-identical to ${seen.get(hash)}`,
    ).toBeUndefined();
    seen.set(hash, name);
  }
  expect(hashes.size).toBe(3);
});
