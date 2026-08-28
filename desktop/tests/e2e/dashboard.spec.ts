import { expect, test } from "@playwright/test";

import { installMockBridge, TEST_IDENTITIES } from "../helpers/bridge";
import { overridePreviewFeatures } from "../helpers/features";
import { openDashboardTab } from "../helpers/dashboard";

/**
 * The Dashboard at `/`: Overview by default, Inbox / Pulse / Agent progress /
 * Agents as URL-driven tabs, and the old standalone paths as redirects.
 */

test.beforeEach(async ({ page }) => {
  await installMockBridge(page);
});

test("lands on the Overview with a card per surface and one sidebar row", async ({
  page,
}) => {
  await page.goto("/");

  await expect(page.getByTestId("dashboard-tabs")).toBeVisible();
  await expect(page.getByTestId("open-overview-view")).toHaveAttribute(
    "data-active",
    "true",
  );
  await expect(page.getByTestId("dashboard-overview")).toBeVisible();
  await expect(page.getByTestId("dashboard-card-inbox")).toBeVisible();
  await expect(page.getByTestId("dashboard-card-agents")).toBeVisible();
  await expect(page.getByTestId("dashboard-card-agent-progress")).toBeVisible();
  await expect(page.getByTestId("dashboard-card-pulse")).toBeVisible();
  await expect(page.getByTestId("home-inbox")).toHaveCount(0);

  const primaryMenu = page.getByTestId("sidebar-primary-menu");
  await expect(primaryMenu.getByTestId("open-dashboard-view")).toHaveAttribute(
    "data-active",
    "true",
  );
  await expect(
    primaryMenu.getByRole("button", { name: "Inbox", exact: true }),
  ).toHaveCount(0);
  await expect(primaryMenu.getByTestId("open-agents-view")).toHaveCount(0);
  await expect(primaryMenu.getByTestId("open-pulse-view")).toHaveCount(0);
});

test("the inbox card and sidebar badge agree", async ({ page }) => {
  await page.goto("/");

  const card = page.getByTestId("dashboard-card-inbox-count");
  await expect(card).toBeVisible();
  const badge = page.getByTestId("sidebar-home-count");
  const badgeText = await badge
    .allTextContents()
    .then((texts) => texts[0] ?? null);
  if (badgeText === null) {
    await expect(card).toHaveText("Nothing needs you right now");
  } else {
    await expect(card).toContainText(`${badgeText} item`);
  }
});

test("tabs are deep links", async ({ page }) => {
  await page.goto("/#/?tab=inbox");
  await expect(page.getByTestId("home-inbox-list")).toBeVisible();
  await expect(page.getByTestId("open-inbox-view")).toHaveAttribute(
    "data-active",
    "true",
  );
  await expect(page.getByTestId("open-dashboard-view")).toHaveAttribute(
    "data-active",
    "true",
  );

  await openDashboardTab(page, "agents");
  await expect(page).toHaveURL(/#\/\?tab=agents$/);
  await expect(page.getByTestId("agents-page-content")).toBeVisible();
  await expect(page.getByTestId("home-inbox")).toHaveCount(0);

  await openDashboardTab(page, "agent-progress");
  await expect(page).toHaveURL(/#\/\?tab=agent-progress$/);
  await expect(page.getByTestId("agent-progress-panel")).toBeVisible();

  await openDashboardTab(page, "pulse");
  await expect(page).toHaveURL(/#\/\?tab=pulse$/);
  await expect(page.getByTestId("dashboard-body-pulse")).toBeVisible();

  await page.getByTestId("open-overview-view").click();
  await expect(page).toHaveURL(/#\/$/);
  await expect(page.getByTestId("dashboard-overview")).toBeVisible();
});

test("overview cards open their tab", async ({ page }) => {
  await page.goto("/");
  await page.getByTestId("dashboard-card-agents").click();
  await expect(page).toHaveURL(/#\/\?tab=agents$/);
  await expect(page.getByTestId("agents-page-content")).toBeVisible();
});

test("an item deep link implies the inbox tab", async ({ page }) => {
  await page.goto("/#/?item=mock-feed-mention");
  await expect(page.getByTestId("open-inbox-view")).toHaveAttribute(
    "data-active",
    "true",
  );
  await expect(
    page.getByTestId("home-inbox-item-mock-feed-mention"),
  ).toBeVisible();
});

test("the old standalone paths redirect onto their tab", async ({ page }) => {
  await page.goto("/#/agents");
  await expect(page).toHaveURL(/#\/\?tab=agents$/);
  await expect(page.getByTestId("agents-page-content")).toBeVisible();

  await page.goto("/#/agent-progress");
  await expect(page).toHaveURL(/#\/\?tab=agent-progress$/);
  await expect(page.getByTestId("agent-progress-panel")).toBeVisible();

  await page.goto(`/#/pulse?profile=${TEST_IDENTITIES.alice.pubkey}`);
  await expect(page).toHaveURL(/#\/\?/);
  await expect(page).toHaveURL(/tab=pulse/);
  await expect(page).toHaveURL(
    new RegExp(`profile=${TEST_IDENTITIES.alice.pubkey}`),
  );
  await expect(page.getByTestId("dashboard-body-pulse")).toBeVisible();
});

test("gated tabs and cards disappear with their preview flag", async ({
  page,
}) => {
  await overridePreviewFeatures(page, {
    pulse: false,
    "agent-progress": false,
  });
  await page.goto("/#/?tab=pulse");

  await expect(page.getByTestId("dashboard-overview")).toBeVisible();
  await expect(page.getByTestId("open-pulse-view")).toHaveCount(0);
  await expect(page.getByTestId("open-agent-progress-view")).toHaveCount(0);
  await expect(page.getByTestId("dashboard-card-pulse")).toHaveCount(0);
  await expect(page.getByTestId("dashboard-card-agent-progress")).toHaveCount(
    0,
  );
  await expect(page.getByTestId("open-inbox-view")).toBeVisible();
  await expect(page.getByTestId("open-agents-view")).toBeVisible();
});
