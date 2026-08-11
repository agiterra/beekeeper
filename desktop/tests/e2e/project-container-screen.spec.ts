import { expect, test, type Page } from "@playwright/test";

import { installMockBridge } from "../helpers/bridge";

// The per-project screen (sidebar ↗ arrow) is the project-scoped management
// surface: owner-gated edit/delete, per-row "Move to project" menus, and
// per-section + create buttons — the all-projects manage tab keeps the
// cross-project view.

const FEATURES = JSON.stringify({
  projects: true,
  forum: true,
  workflows: true,
});

async function boot(page: Page) {
  await page.addInitScript((features) => {
    window.localStorage.setItem("buzz-feature-overrides-v1", features);
  }, FEATURES);
  await installMockBridge(page);
  await page.goto("/", { waitUntil: "domcontentloaded" });
  await expect(page.getByTestId("project-group-general")).toBeVisible({
    timeout: 10_000,
  });
}

async function createProject(page: Page, name: string) {
  await page.getByTestId("project-container-new").click();
  await page.getByTestId("create-project-container-name").fill(name);
  await page.getByTestId("create-project-container-submit").click();
}

async function openProjectScreen(page: Page, dtag: string) {
  const group = page.getByTestId(`project-group-${dtag}`);
  await expect(group).toBeVisible({ timeout: 10_000 });
  await group.hover();
  await page.getByTestId(`project-open-${dtag}`).click();
  await expect(page).toHaveURL(/\/projects\/[^/?]+\/?$/);
}

test("owner can edit a project from its screen; General cannot be deleted", async ({
  page,
}) => {
  await boot(page);
  await createProject(page, "Skunkworks");
  await openProjectScreen(page, "skunkworks");

  await page.getByTestId("project-screen-actions").click();
  await page.getByTestId("project-screen-edit").click();
  await expect(page.getByTestId("edit-project-container-name")).toHaveValue(
    "Skunkworks",
  );
  await page.getByTestId("edit-project-container-name").fill("Moonshot");
  await page.getByTestId("edit-project-container-save").click();
  await expect(page.getByRole("heading", { name: "Moonshot" })).toBeVisible({
    timeout: 10_000,
  });

  // The rename republished the container: two kind:30621 heads share the dtag.
  await expect
    .poll(async () =>
      page.evaluate(
        () =>
          window.__BUZZ_E2E_SIGNED_EVENTS__?.filter(
            (event) =>
              event.kind === 30621 &&
              event.tags.some(
                (tag) => tag[0] === "d" && tag[1] === "skunkworks",
              ),
          ).length ?? 0,
      ),
    )
    .toBeGreaterThan(1);

  // General is editable but never deletable.
  await page.goBack();
  await openProjectScreen(page, "general");
  await page.getByTestId("project-screen-actions").click();
  await expect(page.getByTestId("project-screen-edit")).toBeVisible();
  await expect(page.getByTestId("project-screen-delete")).toHaveCount(0);
});

test("section + buttons create items scoped to the project", async ({
  page,
}) => {
  await boot(page);
  await openProjectScreen(page, "general");

  // Repo: created with the project forward/back refs; no navigation.
  await page.getByTestId("project-section-create-repo").click();
  await page.getByTestId("create-project-name").fill("widget-lib");
  await page.getByTestId("create-project-submit").click();
  await expect
    .poll(async () =>
      page.evaluate(
        () =>
          window.__BUZZ_E2E_SIGNED_EVENTS__?.filter(
            (event) =>
              event.kind === 30617 &&
              event.tags.some(
                (tag) => tag[0] === "d" && tag[1] === "widget-lib",
              ),
          ).length ?? 0,
      ),
    )
    .toBeGreaterThan(0);

  // Channel: creates into the project and navigates to the new channel.
  await page.getByTestId("project-section-create-channel").click();
  await page.getByTestId("create-channel-name").fill("skunk-chat");
  await page.getByTestId("create-channel-submit").click();
  await expect(page).toHaveURL(/\/channels\//, { timeout: 10_000 });
});

test("deleting a project from its screen lands on the projects list", async ({
  page,
}) => {
  await boot(page);
  await createProject(page, "Doomed");
  await openProjectScreen(page, "doomed");

  await page.getByTestId("project-screen-actions").click();
  await page.getByTestId("project-screen-delete").click();
  await page.getByTestId("manage-project-delete-confirm").click();
  await expect(page).toHaveURL(/\/projects\?filter=projects/, {
    timeout: 10_000,
  });
});
