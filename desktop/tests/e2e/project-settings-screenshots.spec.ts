import { expect, test } from "@playwright/test";
import { waitForAnimations } from "../helpers/animations";
import { installMockBridge } from "../helpers/bridge";

const SHOTS = "test-results/project-settings";

// The full owner path through the new Project Settings dialog: create a
// project, give it an emoji icon and a tint color on the General tab, and
// verify the color actually lands where the feature promises — the project's
// sidebar group and the content pane while the project is showing. Each
// screenshot captures one of those states.
test("project settings set icon and color, and the tint reaches both surfaces", async ({
  page,
}) => {
  await page.addInitScript(() => {
    window.localStorage.setItem(
      "buzz-feature-overrides-v1",
      JSON.stringify({ projects: true, forum: true }),
    );
  });
  await installMockBridge(page);
  await page.goto("/", { waitUntil: "domcontentloaded" });

  // Create a project to configure.
  await page.getByTestId("open-projects-view").click();
  await expect(page.getByTestId("projects-manage-panel")).toBeVisible({
    timeout: 10_000,
  });
  await page.getByTestId("projects-create-menu").click();
  await page.getByTestId("projects-create-menu-project").click();
  await page.getByTestId("create-project-container-name").fill("Honeycomb");
  // Color is offered at creation too; the settings dialog changes it below.
  await page.getByTestId("create-project-container-color-blue").click();
  await page.getByTestId("create-project-container-submit").click();
  const card = page.getByTestId("manage-project-honeycomb");
  await expect(card).toBeVisible({ timeout: 10_000 });
  await expect
    .poll(async () =>
      page.evaluate(() => {
        // General's head is published first; look for Honeycomb's own.
        const head = (window.__BUZZ_E2E_SIGNED_EVENTS__ ?? []).find(
          (event) =>
            event.kind === 30621 &&
            event.tags.some((tag) => tag[0] === "d" && tag[1] === "honeycomb"),
        );
        return head?.tags.find((tag) => tag[0] === "color")?.[1] ?? null;
      }),
    )
    .toBe("#3b82f6");

  // Open its settings and set icon + color on the General tab.
  await page.getByTestId("manage-project-actions-honeycomb").click();
  await page.getByRole("menuitem", { name: "Project settings" }).click();
  await expect(page.getByTestId("edit-project-container-name")).toHaveValue(
    "Honeycomb",
  );
  await waitForAnimations(page);

  // Navigate the picker by category rather than search: focusing the
  // shadow-DOM search input trips the dialog's focus trap and closes the
  // popover (a headless-focus artifact, not a product path — users click).
  await page.getByTestId("edit-project-container-icon").click();
  const picker = page.locator("em-emoji-picker");
  await expect(picker).toBeVisible();
  await waitForAnimations(page);
  // emoji-mart remounts once the custom-emoji category resolves; interacting
  // across that remount detaches mid-click.
  await page.waitForTimeout(600);
  await picker.getByRole("button", { name: "Animals & Nature" }).click();
  await picker.getByRole("button", { name: "🐝" }).first().click();
  await expect(page.getByTestId("edit-project-container-icon")).toContainText(
    "🐝",
  );

  await page.getByTestId("edit-project-container-color-orange").click();
  await waitForAnimations(page);
  await page.screenshot({ path: `${SHOTS}/01-general-tab.png` });

  // The other two tabs, for the record.
  await page.getByTestId("project-settings-tab-members").click();
  await waitForAnimations(page);
  await page.screenshot({ path: `${SHOTS}/02-members-tab.png` });
  await page.getByTestId("project-settings-tab-local").click();
  await expect(
    page.getByTestId("project-settings-default-agent"),
  ).toBeVisible();
  await waitForAnimations(page);
  await page.screenshot({ path: `${SHOTS}/03-this-computer-tab.png` });

  // Save publishes icon + color on the head event.
  await page.getByTestId("project-settings-tab-general").click();
  await page.getByTestId("edit-project-container-save").click();
  await expect
    .poll(async () =>
      page.evaluate(() => {
        const events = window.__BUZZ_E2E_SIGNED_EVENTS__ ?? [];
        const head = [...events]
          .reverse()
          .find((event) => event.kind === 30621);
        return head?.tags
          .filter((tag) => tag[0] === "icon" || tag[0] === "color")
          .map((tag) => `${tag[0]}=${tag[1]}`)
          .sort()
          .join(",");
      }),
    )
    .toBe("color=#f97316,icon=🐝");

  // The sidebar group picks up the emoji and the wash.
  const group = page.getByTestId("project-group-honeycomb");
  await expect(group).toBeVisible({ timeout: 10_000 });
  await expect(group).toHaveAttribute("data-project-tinted", "");
  await group.scrollIntoViewIfNeeded();
  await waitForAnimations(page);
  await page.screenshot({
    path: `${SHOTS}/04-sidebar-tinted.png`,
    clip: { x: 0, y: 0, width: 320, height: 720 },
  });

  // Opening the project tints the content pane.
  await group.getByTestId("project-open-honeycomb").click();
  await expect(page.locator("[data-project-tint]")).toHaveCount(1, {
    timeout: 10_000,
  });
  await waitForAnimations(page);
  await page.screenshot({ path: `${SHOTS}/05-content-pane-tinted.png` });

  // Leaving the project drops the tint with it.
  await page.getByTestId("open-projects-view").click();
  await expect(page.locator("[data-project-tint]")).toHaveCount(0);
});
