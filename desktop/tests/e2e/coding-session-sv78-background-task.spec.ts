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
// The D12 shell-rail scenario (SV-92/95/96) registers its test from this
// module: playwright.config.ts was owned by a concurrent batch when it landed,
// so it rides on this registered spec until it gets an entry of its own.
import "./coding-session-audit-shell-rail.scenario";

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
      schema: BEEKEEPER_CODING_SESSION_METADATA_SCHEMA,
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
      schema: BEEKEEPER_CODING_SESSION_TRANSCRIPT_SCHEMA,
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
      const seedEvent = window.__BEEKEEPER_E2E_SEED_MOCK_SIGNED_EVENT__;
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

// SV-91 / SV-93 (ledger 342): the installed-build audit BK-AUDIT-1006 on
// claude-agent-acp 0.84.0, which never forwards the task-notification prompt.
// The only wire evidence of the wake is the provider's status rows: seq 35
// `autonomous_turn_started` opens the next turn, and seq 52 `autonomous_turn:
// … task-notification` lands inside that turn's final answer. Shapes copied
// from the published 44225 events (ids and wording kept, paths elided).

const AUDIT_TASK = "bi9cros3k";
const AUDIT_FIRST = "turn-audit-1006-a";
const AUDIT_WOKEN = "turn-audit-1006-b";

function auditEndedTurn(): RelayEvent[] {
  return [
    metadata("idle", 0),
    transcript(1, AUDIT_FIRST, {
      kind: "user_prompt",
      content:
        "Audit run BK-AUDIT-1006. 6. Run this in the background and end your turn.",
    }),
    transcript(2, AUDIT_FIRST, {
      kind: "assistant_text",
      text: "Still 12. Step 6: starting the 60-second job in the background and ending my turn.",
    }),
    transcript(3, AUDIT_FIRST, {
      kind: "tool_call",
      tool: {
        input: {
          command: `python3 -c "import time; time.sleep(60); print('bg-done')"`,
        },
        toolId: "toolu_01G1xbH6HoXzyH5kKAoJq9RS",
        toolKind: "execute",
        toolName: `python3 -c "import time; time.sleep(60); print('bg-done')"`,
      },
    }),
    transcript(4, AUDIT_FIRST, {
      kind: "tool_result",
      toolId: "toolu_01G1xbH6HoXzyH5kKAoJq9RS",
      toolKind: "execute",
      toolName: `python3 -c "import time; time.sleep(60); print('bg-done')"`,
      input: {
        command: `python3 -c "import time; time.sleep(60); print('bg-done')"`,
        run_in_background: true,
      },
      content: `\`\`\`console\nCommand running in background with ID: ${AUDIT_TASK}. Output is being written to: /private/tmp/claude-502/tasks/${AUDIT_TASK}.output. You will be notified when it completes. To check interim output, use Read on that file path.\n\`\`\``,
      isError: false,
    }),
    transcript(5, AUDIT_FIRST, {
      kind: "assistant_text",
      text: "The 60-second job is running in the background. I'll pick up at step 7 when it finishes.",
    }),
    transcript(6, AUDIT_FIRST, {
      kind: "result",
      subtype: "success",
      result: "completed",
      isError: false,
      durationMs: 32_961,
      costUsd: 0.3958032,
    }),
  ];
}

function auditWokenStart(): RelayEvent[] {
  return [
    transcript(7, AUDIT_WOKEN, {
      kind: "status",
      status: "autonomous_turn_started: the agent began a turn nobody prompted",
    }),
    transcript(8, AUDIT_WOKEN, {
      kind: "assistant_text",
      text: "Background job finished. Step 7: running the 150-second quiet job.",
    }),
  ];
}

function auditWokenEnd(): RelayEvent[] {
  return [
    transcript(9, AUDIT_WOKEN, {
      kind: "assistant_text",
      text: "All nine steps of audit run BK-AUDIT-1006 are done.",
    }),
    transcript(10, AUDIT_WOKEN, {
      kind: "status",
      status: "autonomous_turn: the agent woke on task-notification",
    }),
    transcript(11, AUDIT_WOKEN, {
      kind: "result",
      subtype: "success",
      result: "completed",
      isError: false,
      durationMs: 226_168,
      costUsd: null,
    }),
    metadata("idle", 12),
  ];
}

test("SV-91/SV-93: the audit's background task, the status-row wake, and its marker", async ({
  page,
}) => {
  test.setTimeout(90_000);
  const shots = "test-results/sv91-sv93-autonomous-wake";
  const hashes = new Map<string, string>();
  const shoot = async (name: string, locator: Locator) => {
    await expect(locator).toBeVisible();
    await waitForAnimations(page);
    const png = await locator.screenshot({ path: `${shots}/${name}.png` });
    hashes.set(name, createHash("sha256").update(png).digest("hex"));
  };
  const rest = () => page.mouse.move(2, 2);

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
  await seed(page, auditEndedTurn());
  const trigger = page.getByTestId("channel-coding-sessions-trigger");
  await expect(trigger).toHaveAttribute("aria-label", "Coding sessions (1)", {
    timeout: 15_000,
  });
  await trigger.click();
  await page.getByTestId("channel-coding-session-open").click();
  const workspace = page.getByTestId("coding-session-workspace");
  await expect(workspace).toContainText("The 60-second job is running");

  const turns = workspace.getByTestId("coding-session-turn");
  const first = turns.first();
  await expect(first.getByTestId("coding-session-turn-background")).toHaveText(
    /1 background task running/,
  );
  await rest();
  await shoot("SV-91-ended-turn-running", first);

  // The provider's wake row arrives; no notification prompt ever does.
  await seed(page, auditWokenStart());
  await expect(turns).toHaveCount(2);
  await expect(first.getByTestId("coding-session-turn-background")).toHaveText(
    /1 background task, then the agent woke on its own/,
  );
  const woken = turns.nth(1);
  const marker = woken.getByTestId("coding-session-autonomous-wake");
  await expect(marker).toContainText("Woke on its own");
  await expect(
    marker.getByTestId("coding-session-autonomous-wake-cause"),
  ).toHaveCount(0);
  await expect(woken.getByTestId("coding-session-background-wake")).toHaveCount(
    0,
  );
  await rest();
  await shoot("SV-93-woken-turn-started", woken);

  // Seq 52's row names the cause; the marker says it, once.
  await seed(page, auditWokenEnd());
  await expect(
    marker.getByTestId("coding-session-autonomous-wake-cause"),
  ).toHaveText("· background task");
  await expect(
    workspace.getByTestId("coding-session-autonomous-wake"),
  ).toHaveCount(1);
  await rest();
  await shoot("SV-93-woken-turn-cause", woken);
  await shoot("SV-91-ended-turn-woke", first);

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
