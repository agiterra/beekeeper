import { expect, type Page } from "@playwright/test";

export type DashboardTabId = "inbox" | "pulse" | "agent-progress" | "agents";

/**
 * Inbox, Pulse, Agent progress and Agents are tabs of the Dashboard at `/`.
 * The sidebar has one Dashboard row; each surface is a tab click past it.
 * From a channel (or anywhere else) the strip is not on screen, so this
 * takes the Dashboard row first — the click path a person takes.
 */
export async function openDashboardTab(page: Page, tab: DashboardTabId) {
  if (!(await page.getByTestId("dashboard-tabs").isVisible())) {
    await page.getByTestId("open-dashboard-view").click();
    await expect(page.getByTestId("dashboard-tabs")).toBeVisible();
  }
  await page.getByTestId(`open-${tab}-view`).click();
}

export async function openInboxTab(page: Page) {
  await openDashboardTab(page, "inbox");
  await expect(page.getByTestId("home-inbox")).toBeVisible();
}
