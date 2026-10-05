import { expect, type Page } from "@playwright/test";

/**
 * SV-20 moved the header's surface toggles to the launcher: a surface opens
 * from the right-panel toggle (⌘⌥B) and then its launcher row (or letter).
 * These are the steps the old one-click toggles became, for every spec that
 * used them.
 */

export function rightPanelToggle(page: Page) {
  return page.getByTestId("coding-session-panel-toggle-right");
}

export function bottomPanelToggle(page: Page) {
  return page.getByTestId("coding-session-panel-toggle-bottom");
}

/**
 * Open surface `id` in the right panel, from whatever state the panel is in:
 * closed (open it, then pick from the launcher), on the launcher (pick), or
 * showing other tabs (use the tab if it is there, else "+").
 */
export async function openSessionSurface(page: Page, id: string) {
  const tab = page.getByTestId(`coding-session-surface-tab-${id}`);
  const launcher = page.getByTestId("coding-session-surface-launcher");
  const tabbar = page.getByTestId("coding-session-surface-tabbar");
  const toggle = rightPanelToggle(page);
  if ((await toggle.getAttribute("aria-pressed")) !== "true") {
    await toggle.click();
  }
  await expect(launcher.or(tabbar).first()).toBeVisible({ timeout: 15_000 });
  if (await launcher.isVisible()) {
    await page.getByTestId(`coding-session-surface-launcher-row-${id}`).click();
  } else if (await tab.isVisible()) {
    await tab.click();
  } else {
    await page.getByTestId("coding-session-surface-add").click();
    await page.getByTestId(`coding-session-surface-add-${id}`).click();
  }
  await expect(tab).toHaveAttribute("aria-selected", "true");
}
