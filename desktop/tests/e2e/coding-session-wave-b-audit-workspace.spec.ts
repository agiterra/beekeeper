import { createHash } from "node:crypto";
import { readFileSync } from "node:fs";

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

// Session-view parity, Wave B stage 1, lane audit-workspace-composer: the
// Wave A workspace and composer IDs re-audited against T3 (SV-13, SV-14,
// SV-16, SV-17, SV-18, SV-19) and the SV-42 follow-up. Each shot is scoped to
// its subject, and the last test refuses two byte-identical PNGs (plan rule 4).

const SHOTS = "test-results/session-parity-b";
const SHOT_NAMES = [
  "SV-13-width",
  "SV-14-pill",
  "SV-16-details",
  "SV-17-access-open",
  "SV-18-chip",
  "SV-19-composer",
  "SV-42-idle-unfinished",
] as const;

const secret = generateSecretKey();
const pubkey = getPublicKey(secret);
const channelName = "engineering";
const channelId = "1c7e1c02-87bb-5e88-b2da-5a7a9432d0c9";
const session = {
  driver: "claude-agent-acp",
  instanceId: "b2c3d4e5f6071819",
  sessionId: "eeeeeeee-ffff-0000-1111-222222222222",
  generation: 1,
};
const targetKey = buildCodingSessionTargetKey(session);

const ANSWER = Array.from(
  { length: 18 },
  (_, index) =>
    `Paragraph ${index + 1}: the reconnect path keeps its retry state between attempts, so the backoff never resets after a clean close and the next connect waits for the full ceiling instead of the initial delay.`,
).join("\n\n");

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

