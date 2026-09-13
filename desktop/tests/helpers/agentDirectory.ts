import { expect, type Page } from "@playwright/test";

/** Open the deployed identity through the directory's shared profile action. */
export async function openDirectoryAgentProfile(page: Page, name: string) {
  const row = page.getByTestId("agent-row").filter({
    has: page.getByTestId("agent-row-name").getByText(name, { exact: true }),
  });
  await expect(row).toBeVisible();
  await row.click();
  await page.getByTestId("agent-detail-profile").click();
  await expect(page.getByTestId("user-profile-panel")).toBeVisible();
}

/** Open the secondary saved-definition management surface. */
export async function openAgentDefinitions(page: Page) {
  const section = page.getByTestId("agent-definitions-management");
  if (!(await section.evaluate((element) => element.hasAttribute("open")))) {
    await section.locator("summary").click();
  }
  await expect(section.getByTestId("unified-agents-groups")).toBeVisible();
  return section;
}

/**
 * Open the collapsed "Saved agent groups" disclosure on the Agents page.
 *
 * Saved groups are a secondary tool for adding several agents to a channel at
 * once; the page no longer leads with them, so specs that exercise team cards
 * open this first.
 */
export async function openSavedAgentGroups(page: Page) {
  const section = page.getByTestId("agents-saved-groups");
  await expect(section).toBeVisible({ timeout: 15_000 });
  if (!(await section.evaluate((element) => element.hasAttribute("open")))) {
    await section.locator("summary").click();
  }
  await expect(section.getByTestId("agents-library-teams")).toBeVisible();
  return section;
}
