import { createHash } from "node:crypto";

import { expect, test, type Locator, type Page } from "@playwright/test";

import type { RelayEvent } from "@/shared/api/types";
import { waitForAnimations } from "../helpers/animations";
import { installMockBridge } from "../helpers/bridge";
import {
  CHANNEL_NAME,
  PROVIDER_PUBKEY,
  RELAY_PUBKEY,
  SINGLE_TITLE,
  SINGLE_TURNS,
  singlePrompt,
  singleSessionEvents,
  UMBRELLA_FAILED_TURN,
  UMBRELLA_GATE_FILES,
  UMBRELLA_GATE_TURN,
  UMBRELLA_HANDOVER_TURN,
  UMBRELLA_RULING_TURN,
  UMBRELLA_TEAMMATE_PROMPT,
  UMBRELLA_TITLE,
  umbrellaFixture,
  umbrellaPrompt,
} from "./coding-session-wave-b-minimap.fixtures";

// Session-view parity Wave B, lane B5: the transcript minimap (SV-26) and
// its colours, marks and "since you were here" rule (SV-27). Every shot is
// scoped to the minimap (plus its card when open), and the set is gated on
// distinct hashes.

const SHOTS = "test-results/session-parity-b";
const LAST_SEEN_PREFIX = "beekeeper:session-last-seen:v1:";

async function seed(page: Page, events: RelayEvent[]) {
  await page.evaluate(
    ({ channelName, signedEvents }) => {
      const seedEvent = window.__BUZZ_E2E_SEED_MOCK_SIGNED_EVENT__;
      if (!seedEvent) throw new Error("signed-event seeding hook is missing");
      for (const event of signedEvents) seedEvent({ channelName, event });
    },
    { channelName: CHANNEL_NAME, signedEvents: events },
  );
}

/** Open the seeded session; after a reload the relay mock is re-seeded. */
async function openSession(page: Page, events: RelayEvent[], title: string) {
  await page.goto("/");
  await page.getByTestId(`channel-${CHANNEL_NAME}`).click();
  await seed(page, events);
  const trigger = page.getByTestId("channel-coding-sessions-trigger");
  await expect(trigger).toHaveAttribute("aria-label", "Coding sessions (1)", {
    timeout: 15_000,
  });
  await trigger.click();
  await page.getByTestId("channel-coding-session-open").first().click();
  await expect(page.getByText(title).first()).toBeVisible({ timeout: 15_000 });
}

/** The strip's pixel position for dash `index` (0-based). */
async function dashPoint(page: Page, index: number) {
  const strip = page.getByTestId("coding-session-minimap-strip");
  const count = await page.getByTestId("coding-session-minimap-dash").count();
  const box = await strip.boundingBox();
  if (!box) throw new Error("minimap strip has no box");
  const progress = count <= 1 ? 0 : index / (count - 1);
  // The box's bottom edge is outside it: the last dash is aimed one pixel up.
  return {
    x: box.x + 8,
    y: box.y + Math.min(progress * box.height, box.height - 1),
  };
}

/** The user message bubble carrying `text`, not the card's echo of it. */
function promptBubble(page: Page, text: string): Locator {
  return page
    .getByTestId("coding-session-user-message")
    .filter({ hasText: text });
}

function shooter(page: Page, hashes: Map<string, string>) {
  return async (name: string, withCard: boolean) => {
    const minimap = page.getByTestId("coding-session-minimap");
    await expect(minimap).toBeVisible();
    await waitForAnimations(page);
    const boxes = [
      await page.getByTestId("coding-session-minimap-strip").boundingBox(),
    ];
    if (withCard) {
      boxes.push(
        await page.getByTestId("coding-session-minimap-card").boundingBox(),
      );
    }
    const present = boxes.filter(
      (box): box is NonNullable<typeof box> => box !== null,
    );
    if (present.length === 0) throw new Error(`${name}: nothing to capture`);
    const pad = 16;
    const x = Math.max(0, Math.min(...present.map((box) => box.x)) - pad);
    const y = Math.max(0, Math.min(...present.map((box) => box.y)) - pad);
    const right = Math.max(...present.map((box) => box.x + box.width)) + pad;
    const bottom = Math.max(...present.map((box) => box.y + box.height)) + pad;
    const png = await page.screenshot({
      clip: { x, y, width: right - x, height: bottom - y },
      path: `${SHOTS}/${name}.png`,
    });
    hashes.set(name, createHash("sha256").update(png).digest("hex"));
  };
}

