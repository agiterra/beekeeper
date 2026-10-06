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
import { TEST_IDENTITIES, installMockBridge } from "../helpers/bridge";

// Session-view parity, wave B, stage 1 — lane audit-markdown: the Wave A
// markdown and font IDs (SV-04, SV-09..SV-12) re-audited against T3 Code.
// Every shot is scoped to its subject with `locator.screenshot`, and the set
// is gated on distinct hashes (plans/SESSION_VIEW_PARITY_PLAN.md rule 4).
// SV-12's acceptance is "the same block in a channel and in a session", so
// one fence is rendered in both places.

const SHOTS = "test-results/session-parity-b";
const secret = generateSecretKey();
const pubkey = getPublicKey(secret);
const sessionChannel = "engineering";
const sessionChannelId = "1c7e1c02-87bb-5e88-b2da-5a7a9432d0c9";
const chatChannel = "random";
const session = {
  driver: "claude-agent-acp",
  instanceId: "a1b2c3d4e5f60719",
  sessionId: "dddddddd-eeee-ffff-0000-222222222222",
  generation: 1,
};
const targetKey = buildCodingSessionTargetKey(session);

/** One fence, rendered in a session answer and in a channel message (SV-12). */
const CODE_FENCE = [
  '```ts title="retry.ts"',
  "export function resetRetry(state: RetryState): RetryState {",
  "  return { ...state, attempts: 0, nextDelayMs: INITIAL_DELAY_MS, lastError: null, reason: 'socket closed by the relay' };",
  "}",
  "```",
].join("\n");

const ANSWER = [
  "All four checks are in, and they agree on where reconnect recovery stalls.",
  "",
  "## What the checks measured",
  "",
  "### 1. The retry state outlives the socket",
  "",
  "- **Changing facts break the counter.** A close never cleared `attempts`, so the next connect waited the longest backoff.",
  "- **Bounded backoff fixes the stall.** The cap is `nextDelayMs` at thirty seconds, see `{commit, path, content hash}` for the record.",
  "",
  "| What the field says works | Where Beekeeper has it, or the gap |",
  "| :--- | ---: |",
  "| A verbatim log as the baseline, the plain transcript every reviewer reads first and last | **Have it**: the signed transcript (`44225`) |",
  "| Work state as the main memory | **Missing**: an event recording which memory ids went into which turn |",
  "",
  CODE_FENCE,
].join("\n");

