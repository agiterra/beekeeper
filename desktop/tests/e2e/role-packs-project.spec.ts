import { expect, test, type Page } from "@playwright/test";

import type { RelayEvent } from "@/shared/api/types";
import { KIND_PROJECT } from "@/shared/constants/kinds";

import { installMockBridge } from "../helpers/bridge";
import { openDashboardTab } from "../helpers/dashboard";
import { waitForAnimations } from "../helpers/animations";

/**
 * The Agents tab says which project's role packs it will install.
 *
 * Ledger 85 pre-chose `<checkout>/personas/roles` for the installer and then
 * found the pre-chosen folder unreachable: the Dashboard route names no
 * project, so nothing was ever resolved and the whole path was dead code that
 * every unit test still passed. This spec exists so that cannot happen twice —
 * it drives the surface an operator actually reaches, with more than one
 * project to get wrong.
 */

const IDENTITY = {
  privateKey:
    "3dbaebadb5dfd777ff25149ee230d907a15a9e1294b40b830661e65bb42f6c03",
  pubkey: "e5ebc6cdb579be112e336cc319b5989b4bb6af11786ea90dbe52b5f08d741b34",
  username: "tyler",
};

const PROJECT_FEATURES = JSON.stringify({ projects: true });
const SNAPSHOTS = "test-results/role-pack-snapshots";

/** Kind:30621 is not signature-checked by the client, like the other mock
 * fixtures, so a hand-built head is enough to give this viewer projects. */
function projectHead(dtag: string, name: string): RelayEvent {
  return {
    id: `project-${dtag}`.padEnd(64, "0"),
    pubkey: IDENTITY.pubkey,
    created_at: Math.floor(Date.now() / 1000) - 7_200,
    kind: KIND_PROJECT,
    tags: [
      ["d", dtag],
      ["name", name],
    ],
    content: "",
    sig: "mocksig".repeat(20).slice(0, 128),
  };
}

async function openAgentsTab(page: Page) {
  await page.addInitScript((identity) => {
    window.localStorage.setItem(
      "buzz:e2e-identity-override.v1",
      JSON.stringify(identity),
    );
  }, IDENTITY);
  await page.addInitScript(
    (events) => {
      (
        window as unknown as { __BUZZ_E2E_EXTRA_PROJECT_EVENTS__: unknown }
      ).__BUZZ_E2E_EXTRA_PROJECT_EVENTS__ = events;
    },
    [projectHead("attic", "Attic"), projectHead("skunkworks", "Skunkworks")],
  );
  await installMockBridge(page, {});
  await page.goto("/");
  await openDashboardTab(page, "agents");
  await expect(page.getByTestId("agents-library-teams")).toBeVisible({
    timeout: 15_000,
  });
}

async function openInstaller(page: Page) {
  await page.getByTestId("new-team-card").click();
  await page.getByTestId("install-crew-roles").click();
  await expect(page.getByTestId("install-crew-roles-dialog")).toBeVisible();
}

test("the resolved project is named on the tab and in the installer, and switching moves both", async ({
  page,
}) => {
  await openAgentsTab(page);

  // More than one project to get wrong, so the choice is on the surface.
  const selector = page.getByTestId("role-packs-project-selector");
  await expect(selector).toBeVisible();
  const firstName = (
    await page.getByTestId("role-packs-project-trigger").textContent()
  )?.trim();
  expect(firstName).toBeTruthy();
  await expect(selector).toContainText("Role packs for:");

  // …and the dialog opens naming the same project, not a folder from nowhere.
  await openInstaller(page);
  await expect(page.getByTestId("install-crew-roles-folder-label")).toHaveText(
    `The project's role packs — ${firstName}`,
  );
  await page.keyboard.press("Escape");
  await expect(page.getByTestId("install-crew-roles-dialog")).toHaveCount(0);

  // Switching re-labels the tab. The menu is non-modal on purpose: Radix's
  // modal variant leaves `pointer-events: none` on the body for a beat after a
  // selection, which swallowed the very next click on this page.
  await page.getByTestId("role-packs-project-trigger").click();
  const options = page.getByRole("menuitem");
  const optionCount = await options.count();
  expect(optionCount).toBeGreaterThan(1);
  const otherName = (await options.nth(optionCount - 1).textContent())?.trim();
  expect(otherName).not.toBe(firstName);
  await options.nth(optionCount - 1).click();
  await expect(page.getByTestId("role-packs-project-trigger")).toHaveText(
    otherName ?? "",
  );

  // …and the installer re-opens on the project that is now named, having
  // scanned that project's checkout rather than the one it replaced.
  await openInstaller(page);
  await expect(page.getByTestId("install-crew-roles-folder-label")).toHaveText(
    `The project's role packs — ${otherName}`,
  );
});

test("the Packs tab explains local versions and unverified metadata claims", async ({
  page,
}) => {
  await page.addInitScript((features) => {
    window.localStorage.setItem("buzz-feature-overrides-v1", features);
  }, PROJECT_FEATURES);
  await installMockBridge(page, {});
  await page.setViewportSize({ width: 1280, height: 850 });
  await page.goto("/");
  const general = page.getByTestId("project-group-general");
  await expect(general).toBeVisible({ timeout: 15_000 });
  await general.hover();
  await page.getByTestId("project-open-general").click();
  await expect(page.getByTestId("project-page-tabs")).toBeVisible();
  await page.getByTestId("project-tab-packs").click();

  const snapshots = page.getByTestId("role-pack-snapshots");
  await expect(snapshots).toBeVisible({ timeout: 15_000 });
  await expect(snapshots).toContainText(
    "Versions found on this machine and signed metadata claims visible in this project’s channels.",
  );
  await expect(snapshots).toContainText(
    "Beekeeper has not verified that a commissioned provider authored these claims.",
  );
  await expect(snapshots.getByText("Available here")).toBeVisible();
  await expect(
    snapshots.getByText("Unverified channel metadata"),
  ).toBeVisible();
  await expect(page.getByTestId("role-pack-resolved-row")).toHaveCount(2);
  await expect(snapshots).toContainText("lead · 9f2e1d0c");
  await expect(snapshots).toContainText("reviewer · 0.0.0-e");
  await expect(page.getByTestId("role-pack-reports-empty")).toHaveText(
    "No role-version metadata claims are visible for this project.",
  );
  await expect(snapshots).not.toContainText("44223");

  await waitForAnimations(page);
  await snapshots.screenshot({ path: `${SNAPSHOTS}/available-and-empty.png` });
});