const hashes = new Map<string, string>();

test.describe.configure({ mode: "serial" });

test("SV-26, SV-27: single layout — 40 dashes, hover card, jumps, two hues", async ({
  page,
}) => {
  test.setTimeout(120_000);
  const shoot = shooter(page, hashes);
  await page.setViewportSize({ width: 1440, height: 900 });
  await installMockBridge(page, {
    globalAgentConfig: {
      env_vars: {},
      provider: null,
      model: null,
      "allowed-bridge-pubkeys": [
        { pubkey: PROVIDER_PUBKEY, label: "Minimap provider" },
      ],
    },
  });
  await openSession(page, singleSessionEvents(), SINGLE_TITLE);

  // 41 rows (40 prompted turns and one without a prompt): virtualized.
  await expect(page.getByTestId("coding-session-transcript")).toHaveAttribute(
    "data-transcript-renderer",
    "virtualized",
  );
  const minimap = page.getByTestId("coding-session-minimap");
  await expect(minimap).toBeVisible();
  // It lives in B0's slot, at the pane's left edge.
  await expect(
    page
      .getByTestId("coding-session-minimap-slot")
      .getByTestId("coding-session-minimap"),
  ).toHaveCount(1);
  const dashes = page.getByTestId("coding-session-minimap-dash");
  await expect(dashes).toHaveCount(SINGLE_TURNS);
  // The dashes in view are bright; the rest are not.
  const bright = page.locator(
    '[data-testid="coding-session-minimap-dash"][data-in-view="true"]',
  );
  await expect.poll(() => bright.count()).toBeGreaterThan(0);
  expect(await bright.count()).toBeLessThan(SINGLE_TURNS);
  await shoot("SV26-rail", false);

  // Hover: the prompt's first line and the start of the reply.
  const fifth = await dashPoint(page, 4);
  await page.mouse.move(fifth.x, fifth.y);
  const card = page.getByTestId("coding-session-minimap-card");
  await expect(card).toBeVisible();
  await expect(
    page.getByTestId("coding-session-minimap-card-prompt"),
  ).toHaveText(singlePrompt(5));
  await expect(
    page.getByTestId("coding-session-minimap-card-reply"),
  ).toContainText("Reply 5: step 5 now backs off");
  // Beyond T3: a single session has no genesis, so its gates are unread and
  // the card says so rather than "no gate row".
  await expect(
    page.getByTestId("coding-session-minimap-card-duration"),
  ).toHaveText("Took 42s");
  await expect(
    page.getByTestId("coding-session-minimap-card-gates"),
  ).toContainText("no genesis");
  await shoot("SV26-hover", true);

  // Click dash 3: the virtualizer brings turn 3 into view.
  await expect(promptBubble(page, singlePrompt(3))).toHaveCount(0);
  const third = await dashPoint(page, 2);
  await page.mouse.click(third.x, third.y);
  await expect(promptBubble(page, singlePrompt(3))).toBeInViewport({
    timeout: 10_000,
  });
  await page.mouse.move(2, 2);

  // Keyboard: End then Enter jumps to the last turn; Home then Enter to the
  // first.
  const strip = page.getByTestId("coding-session-minimap-strip");
  await strip.focus();
  await page.keyboard.press("End");
  await page.keyboard.press("Enter");
  await expect(promptBubble(page, singlePrompt(SINGLE_TURNS))).toBeInViewport({
    timeout: 10_000,
  });
  await page.keyboard.press("Home");
  await page.keyboard.press("Enter");
  await expect(promptBubble(page, singlePrompt(1))).toBeInViewport({
    timeout: 10_000,
  });
  await page.keyboard.press("ArrowDown");
  await page.keyboard.press("ArrowDown");
  await page.keyboard.press("Enter");
  await expect(promptBubble(page, singlePrompt(3))).toBeInViewport({
    timeout: 10_000,
  });
  await strip.blur();

  // SV-27 / DB12: your prompts are neutral, the teammate's take their hue.
  const hues = await page
    .getByTestId("coding-session-minimap-dash-hue")
    .evaluateAll((nodes) =>
      Array.from(
        new Set(nodes.map((node) => getComputedStyle(node).backgroundColor)),
      ),
    );
  expect(hues.length).toBe(2);
  await expect(
    page.locator(
      '[data-testid="coding-session-minimap-dash"][data-author-kind="other"]',
    ),
  ).toHaveCount(SINGLE_TURNS / 5);
  await shoot("SV27-authors", false);
});

