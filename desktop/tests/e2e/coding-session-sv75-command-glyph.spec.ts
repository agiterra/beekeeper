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
 * SV-75: on the machine that redacted them, a command's paths read back as
 * themselves — and the amber "only you see this" eye used to follow each one
 * inside the command (`mkdir -p /tmp/bk-view-test 👁 && cd …`). The command
 * now reads intact and the disclosure sits beside the row, once, naming every
 * path it covers. The disclosure stays: only its placement moved.
 */

const SHOTS = "test-results/coding-session-sv75";
const secret = generateSecretKey();
const pubkey = getPublicKey(secret);
const channelName = "engineering";
const channelId = "1c7e1c02-87bb-5e88-b2da-5a7a9432d0c9";
const session = {
  driver: "claude-agent-acp",
  instanceId: "fedcba9876543275",
  sessionId: "75757575-bbbb-cccc-dddd-eeeeeeeeeeee",
  generation: 1,
};
const targetKey = buildCodingSessionTargetKey(session);

const DIR_DIGEST = "7a".repeat(32);
const SRC_DIGEST = "7b".repeat(32);
const DIR = "/tmp/bk-view-test";
const SRC = "/tmp/bk-view-test/src";
const INTACT = `mkdir -p ${DIR} && cd ${SRC}`;

function marker(bytes: number, digest: string): string {
  return `[elided private context: ${bytes} bytes, sha256:${digest}]`;
}

function signed(kind: number, seq: number, content: unknown, tags: string[][]) {
  return finalizeEvent(
    {
      kind,
      created_at: 1_800_200_000 + seq,
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
      title: "Make a scratch directory",
      agentRef: null,
      provider: "claude-agent-acp",
      runtime: "claude-agent-acp",
      model: "sonnet",
      status: "completed",
      branch: "main",
      capabilities: {
        threadTurnStart: true,
        threadTurnInterrupt: true,
        threadSteer: true,
        context: false,
        diff: true,
        plan: true,
      },
    },
    [
      ["csm-v", CODING_SESSION_METADATA_TAG_VERSION],
      ["cs-target", targetKey],
      ["csm-key", codingSessionMetadataSemanticKey(session)],
    ],
  );
}

function transcript(seq: number, item: unknown): RelayEvent {
  return signed(
    KIND_CODING_SESSION_TRANSCRIPT,
    seq,
    {
      schema: BUZZ_CODING_SESSION_TRANSCRIPT_SCHEMA,
      session,
      eventSeq: seq,
      timestamp: 1_800_200_000_000 + seq * 1_000,
      turnId: "sv75-turn",
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
  // The command as it reaches the wire: both paths redacted before signing.
  const command = `mkdir -p ${marker(19, DIR_DIGEST)} && cd ${marker(23, SRC_DIGEST)}`;
  return [
    metadata(),
    transcript(1, { kind: "user_prompt", content: "Make a scratch dir." }),
    transcript(2, {
      kind: "tool_call",
      tool: { toolName: "Bash", toolId: "sv75-bash", input: { command } },
    }),
    transcript(3, {
      kind: "tool_result",
      toolId: "sv75-bash",
      toolName: "Bash",
      content: "",
      isError: false,
    }),
    transcript(4, { kind: "assistant_text", text: "Scratch directory ready." }),
    transcript(5, {
      kind: "result",
      subtype: "success",
      isError: false,
      durationMs: 3_000,
      result: "Scratch directory ready.",
    }),
  ];
}

const LOCAL_VAULT = {
  [DIR_DIGEST]: { class: "host-path", plaintext: DIR },
  [SRC_DIGEST]: { class: "host-path", plaintext: SRC },
};

test("SV-75: a command reads intact, with one disclosure beside the row", async ({
  page,
}) => {
  await page.setViewportSize({ width: 1440, height: 900 });
  await installMockBridge(page, {
    globalAgentConfig: {
      env_vars: {},
      provider: null,
      model: null,
      "allowed-bridge-pubkeys": [{ pubkey, label: "SV-75 provider" }],
    },
    codingSessionRedactionLocalPubkey: pubkey,
    codingSessionRedactionSessionId: session.sessionId,
    codingSessionRedactionVault: LOCAL_VAULT,
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
  await expect(workspace).toContainText("Scratch directory ready.");

  // A settled turn folds its steps; open the fold if there is one.
  const fold = page.getByTestId("coding-session-worked-fold");
  if ((await fold.count()) > 0) await fold.first().click();

  const row = page
    .getByTestId("transcript-tool-item")
    .filter({ hasText: "mkdir -p" })
    .first();
  await expect(row).toBeVisible({ timeout: 15_000 });
  const summary = row.locator("summary").first();

  // The command is the command: both paths in place, no glyph between them.
  await expect(summary).toContainText(INTACT);
  await expect(summary).not.toContainText("elided private context");

  // The disclosure is still there — once, beside the row, naming both paths.
  const markers = summary.getByTestId("redaction-row-marker");
  await expect(markers).toHaveCount(1);
  await expect(summary.locator("[data-redaction-revealed-badge]")).toHaveCount(
    1,
  );
  await expect(markers).toHaveAttribute(
    "aria-label",
    `Redacted for other viewers — only you see these values: ${DIR}, ${SRC}`,
  );

  await page.mouse.move(2, 2);
  await waitForAnimations(page);
  await row.screenshot({ path: `${SHOTS}/01-command-row.png` });

  await markers.hover();
  const tooltip = page.getByRole("tooltip").first();
  await expect(tooltip).toContainText(DIR);
  await expect(tooltip).toContainText(SRC);
  await waitForAnimations(page);
  const box = await summary.boundingBox();
  if (!box) throw new Error("command row has no box");
  await page.screenshot({
    path: `${SHOTS}/02-disclosure-tooltip.png`,
    clip: {
      x: Math.max(0, box.x - 8),
      y: Math.max(0, box.y - 160),
      width: Math.min(1000, 1440 - Math.max(0, box.x - 8)),
      height: 260,
    },
  });
});
