import { expect, test, type Page } from "@playwright/test";

import {
  BUDGET_WINDOW_MS,
  LOCAL_BURST_CAPACITY,
  WRITE_RESERVE,
} from "@/shared/api/relaySendBudget";
import { MOCK_DEFAULT_REPOS_ROOT } from "@/testing/e2eBridge";
import { installMockBridge } from "../helpers/bridge";

const OWNER = "deadbeef".repeat(8);

async function openCreate(page: Page, name: string) {
  await page.getByTestId("project-container-new").click();
  await page.getByTestId("create-project-container-name").fill(name);
}

async function outgoingReads(page: Page) {
  return page.evaluate(
    () =>
      (window.__BEEKEEPER_E2E_COMMAND_LOG__ ?? []).filter((entry) => {
        if (entry.command !== "plugin:websocket|send") return false;
        const data = (entry.payload as { message?: { data?: string } })?.message
          ?.data;
        return data ? JSON.parse(data)[0] === "REQ" : false;
      }).length,
  );
}

test("cold-start Create checks the exact address without waiting for discovery's next read window", async ({
  page,
}) => {
  await installMockBridge(page);
  await page.goto("/", { waitUntil: "domcontentloaded" });
  await expect(page.getByTestId("project-group-general")).toBeVisible();
  await openCreate(page, "Cold Start Project");
  // The folder row follows the name: `<default repository folder>/<slug>`,
  // the host's default root here because the mock community sets none, and
  // the helper line names the same destination.
  const folder = page.getByTestId("project-checkout-folder");
  await expect(folder).toHaveValue(
    `${MOCK_DEFAULT_REPOS_ROOT}/cold-start-project`,
  );
  await expect(folder).toHaveAttribute(
    "data-checkout-parent",
    MOCK_DEFAULT_REPOS_ROOT,
  );
  await expect(page.getByTestId("project-checkout-folder-hint")).toContainText(
    `cloned to ${MOCK_DEFAULT_REPOS_ROOT}/cold-start-project`,
  );
  // Prove this is the saturated startup path rather than a warm-idle create.
  // No clock/budget resets: background discovery keeps using its real budget.
  await expect
    .poll(() => outgoingReads(page))
    .toBeGreaterThanOrEqual(LOCAL_BURST_CAPACITY - WRITE_RESERVE);
  const before = await outgoingReads(page);
  const started = Date.now();
  await page.getByTestId("create-project-container-submit").click();
  await expect(
    page.getByTestId("project-group-cold-start-project"),
  ).toBeVisible({
    timeout: BUDGET_WINDOW_MS - 1_000,
  });
  await expect(page.getByRole("dialog", { name: "New project" })).toHaveCount(
    0,
  );
  expect(Date.now() - started).toBeLessThan(BUDGET_WINDOW_MS);
  // The prerequisite lookup used one existing batch operation. It did not
  // borrow the write reserve for read frames or unfreeze discovery's budget.
  const lookup = await page.evaluate(
    (owner) =>
      (window.__BEEKEEPER_E2E_COMMAND_LOG__ ?? []).some((entry) => {
        if (entry.command !== "query_relay_filters") return false;
        const filters = (entry.payload as { filters: unknown }).filters;
        return (
          JSON.stringify(filters) ===
          JSON.stringify([
            {
              kinds: [30621],
              authors: [owner],
              "#d": ["cold-start-project"],
              limit: 1,
            },
            {
              kinds: [5],
              authors: [owner],
              "#a": [`30621:${owner}:cold-start-project`],
              limit: 1,
            },
            // The two repository ids the project will create, read
            // community-wide in the same batch (spec § 4.11).
            {
              kinds: [30617],
              "#d": [
                "cold-start-project",
                "cold-start-project-beekeeper-agents",
              ],
              limit: 16,
            },
          ])
        );
      }),
    OWNER,
  );
  expect(lookup).toBe(true);
  expect(await outgoingReads(page)).toBeLessThan(
    before + LOCAL_BURST_CAPACITY - WRITE_RESERVE,
  );
});

test("an owned project older than the discovery page cannot be overwritten by Create", async ({
  page,
}) => {
  await page.addInitScript((owner) => {
    const now = Math.floor(Date.now() / 1_000);
    window.__BEEKEEPER_E2E_EXTRA_PROJECT_EVENTS__ = Array.from(
      { length: 251 },
      (_, index) => ({
        id: index.toString(16).padStart(64, "0"),
        pubkey: owner,
        kind: 30621,
        created_at: now - index,
        tags: [
          ["d", index === 250 ? "existing-project" : `newer-${index}`],
          ["name", index === 250 ? "Existing project" : `Newer ${index}`],
        ],
        content: "",
        sig: "mocksig",
      }),
    );
  }, OWNER);
  await installMockBridge(page);
  await page.goto("/", { waitUntil: "domcontentloaded" });
  await expect(page.getByTestId("project-group-general")).toBeVisible();
  await expect(page.getByTestId("project-group-existing-project")).toHaveCount(
    0,
  );
  await openCreate(page, "Existing project");
  await page.getByTestId("create-project-container-submit").click();
  await expect(page.getByRole("dialog", { name: "New project" })).toContainText(
    'You already have a project named "existing-project".',
  );
  expect(
    await page.evaluate(() =>
      (window.__BEEKEEPER_E2E_SIGNED_EVENTS__ ?? []).some(
        (event) =>
          event.kind === 30621 &&
          event.tags.some(
            (tag) => tag[0] === "d" && tag[1] === "existing-project",
          ),
      ),
    ),
  ).toBe(false);
});