test("SV-26, SV-27: umbrella — marks, card, reveal past the window, since rule, Mission", async ({
  page,
}) => {
  test.setTimeout(150_000);
  const shoot = shooter(page, hashes);
  const fixture = umbrellaFixture();
  await page.setViewportSize({ width: 1440, height: 900 });
  // The team fold answers from the signed 44244 rows (lane B5's bridge), so
  // the open `decision.request` reaches the minimap as a waiting ruling.
  await page.addInitScript(() => {
    (
      window as Window & { __BUZZ_E2E_WAVE_B_MINIMAP_TEAM_FOLD__?: boolean }
    ).__BUZZ_E2E_WAVE_B_MINIMAP_TEAM_FOLD__ = true;
  });
  await installMockBridge(page, {
    // The handover read trusts only receipts from the relay's own key.
    relaySelf: RELAY_PUBKEY,
    codingSessionObservationFoldResponse: fixture.foldResponse,
    globalAgentConfig: {
      env_vars: {},
      provider: null,
      model: null,
      "allowed-bridge-pubkeys": [
        { pubkey: PROVIDER_PUBKEY, label: "This computer (coding sessions)" },
      ],
    },
  });
  await openSession(page, fixture.events, UMBRELLA_TITLE);
  await expect(
    page.getByTestId("coding-session-umbrella-workspace"),
  ).toBeVisible();

  const dashes = page.getByTestId("coding-session-minimap-dash");
  await expect(dashes).toHaveCount(15);
  // Marks sit on their own turn: the failed turn is red, the gate turn
  // carries a ✓, and the founder's checkpoint (kind 44247) is a handover
  // glyph on the turn whose window holds it.
  const marksOf = async (turn: number) => {
    const itemId = await dashes.nth(turn - 1).getAttribute("data-item-id");
    if (itemId === null) throw new Error(`dash ${turn} has no item id`);
    return page.locator(
      `[data-testid="coding-session-minimap-marks"][data-item-id="${itemId}"]`,
    );
  };
  const failedDash = dashes.nth(UMBRELLA_FAILED_TURN - 1);
  await expect(failedDash).toHaveAttribute("data-author-kind", "you");
  await expect(
    page.getByTestId("coding-session-minimap-mark-failed"),
  ).toHaveCount(1);
  await expect(
    (await marksOf(UMBRELLA_FAILED_TURN)).getByTestId(
      "coding-session-minimap-mark-failed",
    ),
  ).toHaveCount(1);
  const gateMark = page.getByTestId("coding-session-minimap-mark-gate");
  await expect(gateMark).toHaveCount(1, { timeout: 15_000 });
  await expect(gateMark).toHaveText("✓");
  await expect(gateMark).toHaveAttribute("data-glyph", "pass");
  await expect(
    (await marksOf(UMBRELLA_GATE_TURN)).getByTestId(
      "coding-session-minimap-mark-gate",
    ),
  ).toHaveCount(1);
  await expect(
    page.getByTestId("coding-session-minimap-mark-handover"),
  ).toHaveCount(1, { timeout: 15_000 });
  await expect(
    (await marksOf(UMBRELLA_HANDOVER_TURN)).getByTestId(
      "coding-session-minimap-mark-handover",
    ),
  ).toHaveCount(1);
  // DB8: the open `decision.request` is an amber mark on the turn whose
  // window holds its signed time, and nowhere else.
  const rulingMark = page.getByTestId("coding-session-minimap-mark-ruling");
  await expect(rulingMark).toHaveCount(1, { timeout: 15_000 });
  await expect(
    (await marksOf(UMBRELLA_RULING_TURN)).getByTestId(
      "coding-session-minimap-mark-ruling",
    ),
  ).toHaveCount(1);
  const rulingPoint = await dashPoint(page, UMBRELLA_RULING_TURN - 1);
  await page.mouse.move(rulingPoint.x, rulingPoint.y);
  await expect(
    page.getByTestId("coding-session-minimap-card-ruling"),
  ).toHaveText("Waiting on a ruling opened during this turn");
  await page.mouse.move(2, 2);
  await expect(page.getByTestId("coding-session-minimap-card")).toHaveCount(0);
  await shoot("SV27-marks", false);

  // The card beyond T3: duration, files (count and first three), gates.
  const gatePoint = await dashPoint(page, UMBRELLA_GATE_TURN - 1);
  await page.mouse.move(gatePoint.x, gatePoint.y);
  await expect(
    page.getByTestId("coding-session-minimap-card-prompt"),
  ).toHaveText(umbrellaPrompt(UMBRELLA_GATE_TURN));
  const names = UMBRELLA_GATE_FILES.slice(0, 3).map((path) =>
    path.replace("src/", ""),
  );
  await expect(
    page.getByTestId("coding-session-minimap-card-files"),
  ).toHaveText(`4 files changed: ${names.join(", ")} and 1 more`);
  await expect(
    page.getByTestId("coding-session-minimap-card-gates"),
  ).toHaveText("Gates: 1 passed");
  await expect(
    page.getByTestId("coding-session-minimap-card-duration"),
  ).toHaveText("Took 42s");
  await shoot("SV27-card-beyond", true);

  // Two authors: the teammate's dash takes a hue of its own.
  const hues = await page
    .getByTestId("coding-session-minimap-dash-hue")
    .evaluateAll((nodes) =>
      Array.from(
        new Set(nodes.map((node) => getComputedStyle(node).backgroundColor)),
      ),
    );
  expect(hues.length).toBe(2);

  // Click dash 3: turn 3 sits above the ten-turn window; it is revealed, then
  // scrolled into view.
  await expect(promptBubble(page, umbrellaPrompt(3))).toHaveCount(0);
  const third = await dashPoint(page, 2);
  await page.mouse.click(third.x, third.y);
  await expect(promptBubble(page, umbrellaPrompt(3))).toBeInViewport({
    timeout: 10_000,
  });
  await page.mouse.move(2, 2);

  // Since you were here: this visit wrote the device marker. Move it back
  // to turn 5, reload, and the rule sits after turn 5's dash.
  await expect
    .poll(() =>
      page.evaluate(
        (prefix) =>
          Object.keys(window.localStorage).filter((key) =>
            key.startsWith(prefix),
          ).length,
        LAST_SEEN_PREFIX,
      ),
    )
    .toBe(1);
  const fifthId = await dashes.nth(4).getAttribute("data-item-id");
  if (fifthId === null) throw new Error("dash 5 has no item id");
  await page.evaluate(
    ({ prefix, itemId }) => {
      const key = Object.keys(window.localStorage).find((candidate) =>
        candidate.startsWith(prefix),
      );
      if (!key) throw new Error("no last-seen marker");
      window.localStorage.setItem(key, JSON.stringify({ itemId, at: 1 }));
    },
    { prefix: LAST_SEEN_PREFIX, itemId: fifthId },
  );
  await page.reload();
  await openSession(page, fixture.events, UMBRELLA_TITLE);
  const since = page.getByTestId("coding-session-minimap-since");
  await expect(since).toHaveCount(1);
  await expect(since).toHaveAttribute("data-after-item", fifthId);
  await expect(since).toContainText("New since you were here");
  await expect(since).toContainText("on this device");
  const sixth = await dashPoint(page, 5);
  await page.mouse.move(sixth.x, sixth.y);
  await expect(
    page.getByTestId("coding-session-minimap-card-since"),
  ).toHaveText("New since you were here (on this device)");
  await page.mouse.move(2, 2);
  await shoot("SV27-since", false);

  // The teammate's turn is last; its card names who prompted it.
  const last = await dashPoint(page, 14);
  await page.mouse.move(last.x, last.y);
  await expect(
    page.getByTestId("coding-session-minimap-card-prompt"),
  ).toHaveText(UMBRELLA_TEAMMATE_PROMPT);
  await page.mouse.move(2, 2);

  // Mission: the route rail is the map; there is no minimap.
  await page.getByTestId("coding-session-lens-mission").click();
  await expect(page.getByTestId("coding-session-minimap")).toHaveCount(0);
});

test("screenshots are hash-distinct", () => {
  const expected = [
    "SV26-rail",
    "SV26-hover",
    "SV27-authors",
    "SV27-marks",
    "SV27-since",
    "SV27-card-beyond",
  ];
  expect([...hashes.keys()].sort()).toEqual([...expected].sort());
  expect(new Set(hashes.values()).size).toBe(expected.length);
});