/** The provider's signed status is `idle`: the session is resting (SV-42). */
function metadata(): RelayEvent {
  return signed(
    KIND_CODING_SESSION_METADATA,
    0,
    {
      schema: BUZZ_CODING_SESSION_METADATA_SCHEMA,
      session,
      projectRef: null,
      repoRef: null,
      title: "Bound the reconnect backoff",
      agentRef: null,
      provider: "claude-agent-acp",
      runtime: "claude-agent-acp",
      model: "claude-opus-5",
      status: "idle",
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
 * Two turns. The first answers at length (enough to scroll) and completes.
 * The second starts a command that never reports an end, and the session's
 * signed status is `idle`: that call did not finish (SV-42).
 */
function events(): RelayEvent[] {
  let seq = 1;
  return [
    metadata(),
    transcript(seq++, null, {
      kind: "status",
      status: "session_fresh",
      reason: "no_prior_execution",
    }),
    transcript(seq++, null, {
      kind: "status",
      status: "execution_boundary_not_enforced",
      reason: "full-access",
    }),
    transcript(seq++, "turn-1", {
      kind: "user_prompt",
      content: "Why does reconnect stall after a clean close?",
    }),
    transcript(seq++, "turn-1", { kind: "assistant_text", text: ANSWER }),
    transcript(seq++, "turn-1", {
      kind: "result",
      subtype: "success",
      isError: false,
      durationMs: 41_000,
      result: "Explained.",
    }),
    transcript(seq++, "turn-2", {
      kind: "user_prompt",
      content: "Run the reconnect suite.",
    }),
    transcript(seq, "turn-2", {
      kind: "tool_call",
      tool: {
        toolName: "Bash",
        toolId: "bash-unfinished",
        input: { command: "pnpm test reconnect" },
      },
    }),
  ];
}

async function openSession(page: Page): Promise<Locator> {
  await page.setViewportSize({ width: 1440, height: 760 });
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
  await expect(workspace).toContainText("Run the reconnect suite.");
  return workspace;
}

test.describe.configure({ mode: "serial" });

test("captures the audited workspace and composer IDs", async ({ page }) => {
  test.setTimeout(120_000);
  const shoot = async (name: (typeof SHOT_NAMES)[number], locator: Locator) => {
    await expect(locator).toBeVisible();
    await waitForAnimations(page);
    await locator.screenshot({ path: `${SHOTS}/${name}.png` });
  };
  const rest = () => page.mouse.move(2, 2);

  const workspace = await openSession(page);
  const pane = page.getByTestId("coding-session-transcript-pane");
  const scroller = page.getByTestId("coding-session-transcript-scroll");
  const dock = page.getByTestId("coding-session-composer-dock");

  // SV-13: with no side surface open the default reading column is capped
  // at 48rem (768 px at 1x), not widened to the window.
  const column = scroller.locator("[data-coding-session-column]");
  const columnBox = await column.boundingBox();
  if (!columnBox) throw new Error("the transcript column has no box");
  expect(columnBox.width).toBeLessThanOrEqual(769);
  expect(columnBox.width).toBeGreaterThan(700);
  await rest();
  await shoot("SV-13-width", pane);

  // SV-42: an unfinished call in an Idle session is settled — "Did not
  // finish" — and the screen reader hears "idle", never "status unknown".
  await expect(page.getByTestId("coding-session-live-status")).toHaveText(
    "Coding session idle",
  );
  const unfinished = page
    .getByTestId("coding-session-active-tool")
    .filter({ hasText: "Did not finish" });
  const fold = page.getByTestId("coding-session-worked-fold");
  if (!(await unfinished.first().isVisible()) && (await fold.count()) > 0) {
    // The call may sit inside its turn's fold; open it.
    await fold.last().click();
  }
  await expect(unfinished).toHaveCount(1);
  await expect(unfinished).toBeVisible();
  await expect(workspace).not.toContainText("Status unknown");
  await shoot("SV-42-idle-unfinished", unfinished);

  // SV-16: routine continuity left the transcript for Details.
  const transcriptLog = page.getByTestId("coding-session-transcript");
  await expect(transcriptLog).not.toContainText("Started fresh");
  await page.getByTestId("coding-session-provenance-toggle").click();
  const continuity = page.getByTestId("coding-session-details-continuity");
  await expect(continuity).toContainText("Started fresh");
  await shoot("SV-16-details", continuity);
  await page.keyboard.press("Escape");
  await expect(continuity).toHaveCount(0);

  // SV-18: the provider's mark, then the model's name alone.
  const identity = workspace.getByTestId("coding-session-control-identity");
  await expect(identity).toContainText("Claude Opus 5");
  await expect(identity).not.toContainText("·");
  await expect(identity.locator('[data-provider-mark="claude"]')).toHaveCount(
    1,
  );
  await rest();
  await shoot("SV-18-chip", identity);

  // SV-19: the attach icon sits immediately before send; the placeholder
  // claims no `@`, `$` or `/`.
  const composer = workspace.getByTestId("coding-session-composer");
  const attach = composer.getByTestId("coding-session-composer-attach");
  const send = composer.getByTestId("coding-session-composer-primary");
  const attachBox = await attach.boundingBox();
  const sendBox = await send.boundingBox();
  if (!attachBox || !sendBox) throw new Error("attach or send has no box");
  expect(attachBox.x + attachBox.width).toBeLessThanOrEqual(sendBox.x + 1);
  expect(sendBox.x - (attachBox.x + attachBox.width)).toBeLessThan(24);
  const placeholder =
    (await composer
      .getByLabel("Coding-session instruction")
      .getAttribute("placeholder")) ?? "";
  expect(placeholder).not.toMatch(/[@$/]/);
  await shoot("SV-19-composer", composer);

  // SV-17: one chip, "Full access ▾", warned on the chip itself; the open
  // menu lists both modes with descriptions and checks the one in force.
  const chip = composer.getByTestId("coding-session-control-sandbox");
  await expect(chip).toContainText("Full access");
  await expect(chip).toHaveAttribute("data-tone", "warning");
  await chip.click();
  const modes = page.getByTestId("coding-session-sandbox-mode");
  await expect(modes).toHaveCount(2);
  await expect(modes.nth(0)).toContainText("Sandboxed");
  await expect(modes.nth(1)).toContainText("Full access");
  await expect(modes.nth(1)).toHaveAttribute("aria-current", "true");
  const menu = page
    .getByRole("dialog")
    .filter({ has: page.getByTestId("coding-session-sandbox-modes") });
  await expect(menu).toContainText("Sandbox off");
  await shoot("SV-17-access-open", menu);
  await page.keyboard.press("Escape");
  await expect(menu).toHaveCount(0);

  // SV-14: scrolled away, the pill sits above the composer — and still does
  // once the composer grows.
  const box = await scroller.boundingBox();
  if (!box) throw new Error("the transcript scroller has no box");
  await page.mouse.move(box.x + box.width / 2, box.y + box.height / 2);
  for (let tick = 0; tick < 12; tick += 1) await page.mouse.wheel(0, -400);
  const pill = page.getByTestId("coding-session-scroll-to-latest");
  await expect(pill).toBeVisible();
  const clearsDock = async () => {
    await expect
      .poll(async () => {
        const pillBox = await pill.boundingBox();
        const dockBox = await dock.boundingBox();
        if (!pillBox || !dockBox) return Number.NaN;
        return dockBox.y - (pillBox.y + pillBox.height);
      })
      .toBeGreaterThanOrEqual(0);
  };
  await clearsDock();
  await composer
    .getByLabel("Coding-session instruction")
    .fill(["One", "two", "three", "four", "five", "six"].join("\n"));
  await clearsDock();
  await rest();
  await shoot("SV-14-pill", pane);
});

test("every audit screenshot is distinct", () => {
  const seen = new Map<string, string>();
  for (const name of SHOT_NAMES) {
    const hash = createHash("sha256")
      .update(readFileSync(`${SHOTS}/${name}.png`))
      .digest("hex");
    expect(
      seen.get(hash),
      `${name} is byte-identical to ${seen.get(hash)}`,
    ).toBeUndefined();
    seen.set(hash, name);
  }
  expect(seen.size).toBe(SHOT_NAMES.length);
});
