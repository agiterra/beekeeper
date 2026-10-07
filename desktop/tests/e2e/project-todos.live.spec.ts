import { execFile } from "node:child_process";
import { promisify } from "node:util";

import { expect, test, type Page } from "@playwright/test";

import { installRelayBridge, TEST_IDENTITIES } from "../helpers/bridge";
import { waitForAnimations } from "../helpers/animations";

const exec = promisify(execFile);

// Live gate for the project To-Do tab against a REAL relay: the app (as
// tyler, the project's creator) creates a list and an item through the UI,
// then `bee todos` (as alice, a collaborator) adds and completes items and
// the tab must show each change live, with no reload. Proves the four
// halves synthetic specs cannot: the relay's admission of a collaborator's
// op, the `#a` live fan-out reaching the tab, the client fold agreeing with
// the CLI fold, and the UI's own writes landing on the relay.
//
// Requires: BEEKEEPER_E2E_PROJECT_TODOS=1, BEEKEEPER_E2E_CLI_BIN (a built `bee`), and
// BEEKEEPER_E2E_RELAY_URL pointing at a running relay (e.g. http://localhost:3010).
const enabled = process.env.BEEKEEPER_E2E_PROJECT_TODOS === "1";

function required(name: string, value: string | undefined): string {
  if (!value) throw new Error(`${name} is required for the live gate`);
  return value;
}

async function runCli(args: string[], privateKey: string): Promise<string> {
  const binary = required(
    "BEEKEEPER_E2E_CLI_BIN",
    process.env.BEEKEEPER_E2E_CLI_BIN,
  );
  const relayUrl = required(
    "BEEKEEPER_E2E_RELAY_URL",
    process.env.BEEKEEPER_E2E_RELAY_URL,
  );
  const { stdout } = await exec(binary, args, {
    cwd: "..",
    env: {
      ...process.env,
      BEEKEEPER_AUTH_TAG: "",
      BEEKEEPER_PRIVATE_KEY: privateKey,
      BEEKEEPER_RELAY_URL: relayUrl,
    },
  });
  return stdout;
}

type Seed = { coordinate: string; projectId: string; dtag: string };

async function seedProject(): Promise<Seed> {
  const dtag = `todo-live-${process.pid}-${Date.now().toString(36)}`;
  const tyler = TEST_IDENTITIES.tyler;
  await runCli(
    ["repos", "create", "--id", dtag, "--name", dtag],
    tyler.privateKey,
  );
  await runCli(
    [
      "projects",
      "create",
      dtag,
      "--repo",
      dtag,
      "--name",
      `To-do live ${dtag}`,
    ],
    tyler.privateKey,
  );
  await runCli(
    [
      "projects",
      "add-member",
      dtag,
      "--pubkey",
      TEST_IDENTITIES.alice.pubkey,
      "--role",
      "collaborator",
    ],
    tyler.privateKey,
  );
  return {
    coordinate: `30621:${tyler.pubkey}:${dtag}`,
    projectId: `${tyler.pubkey}:${dtag}`,
    dtag,
  };
}

async function openTab(page: Page, seed: Seed) {
  // The preview server is a static file server with no history fallback and
  // the app's router does not read browser history, so reach the tab the way
  // a person does: open the project from the sidebar, then click To-Do.
  await page.goto("/");
  await page
    .getByTestId("app-sidebar")
    .waitFor({ state: "visible", timeout: 60_000 });
  await page
    .getByTestId(`project-open-${seed.dtag}`)
    .click({ timeout: 60_000 });
  await page.getByTestId("project-tab-todos").click({ timeout: 30_000 });
  await expect(page.getByTestId("project-todos-screen")).toBeVisible({
    timeout: 60_000,
  });
  await expect(page.getByTestId("todo-list-picker")).toBeVisible({
    timeout: 60_000,
  });
}

