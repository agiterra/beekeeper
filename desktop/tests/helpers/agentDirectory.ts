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
