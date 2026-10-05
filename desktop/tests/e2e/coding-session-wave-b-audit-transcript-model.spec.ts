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

// Session-view parity, wave B audit of the transcript model (lane
// audit-transcript-model): the text the model hands the view for SV-02 (a
// failed step folds, muted, and the fold sentence names it), SV-03 (Claude's
// Bash counts as a command, in T3's words) and SV-06 ("Ran N subagents").
// Every shot is scoped to its subject with `locator.screenshot`, and the set
// is gated on distinct hashes.

const SHOTS = "test-results/session-parity-b";
const secret = generateSecretKey();
const pubkey = getPublicKey(secret);
const channelName = "engineering";
const channelId = "1c7e1c02-87bb-5e88-b2da-5a7a9432d0c9";
const session = {
  driver: "claude-agent-acp",
  instanceId: "c1c2c3d4e5f6071a",
  sessionId: "ffffffff-0000-1111-2222-333333333333",
  generation: 1,
};
const targetKey = buildCodingSessionTargetKey(session);

const ANSWER =
  "All five checks ran: the failing script was the one that does not exist, and the rest pass.";

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
      title: "Run the temperature checks",
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

/**
 * One settled turn: two subagents, five Claude `Bash` calls (no `toolKind`,
 * so only the tool name says each ran a command) of which one fails on the
 * way, then the answer.
 */
function events(): RelayEvent[] {
  const turn = "model-audit-turn";
  let seq = 1;
  const rows: RelayEvent[] = [
    metadata(),
    transcript(seq++, turn, {
      kind: "user_prompt",
      content: "Run the temperature checks and have two reviewers look.",
    }),
  ];
  for (const [toolId, description] of [
    ["task-1", "Review temperature.py edge cases"],
    ["task-2", "Review the test names"],
  ]) {
    rows.push(
      transcript(seq++, turn, {
        kind: "tool_call",
        tool: {
          toolName: "Task",
          toolKind: "think",
          toolId,
          input: { description, prompt: "Look", subagent_type: "Explore" },
        },
      }),
      transcript(seq++, turn, {
        kind: "tool_result",
        toolId,
        toolName: "Task",
        content: "Looks right.",
        isError: false,
      }),
    );
  }
  for (const [command, failed] of [
    ["python3 -m pytest demo/", false],
    ["python3 demo/temperature.py 100", false],
    ["python3 demo/does_not_exist.py", true],
    ["python3 -m pytest demo/ -k edge", false],
    ["python3 -m pyflakes demo/", false],
  ] as const) {
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
        content: failed
          ? "Exit code 2\npython3: can't open file 'demo/does_not_exist.py': [Errno 2] No such file or directory"
          : `${command}: ok`,
        isError: failed,
      }),
    );
  }
  rows.push(
    transcript(seq++, turn, { kind: "assistant_text", text: ANSWER }),
    transcript(seq, turn, {
      kind: "result",
      subtype: "success",
      isError: false,
      durationMs: 80_000,
      result: ANSWER,
      costUsd: 0.12,
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
      "allowed-bridge-pubkeys": [{ pubkey, label: "Model audit provider" }],
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
  await expect(workspace).toContainText("All five checks ran");
  return workspace;
}

test("captures the transcript model's SV-02, SV-03 and SV-06 text, hash-distinct", async ({
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

  // SV-03: five Claude Bash calls read as commands, in T3's words, and the
  // folded failure is named, muted, after them (SV-02).
  const foldRow = page.getByTestId("coding-session-worked-fold-row");
  await expect(foldRow).toContainText("Ran 5 commands");
  await expect(foldRow).toContainText("1 step failed");
  await expect(foldRow).not.toContainText("tool calls");
  await rest();
  await shoot("SV-03-commands-sentence", foldRow);

  // SV-02: opened, the failed step sits in place, muted — not the red
  // "Tool call failed" alarm a failed turn keeps.
  await page.getByTestId("coding-session-worked-fold").click();
  // Inside the fold the five commands are one tool group, and its label
  // names the failure ("· 1 failed") while the group is still closed.
  const failedGroup = page
    .getByTestId("coding-session-tool-group")
    .filter({ hasText: "· 1 failed" });
  await expect(failedGroup).toHaveCount(1);
  await failedGroup.getByRole("button").first().click();
  await expect(failedGroup).toHaveAttribute("data-open", "");
  const failedStep = failedGroup
    .getByTestId("transcript-tool-item")
    .filter({ hasText: "does_not_exist.py" });
  await expect(failedStep).toHaveCount(1);
  await expect(failedStep).not.toContainText("Tool call failed");
  await rest();
  await shoot("SV-02-fold-failed-step", failedStep);

  // SV-06: the subagents row reads "Ran 2 subagents" — the batch, not the
  // first spawn's description.
  const subagents = page.getByTestId("coding-session-subagents");
  await expect(subagents).toContainText("Ran 2 subagents");
  await rest();
  await shoot("SV-06-subagents-label", subagents);

  // No two shots proven by the same pixels.
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