function signed(kind: number, seq: number, content: unknown, tags: string[][]) {
  return finalizeEvent(
    {
      kind,
      created_at: 1_800_500_000 + seq,
      tags: [["h", sessionChannelId], ...tags],
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
      title: "Audit the markdown renderer",
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

function transcript(seq: number, item: unknown): RelayEvent {
  return signed(
    KIND_CODING_SESSION_TRANSCRIPT,
    seq,
    {
      schema: BUZZ_CODING_SESSION_TRANSCRIPT_SCHEMA,
      session,
      eventSeq: seq,
      timestamp: 1_800_500_000_000 + seq * 1_000,
      turnId: "audit-turn",
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
  return [
    metadata(),
    transcript(1, { kind: "user_prompt", content: "Summarize the checks." }),
    transcript(2, { kind: "assistant_text", text: ANSWER }),
    transcript(3, {
      kind: "result",
      subtype: "success",
      isError: false,
      durationMs: 12_000,
      result: "Summarized.",
      costUsd: 0.05,
    }),
  ];
}

async function waitForMockLiveSubscription(page: Page, channelName: string) {
  await expect
    .poll(
      () =>
        page.evaluate(
          ({ ch }) =>
            (
              window as Window & {
                __BEEKEEPER_E2E_HAS_MOCK_LIVE_SUBSCRIPTION__?: (input: {
                  channelName: string;
                }) => boolean;
              }
            ).__BEEKEEPER_E2E_HAS_MOCK_LIVE_SUBSCRIPTION__?.({
              channelName: ch,
            }) ?? false,
          { ch: channelName },
        ),
      { timeout: 20_000 },
    )
    .toBe(true);
}

async function openSession(page: Page): Promise<Locator> {
  await page.getByTestId(`channel-${sessionChannel}`).click();
  await page.evaluate(
    ({ channelName, signedEvents }) => {
      const seed = window.__BEEKEEPER_E2E_SEED_MOCK_SIGNED_EVENT__;
      if (!seed) throw new Error("signed-event seeding hook is missing");
      for (const event of signedEvents) seed({ channelName, event });
    },
    { channelName: sessionChannel, signedEvents: events() },
  );
  const trigger = page.getByTestId("channel-coding-sessions-trigger");
  await expect(trigger).toHaveAttribute("aria-label", "Coding sessions (1)", {
    timeout: 15_000,
  });
  await trigger.click();
  await page.getByTestId("channel-coding-session-open").click();
  const workspace = page.getByTestId("coding-session-workspace");
  await expect(workspace).toContainText("What the checks measured");
  return workspace;
}

test("audits SV-04 and SV-09..SV-12 against T3, hash-distinct", async ({
  page,
}) => {
  test.setTimeout(120_000);
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

  const hashes = new Map<string, string>();
  const shoot = async (name: string, locator: Locator) => {
    await expect(locator).toBeVisible();
    await waitForAnimations(page);
    const png = await locator.screenshot({ path: `${SHOTS}/${name}.png` });
    hashes.set(name, createHash("sha256").update(png).digest("hex"));
  };
  const rest = () => page.mouse.move(2, 2);

  // ── SV-12 in a channel: the fence as an ordinary chat message. ─────────
  await page.getByTestId(`channel-${chatChannel}`).click();
  await waitForMockLiveSubscription(page, chatChannel);
  await page.evaluate(
    ({ content, author }) => {
      window.__BEEKEEPER_E2E_EMIT_MOCK_MESSAGE__?.({
        channelName: "random",
        content,
        pubkey: author,
      });
    },
    { content: CODE_FENCE, author: TEST_IDENTITIES.alice.pubkey },
  );
  const channelBlock = page
    .getByTestId("message-row")
    .filter({ hasText: "resetRetry" })
    .locator("[data-code-block]");
  await expect(channelBlock).toHaveAttribute("data-wrap", "true");
  await expect(
    channelBlock.locator('[data-code-block-title="retry.ts"]'),
  ).toBeVisible();
  await expect(channelBlock.getByTestId("code-block-copy")).toHaveCSS(
    "opacity",
    "1",
  );
  await rest();
  await shoot("SV-12-channel", channelBlock);

  // ── The session. ──────────────────────────────────────────────────────
  const workspace = await openSession(page);
  // The run's result text ("Summarized.") renders as its own message, so the
  // answer under audit is picked by its words.
  const answer = page
    .getByTestId("coding-session-assistant-message")
    .filter({ hasText: "All four checks are in" });
  await expect(answer).toBeVisible();

  // SV-04 (D1): the system UI face, on the shell and inside the answer.
  for (const target of [page.locator("body"), answer.locator("h2").first()]) {
    const family = await target.evaluate(
      (element) => getComputedStyle(element).fontFamily,
    );
    // Chromium reports the declared `BlinkMacSystemFont` as "system-ui".
    expect(family).toMatch(/^-apple-system, (BlinkMacSystemFont|"system-ui"),/);
    expect(family).not.toMatch(/Inter/);
  }
  await shoot(
    "SV-04-session-header",
    workspace.getByTestId("coding-session-header"),
  );

  // SV-09: headings a step above the body, bold list leads, document rhythm.
  const h2 = answer.locator("h2").first();
  const paragraph = answer.locator("p").first();
  const [h2Size, bodySize] = await Promise.all([
    h2.evaluate((el) => Number.parseFloat(getComputedStyle(el).fontSize)),
    paragraph.evaluate((el) =>
      Number.parseFloat(getComputedStyle(el).fontSize),
    ),
  ]);
  expect(h2Size).toBeGreaterThan(bodySize);
  await expect(answer.locator("li strong").first()).toHaveCSS(
    "font-weight",
    "700",
  );
  await shoot("SV-09-document", answer);
  await shoot("SV-09-list-leads", answer.locator("ul").first());

  // SV-10: inline code is an inline bordered pill that wraps with the line.
  const pill = answer.locator("li code").first();
  await expect(pill).toHaveCSS("display", "inline");
  await expect(pill).toHaveCSS("border-top-style", "solid");
  await shoot("SV-10-inline-code", answer.locator("li").nth(1));

  // SV-11: header, dividers, scroll; Expand/Collapse; Copy table.
  const table = answer.locator("[data-table-container]");
  await expect(table).toHaveAttribute("data-expanded", "true");
  await expect(table.locator("th").nth(1)).toHaveCSS("text-align", "right");
  await shoot("SV-11-expanded", table);
  await table.getByTestId("markdown-table-cells-toggle").click();
  await expect(table).toHaveAttribute("data-expanded", "false");
  await expect(table.locator("td").first()).toHaveCSS(
    "text-overflow",
    "ellipsis",
  );
  await rest();
  await shoot("SV-11-collapsed", table);
  await table.getByTestId("markdown-table-copy").click();
  const copyMenu = page.getByRole("menu");
  await expect(copyMenu).toContainText("Copy as Markdown");
  await expect(copyMenu).toContainText("Copy as CSV");
  await shoot("SV-11-copy-menu", copyMenu);
  await page.keyboard.press("Escape");
  await expect(copyMenu).toBeHidden();

  // SV-12 in the session: the same block, the same chrome.
  const sessionBlock = answer.locator("[data-code-block]");
  await expect(sessionBlock).toHaveAttribute("data-wrap", "true");
  await expect(
    sessionBlock.locator('[data-code-block-title="retry.ts"]'),
  ).toBeVisible();
  await expect(sessionBlock.getByTestId("code-block-wrap-toggle")).toHaveCSS(
    "opacity",
    "1",
  );
  await expect(sessionBlock.locator("[data-line]").first()).toHaveCSS(
    "font-weight",
    "400",
  );
  await rest();
  await shoot("SV-12-session", sessionBlock);
  await sessionBlock.getByTestId("code-block-wrap-toggle").click();
  await expect(sessionBlock).toHaveAttribute("data-wrap", "false");
  await rest();
  await shoot("SV-12-session-scrolling", sessionBlock);

  // Rule 4: no two states proven by the same pixels.
  const seen = new Map<string, string>();
  for (const [name, hash] of hashes) {
    expect(
      seen.get(hash),
      `${name} is byte-identical to ${seen.get(hash)}`,
    ).toBeUndefined();
    seen.set(hash, name);
  }
  expect(hashes.size).toBe(10);
});
