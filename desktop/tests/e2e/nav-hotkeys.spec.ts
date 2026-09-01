import { expect, test } from "@playwright/test";
import type { Page } from "@playwright/test";

import { waitForAnimations } from "../helpers/animations";
import { installMockBridge } from "../helpers/bridge";
import { openSettings } from "../helpers/settings";

const SHOTS = "test-results/nav-hotkeys";

// The badges arm only after the modifier has been held alone past the store's
// delay (`features/hotkeys/lib/heldModifierStore.ts`), so a tap on ⌘K does not
// strobe the whole sidebar. Overshoot it here rather than racing it.
const ARM_HOLD_MS = 400;

/**
 * Hold a modifier until the badges arm.
 *
 * Waits for the sidebar first: the store installs its window listeners lazily,
 * on the first badge that mounts, so a key pressed in the moment between
 * navigation and React's first paint reaches nothing at all.
 */
async function hold(page: Page, modifier: "Alt" | "Meta") {
  await expect(page.getByTestId("app-sidebar")).toBeVisible();
  await page.keyboard.down(modifier);
  await page.waitForTimeout(ARM_HOLD_MS);
}

async function release(page: Page, modifier: "Alt" | "Meta") {
  await page.keyboard.up(modifier);
}

test.beforeEach(async ({ page }) => {
  await installMockBridge(page);
});

test("holding the destination modifier reveals its badges, releasing hides them", async ({
  page,
}) => {
  await page.goto("/");
  const dashboard = page.getByTestId("hotkey-badge-dashboard");
  await expect(dashboard).toHaveCount(0);

  await hold(page, "Alt");
  await expect(dashboard).toBeVisible();
  await expect(dashboard).toHaveText("⌥D");
  await expect(page.getByTestId("hotkey-badge-projects")).toHaveText("⌥P");
  await expect(page.getByTestId("hotkey-badge-dms")).toHaveText("⌥M");

  await waitForAnimations(page);
  await page
    .getByTestId("app-sidebar")
    .screenshot({ path: `${SHOTS}/01-destination-badges.png` });

  await release(page, "Alt");
  await expect(dashboard).toHaveCount(0);
});

test("the destination badges number the projects in the order they are listed", async ({
  page,
}) => {
  await page.goto("/");
  await hold(page, "Alt");
  // Position 1 is General, which the sidebar pins above the sortable projects.
  await expect(page.getByTestId("hotkey-badge-project-1")).toBeVisible();
  await release(page, "Alt");
});

test("⌥D and ⌥P go where their badges say", async ({ page }) => {
  await page.goto("/");
  await page.getByTestId("channel-general").click();
  await expect(page.getByTestId("chat-title")).toHaveText("general");

  await page.keyboard.press("Alt+p");
  await expect(page).toHaveURL(/#\/projects/);

  await page.keyboard.press("Alt+d");
  await expect(page).toHaveURL(/#\/$/);
});

test("the row modifier numbers the direct messages while a DM is open", async ({
  page,
}) => {
  await page.goto("/");
  const dmRows = page
    .getByTestId("dm-list")
    .locator("[data-testid^='channel-']");
  await dmRows.first().click();

  await hold(page, "Meta");
  const first = page.getByTestId("hotkey-badge-item-1");
  await expect(first).toBeVisible();
  await expect(first).toHaveText("⌘1");
  await waitForAnimations(page);
  await page
    .getByTestId("dm-list")
    .screenshot({ path: `${SHOTS}/02-direct-message-badges.png` });
  await release(page, "Meta");
});

// Outside a project or a conversation there is no list for the row modifier to
// number, and a badge over nothing is a control that lies about what it does.
test("the row modifier numbers nothing on the Dashboard", async ({ page }) => {
  await page.goto("/");
  await hold(page, "Meta");
  await expect(page.getByTestId("hotkey-badge-item-1")).toHaveCount(0);
  await release(page, "Meta");
});

test("the sidebar marks the row whose page is showing", async ({ page }) => {
  await page.goto("/");
  const general = page.getByTestId("channel-general");
  await general.click();
  await expect(general).toHaveAttribute("data-active", "true");

  const random = page.getByTestId("channel-random");
  await random.click();
  await expect(random).toHaveAttribute("data-active", "true");
  await expect(general).not.toHaveAttribute("data-active", "true");

  await waitForAnimations(page);
  await page
    .getByTestId("app-sidebar")
    .screenshot({ path: `${SHOTS}/03-active-row-indicator.png` });
});

test("the shortcuts settings offer the navigation hotkeys and nothing else", async ({
  page,
}) => {
  await page.goto("/");
  await openSettings(page, "shortcuts");

  await expect(page.getByTestId("nav-hotkeys-toggle")).toBeVisible();
  await expect(
    page.getByTestId("nav-hotkey-scope-modifier-trigger"),
  ).toBeVisible();
  await expect(
    page.getByTestId("nav-hotkey-item-modifier-trigger"),
  ).toBeVisible();
  await expect(page.getByTestId("nav-hotkey-capture-dashboard")).toHaveText(
    "⌥D",
  );
  // Nothing to reset until something is changed.
  await expect(page.getByTestId("nav-hotkeys-reset")).toBeDisabled();

  await waitForAnimations(page);
  await page
    .getByTestId("settings-nav-hotkeys")
    .screenshot({ path: `${SHOTS}/04-shortcut-settings.png` });
});

// Taking a modifier that an existing shortcut already owns is allowed — but
// the person has to be told what it costs, not discover it later at a dead
// keystroke.
test("a rebind that would break an existing shortcut says so", async ({
  page,
}) => {
  await page.goto("/");
  await openSettings(page, "shortcuts");

  await page.getByTestId("nav-hotkey-scope-modifier-trigger").click();
  await page.getByTestId("nav-hotkey-scope-modifier-meta").click();

  await page.getByTestId("nav-hotkey-capture-dashboard").click();
  await page.keyboard.press("f");

  const conflicts = page.getByTestId("nav-hotkeys-conflicts");
  await expect(conflicts).toContainText("⌘F");
  await expect(conflicts).toContainText("Find in channel");
  // Both families on ⌘ is its own conflict, and it is reported too.
  await expect(conflicts).toContainText("share a modifier");

  await waitForAnimations(page);
  await page
    .getByTestId("nav-hotkeys-conflicts")
    .screenshot({ path: `${SHOTS}/05-rebind-conflict.png` });

  await page.getByTestId("nav-hotkeys-reset").click();
  await expect(page.getByTestId("nav-hotkey-capture-dashboard")).toHaveText(
    "⌥D",
  );
  await expect(conflicts).toHaveCount(0);
});

// A badge is a promise about a keystroke. With the hotkeys switched off there
// is no keystroke, so there must be no badge either.
test("switching the hotkeys off takes the badges with them", async ({
  page,
}) => {
  await page.goto("/");
  await openSettings(page, "shortcuts");
  await page.getByTestId("nav-hotkeys-toggle").click();
  await page.getByTestId("settings-back-to-app").click();

  await hold(page, "Alt");
  await expect(page.getByTestId("hotkey-badge-dashboard")).toHaveCount(0);
  await release(page, "Alt");

  // And the chord itself is inert.
  await page.getByTestId("channel-general").click();
  await expect(page.getByTestId("chat-title")).toHaveText("general");
  await page.keyboard.press("Alt+p");
  await expect(page).not.toHaveURL(/#\/projects/);
});