test.describe("project to-do lists (live relay)", () => {
  test.skip(!enabled, "set BEEKEEPER_E2E_PROJECT_TODOS=1 to run the live gate");
  test.setTimeout(180_000);

  test("the tab writes to the relay and shows a collaborator's changes live", async ({
    page,
  }) => {
    const seed = await seedProject();
    await installRelayBridge(page, "tyler");
    await openTab(page, seed);

    await test.step("the creator creates a list and adds an item in the UI", async () => {
      await page.getByTestId("todo-list-new").click();
      await page.getByTestId("todo-list-create-title").fill("Launch");
      // Project-visible, not pinned: the pin test below pins it from the CLI.
      await page.getByTestId("todo-list-create-pinned").click();
      await page.getByTestId("todo-list-create-submit").click();
      await expect(page.getByTestId("todo-list-panel")).toBeVisible({
        timeout: 30_000,
      });
      await page.getByTestId("todo-add-input").fill("Write the NIP");
      await page.getByTestId("todo-add-submit").click();
      await expect(page.getByTestId("todo-open-section")).toContainText(
        "Write the NIP",
        { timeout: 30_000 },
      );
    });

    await test.step("the CLI reads back what the UI wrote", async () => {
      const shown = JSON.parse(
        await runCli(
          [
            "--format",
            "compact",
            "todos",
            "show",
            "--project",
            seed.coordinate,
            "Launch",
          ],
          TEST_IDENTITIES.alice.privateKey,
        ),
      ) as { open: { text: string }[]; completed: unknown[] };
      expect(shown.open.map((item) => item.text)).toEqual(["Write the NIP"]);
      expect(shown.completed).toEqual([]);
    });

    await test.step("a collaborator's add appears live, first, with a due date", async () => {
      await runCli(
        [
          "todos",
          "add",
          "--project",
          seed.coordinate,
          "Launch",
          "Ship the desktop tab",
          "--index",
          "0",
          "--due",
          "2000-01-01",
        ],
        TEST_IDENTITIES.alice.privateKey,
      );
      const open = page.getByTestId("todo-open-section");
      await expect(open).toContainText("Ship the desktop tab", {
        timeout: 30_000,
      });
      const texts = open.getByTestId("todo-item-text");
      await expect(texts.first()).toHaveText("Ship the desktop tab");
      await expect(
        open.locator('[data-testid="todo-item-due"][data-overdue="true"]'),
      ).toHaveCount(1, { timeout: 30_000 });
    });

    await test.step("a collaborator's done moves the item to Completed live", async () => {
      const shown = JSON.parse(
        await runCli(
          [
            "--format",
            "compact",
            "todos",
            "show",
            "--project",
            seed.coordinate,
            "Launch",
          ],
          TEST_IDENTITIES.alice.privateKey,
        ),
      ) as { open: { id: string; text: string }[] };
      const target = shown.open.find((item) => item.text === "Write the NIP");
      if (!target)
        throw new Error("the UI-added item is missing from the CLI fold");
      await runCli(
        ["todos", "done", "--project", seed.coordinate, target.id],
        TEST_IDENTITIES.alice.privateKey,
      );
      await expect(page.getByTestId("todo-completed-section")).toContainText(
        "Write the NIP",
        { timeout: 30_000 },
      );
      await expect(page.getByTestId("todo-open-section")).not.toContainText(
        "Write the NIP",
      );
    });

    await test.step("ticking the box in the UI lands on the relay", async () => {
      await page
        .getByTestId("todo-open-section")
        .getByTestId("todo-item-checkbox")
        .first()
        .click();
      await expect(page.getByTestId("todo-completed-section")).toContainText(
        "Ship the desktop tab",
        { timeout: 30_000 },
      );
      await expect
        .poll(
          async () => {
            const shown = JSON.parse(
              await runCli(
                [
                  "--format",
                  "compact",
                  "todos",
                  "show",
                  "--project",
                  seed.coordinate,
                  "Launch",
                ],
                TEST_IDENTITIES.alice.privateKey,
              ),
            ) as { open: unknown[]; completed: { text: string }[] };
            return shown.completed.map((item) => item.text).sort();
          },
          { timeout: 30_000 },
        )
        .toEqual(["Ship the desktop tab", "Write the NIP"]);
    });

    await test.step("the sidebar + menu lists work first, rooms below a rule, and creates a personal pinned list", async () => {
      await page.getByTestId(`project-create-${seed.dtag}`).click();
      const items = page.getByRole("menuitem");
      await expect(items).toHaveCount(5);
      const labels = await items.allTextContents();
      expect(labels.map((label) => label.trim())).toEqual([
        "New coding session",
        "New terminal",
        "New to-do list",
        "New channel",
        "New forum",
      ]);
      await page.getByTestId(`project-new-todo-list-${seed.dtag}`).click();
      await page.getByTestId("todo-list-create-title").fill("Only mine");
      await page.getByTestId("todo-list-visibility-personal").click();
      await page.getByTestId("todo-list-create-submit").click();
      // The create lands on the focused view of the new list alone …
      await expect(page.getByTestId("todo-focused")).toBeVisible({
        timeout: 30_000,
      });
      await expect(page.getByTestId("todo-focused")).toContainText("Only mine");
      await expect(page.getByTestId("todo-list-picker")).toHaveCount(0);
      await expect(page).toHaveURL(/\/todos\?list=[0-9a-f]{32}&view=list$/);
      // … and as a pinned sidebar row wearing the lock.
      const row = page
        .getByTestId(`project-group-${seed.dtag}`)
        .locator('[data-testid^="project-todo-list-row-"]');
      await expect(row).toHaveCount(1, { timeout: 30_000 });
      await expect(row).toContainText("Only mine");
      await expect(row.getByTestId("project-todo-list-personal")).toBeVisible();
      // "All lists" returns to the full tab, where the rail marks it.
      await page.getByTestId("todo-focused-all-lists").click();
      await expect(page.getByTestId("todo-list-picker")).toBeVisible();
      await expect(page.getByTestId("todo-list-personal")).toBeVisible();
      await expect(page.getByTestId("todo-list-pinned")).toBeVisible();
    });

    await test.step("a collaborator never sees the personal list", async () => {
      const lists = JSON.parse(
        await runCli(
          [
            "--format",
            "compact",
            "todos",
            "lists",
            "--project",
            seed.coordinate,
          ],
          TEST_IDENTITIES.alice.privateKey,
        ),
      ) as { lists: { title: string; visibility: string }[] };
      expect(lists.lists.map((list) => list.title)).toEqual(["Launch"]);
      expect(lists.lists[0]?.visibility).toBe("project");
    });

    await test.step("a collaborator's pin shows in the creator's sidebar live", async () => {
      await runCli(
        ["todos", "pin", "--project", seed.coordinate, "Launch"],
        TEST_IDENTITIES.alice.privateKey,
      );
      const rows = page
        .getByTestId(`project-group-${seed.dtag}`)
        .locator('[data-testid^="project-todo-list-row-"]');
      await expect(rows).toHaveCount(2, { timeout: 30_000 });
      await expect(rows.filter({ hasText: "Launch" })).toBeVisible();
      // A sidebar row opens the focused view of that list.
      await rows.filter({ hasText: "Launch" }).click();
      await expect(page.getByTestId("todo-focused")).toContainText("Launch", {
        timeout: 30_000,
      });
      await expect(page.getByTestId("todo-completed-section")).toContainText(
        "Ship the desktop tab",
      );
    });

    await waitForAnimations(page);
    await page.screenshot({
      path: "test-results/project-todos/live-tab.png",
      clip: { x: 0, y: 0, width: 1280, height: 720 },
    });
  });
});
