import { createHash } from "node:crypto";

import { expect, test, type Locator, type Page } from "@playwright/test";
import { finalizeEvent, getPublicKey } from "nostr-tools/pure";

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

// Imported by coding-session-sv78-background-task.spec.ts, which registers it.
// D12 stabilization, lane shell-rail: Brian's installed-build audit
// (session becbd0cb, 2026-10-06). One idle Lead whose transcript copies the
// published shapes: a Bash result as claude-agent-acp 0.84.0 sends it — plain
// text in a `console` fence, no stdout envelope (eventSeq 58) — and a
// markdown final answer.
//
// SV-92: the successful Bash call shows its output, unfenced.
// SV-95: the Agents card's latest activity is plain words, not markdown.
// SV-96: an idle Lead reads "1 idle", its dot is not live, "0/1 settled".

const SHOTS = "test-results/session-parity-d12";
const CHANNEL_NAME = "engineering";
const CHANNEL_ID = "1c7e1c02-87bb-5e88-b2da-5a7a9432d0c9";
const SESSION_REF = "d12d12d1-0000-4000-8000-000000000092";

function hexToBytes(value: string): Uint8Array {
  const bytes = new Uint8Array(value.length / 2);
  for (let index = 0; index < bytes.length; index += 1) {
    bytes[index] = Number.parseInt(value.slice(index * 2, index * 2 + 2), 16);
  }
  return bytes;
}

const SECRET = hexToBytes("d12d12d1".repeat(8));
const PROVIDER = getPublicKey(SECRET);
const ACTOR = getPublicKey(hexToBytes("d12d12d2".repeat(8)));
const SESSION = {
  driver: "claude-agent-acp",
  instanceId: "d12d12d12d12d12d",
  sessionId: "d1200000-0000-4000-8000-000000000092",
  generation: 1,
};

const COMMAND =
  "python3 -c \"import time; time.sleep(150); print('quiet-done')\"";
const FENCED_RESULT = "```console\nquiet-done\n```";
const ANSWER =
  "**Failed commands:** only `python3 check.py`\n\n- the rest passed";

function metadata(createdAt: number): RelayEvent {
  return finalizeEvent(
    {
      kind: KIND_CODING_SESSION_METADATA,
      created_at: createdAt,
      tags: [
        ["h", CHANNEL_ID],
        ["csm-v", CODING_SESSION_METADATA_TAG_VERSION],
        ["cs-target", buildCodingSessionTargetKey(SESSION)],
        ["csm-key", codingSessionMetadataSemanticKey(SESSION)],
      ],
      content: JSON.stringify({
        schema: BUZZ_CODING_SESSION_METADATA_SCHEMA,
        session: SESSION,
        projectRef: null,
        repoRef: null,
        title: "audit lead",
        agentRef: ACTOR,
        provider: "claude-agent-acp",
        runtime: "claude-agent-acp",
        model: "claude-opus-5-5[1m]",
        status: "idle",
        branch: null,
        capabilities: {
          threadTurnStart: true,
          threadTurnInterrupt: true,
          threadSteer: true,
          context: false,
          diff: false,
          plan: false,
        },
        sessionRef: SESSION_REF,
        role: "lead",
      }),
    },
    SECRET,
  ) as unknown as RelayEvent;
}

function transcriptEvents(nowSeconds: number): RelayEvent[] {
  const items: unknown[] = [
    { kind: "user_prompt", content: "Run the quiet job, then report." },
    {
      kind: "tool_call",
      tool: {
        toolName: "Bash",
        toolKind: "execute",
        toolId: "toolu_bash_quiet",
        input: { command: COMMAND, description: "Run 150-second sleep job" },
      },
    },
    {
      kind: "tool_result",
      toolId: "toolu_bash_quiet",
      toolName: "Bash",
      toolKind: "execute",
      content: FENCED_RESULT,
      isError: false,
    },
    { kind: "assistant_text", text: ANSWER },
    {
      kind: "result",
      subtype: "success",
      isError: false,
      durationMs: 152_000,
      result: "Done.",
    },
  ];
  return items.map((item, index) => {
    const eventSeq = index + 1;
    const createdAt = nowSeconds - 60 + index;
    return finalizeEvent(
      {
        kind: KIND_CODING_SESSION_TRANSCRIPT,
        created_at: createdAt,
        tags: [
          ["h", CHANNEL_ID],
          ["cst-v", CODING_SESSION_TRANSCRIPT_TAG_VERSION],
          ["cs-target", buildCodingSessionTargetKey(SESSION)],
          ["cst-seq", String(eventSeq)],
          ["cst-key", codingSessionTranscriptSemanticKey(SESSION, eventSeq)],
        ],
        content: JSON.stringify({
          schema: BUZZ_CODING_SESSION_TRANSCRIPT_SCHEMA,
          session: SESSION,
          eventSeq,
          timestamp: createdAt * 1_000,
          turnId: "turn-1",
          item,
        }),
      },
      SECRET,
    ) as unknown as RelayEvent;
  });
}

