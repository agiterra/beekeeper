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

// Session-view parity, wave B audit of the transcript presentation (lane
// audit-transcript-view): SV-01, SV-02, SV-05, SV-06, SV-07 (answer and
// prompt hover), SV-08, SV-15,
// each against its T3 reference in
// `plans/archive/2026-10-04-session-parity/ref/`. Every shot is scoped to its
// subject with `locator.screenshot`, and the set is gated on distinct hashes.

const SHOTS = "test-results/session-parity-b";
const secret = generateSecretKey();
const pubkey = getPublicKey(secret);
const channelName = "engineering";
const channelId = "1c7e1c02-87bb-5e88-b2da-5a7a9432d0c9";
const session = {
  driver: "claude-agent-acp",
  instanceId: "b1b2c3d4e5f60719",
  sessionId: "eeeeeeee-ffff-0000-1111-222222222222",
  generation: 1,
};
const targetKey = buildCodingSessionTargetKey(session);

const LONG_PROMPT = Array.from(
  { length: 14 },
  (_, index) =>
    `${index + 1}. Check the reconnect path step ${index + 1} and note what it does.`,
).join("\n");

const ANSWER =
  "Reconnect now recovers cleanly: the retry state is cleared when the socket closes, and the backoff is bounded.";

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
  const turn = "wave-b-turn";
  let seq = 1;
  const rows: RelayEvent[] = [
    metadata(),
    transcript(seq++, turn, { kind: "user_prompt", content: LONG_PROMPT }),
    transcript(seq++, turn, {
      kind: "reasoning",
      text: "The retry counter survives a close; that explains the stall.",
    }),
    transcript(seq++, turn, {
      kind: "tool_call",
      tool: {
        toolName: "Task",
        toolKind: "think",
        toolId: "task-1",
        input: {
          description: "Review the backoff bounds",
          prompt: "Look around",
          subagent_type: "Explore",
        },
      },
    }),
    transcript(seq++, turn, {
      kind: "tool_result",
      toolId: "task-1",
      toolName: "Task",
      content: "The backoff has no upper bound.",
      isError: false,
    }),
  ];
  // Two commands that pass, then one that fails: the failed step folds with
  // the rest (D2) and reads quiet, with a dimmed red glyph, once opened.
  for (const [command, failed] of [
    ["pnpm test reconnect", false],
    ["pnpm exec tsc --noEmit", false],
    ["python3 scripts/does_not_exist.py", true],
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
          ? JSON.stringify({
              stdout: "",
              stderr: "No such file or directory",
              exitCode: 2,
            })
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
      durationMs: 44_000,
      result: "Reconnect recovery is bounded.",
      costUsd: 0.21,
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
  await expect(workspace).toContainText("Reconnect now recovers");
  return workspace;
}

test("captures the wave B transcript audit, hash-distinct", async ({
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
  // Move the pointer somewhere inert so no hover state leaks between shots.
  const rest = () => page.mouse.move(2, 2);

  await openSession(page);
  const turn = page.getByTestId("coding-session-turn").last();
  const answer = page.getByTestId("coding-session-answer-block");

  // SV-15: a long prompt clamps behind "Show full message" (ref run-t3-1-top).
  const toggle = page.getByTestId("coding-session-user-message-toggle");
  await expect(toggle).toContainText("Show full message");
  await expect(
    page.getByTestId("coding-session-user-message-body"),
  ).toHaveAttribute("data-user-message-clamped", "true");
  await rest();
  await shoot("SV-15-clamped", page.getByTestId("coding-session-user-message"));

  // SV-07: the prompt's send time and copy sit hidden under it and appear on
  // hover, as T3's timeline row does (ref sv07-t3-block-hover-copy-time).
  const prompt = page.getByTestId("coding-session-user-message");
  const promptMeta = page.getByTestId("coding-session-user-message-meta");
  await expect(promptMeta).toHaveCSS("opacity", "0");
  await prompt.hover();
  await expect(promptMeta).toHaveCSS("opacity", "1");
  await expect(
    page.getByTestId("coding-session-user-message-time"),
  ).toBeVisible();
  await expect(
    page.getByTestId("coding-session-user-message-copy"),
  ).toHaveAttribute("aria-label", "Copy message");
  await shoot("SV-07-prompt-hover", prompt);
  await rest();

  // SV-08: the folded turn — the fold row, the hairline under it, and the
  // answer — with no rule above the turn (ref sv08-t3-spacing).
  await expect(page.getByTestId("coding-session-worked-fold-row")).toHaveCSS(
    "border-bottom-style",
    "solid",
  );
  await shoot("SV-08-rhythm", turn);

  // SV-07: copy and time under the answer appear on hover; cost stays
  // reachable in the same line (ref sv07-t3-block-hover-copy-time).
  await answer.hover();
  await expect(page.getByTestId("coding-session-turn-copy")).toBeVisible();
  await expect(page.getByTestId("coding-session-turn-meta")).toHaveCSS(
    "opacity",
    "1",
  );
  await expect(page.getByTestId("coding-session-turn-meta")).toContainText("$");
  await shoot("SV-07-block-hover", answer);
  await rest();

  // SV-02: the fold opened — every step back in place, the failed command
  // quiet with its own glyph dimmed red (ref sv02-t3-fold-open).
  await page.getByTestId("coding-session-worked-fold").click();
  // The three commands are adjacent, so they read as one sentence row whose
  // label names the failure; open it to put each call back in place.
  const commandGroup = turn.getByTestId("coding-session-tool-group");
  await expect(commandGroup).toHaveCount(1);
  await expect(commandGroup).toHaveAttribute("data-count", "3");
  await expect(commandGroup).toContainText("1 failed");
  const commandGroupToggle = commandGroup.getByRole("button").first();
  await commandGroupToggle.click();
  await expect(commandGroupToggle).toHaveAttribute("aria-expanded", "true");
  const failedIcon = turn.locator(
    '[data-testid="transcript-tool-row-icon"][data-failure-tone="quiet"]',
  );
  await expect(failedIcon).toHaveCount(1);
  await expect(failedIcon).toHaveAttribute("aria-label", "Failed");
  await rest();
  await shoot("SV-02-fold-open", turn);

  // SV-01: the subagent row is full width and fills on hover
  // (refs sv01-t3-subagent-row-rest/-hover).
  const subagents = page.getByTestId("coding-session-subagents");
  await expect(subagents).toContainText("1 subagent");
  await subagents.locator("summary").hover();
  await shoot("SV-01-hover", subagents);
  await rest();

  // SV-05: the thought, opened under its brain and "Thought" label
  // (refs sv05-t3-thought, sv05-t3-thought-open).
  const thought = page.getByTestId("transcript-thought-item");
  await expect(page.getByTestId("transcript-thought-label")).toHaveText(
    "Thought",
  );
  await thought.locator("summary").click();
  await expect(thought).toContainText("retry counter survives");
  await rest();
  await shoot("SV-05-thought-open", thought);

  // SV-06: the subagent opened. Where the workspace wires the Agents surface
  // the row opens it and the chevron expands in place; either way the inline
  // expansion is what this shot pins (ref sv06-t3-subagent-open).
  const inlineToggle = subagents.getByTestId(
    "coding-session-subagents-inline-toggle",
  );
  if ((await inlineToggle.count()) > 0) {
    await subagents.locator("summary").hover();
    await inlineToggle.click();
  } else {
    await subagents.locator("summary").click();
  }
  await expect(
    subagents.getByTestId("coding-session-subagent-spawn"),
  ).toBeVisible();
  await rest();
  await shoot("SV-06-subagent-open", subagents);

  // Rule 4: no two IDs proven by the same pixels.
  const seen = new Map<string, string>();
  for (const [name, hash] of hashes) {
    expect(
      seen.get(hash),
      `${name} is byte-identical to ${seen.get(hash)}`,
    ).toBeUndefined();
    seen.set(hash, name);
  }
  expect(hashes.size).toBe(8);
});