async function openSession(page: Page): Promise<void> {
  await page.setViewportSize({ width: 1440, height: 900 });
  await installMockBridge(page, {
    globalAgentConfig: {
      env_vars: {},
      provider: null,
      model: null,
      "allowed-bridge-pubkeys": [{ pubkey: PROVIDER, label: "Audit provider" }],
    },
  });
  await page.goto("/");
  await page.getByTestId(`channel-${CHANNEL_NAME}`).click();
  const now = Math.floor(Date.now() / 1_000);
  await page.evaluate(
    ({ channelName, events }) => {
      const seed = window.__BUZZ_E2E_SEED_MOCK_SIGNED_EVENT__;
      if (!seed) throw new Error("signed-event seeding hook is missing");
      for (const event of events) seed({ channelName, event });
    },
    {
      channelName: CHANNEL_NAME,
      events: [metadata(now - 90), ...transcriptEvents(now)],
    },
  );
  const trigger = page.getByTestId("channel-coding-sessions-trigger");
  await expect(trigger).toHaveAttribute("aria-label", "Coding sessions (1)", {
    timeout: 15_000,
  });
  await trigger.click();
  await page.getByTestId("channel-coding-session-open").first().click();
}

async function openAgentsSurface(page: Page): Promise<Locator> {
  const orchestration = page.getByTestId("coding-session-agents-orchestration");
  if (!(await orchestration.isVisible())) {
    const launcher = page.getByTestId("coding-session-surface-launcher");
    if (!(await launcher.isVisible())) {
      await page.keyboard.press("ControlOrMeta+Alt+KeyB");
    }
    await page
      .getByTestId("coding-session-surface-launcher-row-agents")
      .click();
  }
  await expect(orchestration).toBeVisible({ timeout: 15_000 });
  return orchestration;
}

test("D12 shell-rail: fenced Bash output, plain-text activity, idle is not active", async ({
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

  await openSession(page);

  // SV-92: open the fold and the Bash row; the fenced plain-text result is
  // the output, without its fence.
  const turn = page.getByTestId("coding-session-turn").last();
  await expect(turn).toContainText("Failed commands:", { timeout: 15_000 });
  const fold = page.getByTestId("coding-session-worked-fold");
  if ((await fold.count()) > 0) await fold.first().click();
  const tool = turn.getByTestId("transcript-tool-item").first();
  const shell = turn.getByTestId("transcript-shell-command");
  if ((await shell.count()) === 0) {
    // The row is a <details>; its <summary> opens it.
    await tool.locator("summary").first().click();
  }
  await expect(shell).toBeVisible();
  await expect(shell.locator("pre")).toHaveText("quiet-done");
  await expect(shell).not.toContainText("```");
  await shoot("SV92-fenced-bash-output", shell);

  // SV-96 and SV-95 on the Agents surface.
  const orchestration = await openAgentsSurface(page);
  const card = orchestration.getByTestId("coding-session-agents-workflow-card");
  await expect(card).toHaveCount(1);
  await expect(
    card.getByTestId("coding-session-agents-workflow-settled"),
  ).toHaveText("0/1 settled");
  const leadToggle = card.getByTestId(
    "coding-session-agents-phase-toggle-role:lead",
  );
  await expect(leadToggle).toContainText("1 idle");
  await expect(leadToggle).not.toContainText("active");
  const leadChip = card.getByTestId(
    "coding-session-agents-phase-chip-role:lead",
  );
  await expect(leadChip).toHaveAttribute("data-state", "idle");
  await expect(
    leadChip.locator(
      '[data-testid="coding-session-agents-dot"][data-state="idle"]',
    ),
  ).toHaveCount(1);
  // The card's header dot is the live dot: never primary over an idle seat.
  await expect(card.locator("header > span").first()).not.toHaveClass(
    /bg-primary/,
  );
  await shoot("SV96-idle-lead-card", card);

  // The participant cards sit beside the orchestration card, not inside it.
  const activity = page
    .getByTestId("coding-session-execution-card-activity")
    .first();
  await expect(activity).toHaveText(
    "Failed commands: only python3 check.py the rest passed",
  );
  await shoot(
    "SV95-plain-activity",
    page.getByTestId("coding-session-execution-card").first(),
  );

  expect(new Set(hashes.values()).size).toBe(hashes.size);
});
