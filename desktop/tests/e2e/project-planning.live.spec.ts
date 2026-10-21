import { execFile } from "node:child_process";
import { mkdirSync } from "node:fs";
import { promisify } from "node:util";

import { expect, test, type Page } from "@playwright/test";

import { waitForAnimations } from "../helpers/animations";
import { installRelayBridge, TEST_IDENTITIES } from "../helpers/bridge";

const exec = promisify(execFile);

// Live gate for the project Planning tab against a REAL relay running the
// Planning plugin: the app (as tyler, the project's creator) creates an
// issue with "New issue" and sets its status in the detail — both through
// the plugin route, signed as tyler; a reload must bring both back from the
// relay, `bee todos show` (as alice, a collaborator, no plugin involved)
// must fold the same status, and `bee planning status` must change it from
// the CLI through the same plugin.
// Proves what the mock spec (project-planning.spec.ts) cannot: the relay's
// admission of the UI's item.status op, and persistence past the page.
//
// Requires: BEEKEEPER_E2E_PROJECT_PLANNING=1, BEEKEEPER_E2E_CLI_BIN (a built
// `bee`), and BEEKEEPER_E2E_RELAY_URL pointing at a running relay.
const enabled = process.env.BEEKEEPER_E2E_PROJECT_PLANNING === "1";

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

type Seed = { coordinate: string; dtag: string };

async function seedProject(): Promise<Seed> {
  const dtag = `planning-live-${process.pid}-${Date.now().toString(36)}`;
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
      `Planning live ${dtag}`,
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
  const coordinate = `30621:${tyler.pubkey}:${dtag}`;
  // One list, so "New issue" files into it rather than creating one.
  await runCli(
    ["todos", "create-list", "--project", coordinate, "--title", "Launch"],
    tyler.privateKey,
  );
  return { coordinate, dtag };
}

async function openPlanning(page: Page, seed: Seed) {
  await page.goto("/");
  await page
    .getByTestId("app-sidebar")
    .waitFor({ state: "visible", timeout: 60_000 });
  await page
    .getByTestId(`project-open-${seed.dtag}`)
    .click({ timeout: 60_000 });
  await page.getByTestId("project-tab-planning").click({ timeout: 30_000 });
  await expect(page.getByTestId("planning-screen")).toBeVisible({
    timeout: 60_000,
  });
}

type Shown = {
  open: { id: string; text: string; status: string; assignee: string | null }[];
  completed: {
    id: string;
    text: string;
    status: string;
    assignee: string | null;
  }[];
};

async function showAsAlice(seed: Seed): Promise<Shown> {
  return JSON.parse(
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
  ) as Shown;
}

test.describe("project planning (live relay)", () => {
  test.skip(
    !enabled,
    "set BEEKEEPER_E2E_PROJECT_PLANNING=1 to run the live gate",
  );
  test.setTimeout(180_000);

  test("an issue created and moved in the UI comes back from the relay after a reload", async ({
    page,
  }) => {
    const title = `Planning live issue ${Date.now().toString(36)}`;
    const seed = await seedProject();
    await installRelayBridge(page, "tyler");
    await openPlanning(page, seed);
    await expect(page.getByTestId("planning-view")).toHaveAttribute(
      "data-access",
      "writable",
      { timeout: 60_000 },
    );

    await test.step("New issue writes an issue to the relay", async () => {
      await page.getByTestId("planning-new-issue").click();
      await expect(page.getByTestId("planning-quick-create")).toBeVisible();
      await page.getByTestId("planning-quick-create-title").fill(title);
      await page.getByTestId("planning-quick-create-submit").click();
      await expect(page.getByTestId("planning-quick-create")).toBeHidden({
        timeout: 30_000,
      });
      await expect(
        page.getByTestId("planning-list").getByText(title),
      ).toBeVisible({ timeout: 30_000 });
    });

    await test.step("the detail sets its status to In Progress", async () => {
      const row = page
        .getByTestId("planning-list")
        .locator("li", { hasText: title });
      await row.getByRole("button", { name: new RegExp(title) }).click();
      await expect(page.getByTestId("planning-detail")).toBeVisible({
        timeout: 30_000,
      });
      await page.getByTestId("planning-detail-status").click();
      await page.getByTestId("planning-status-option-in_progress").click();
      await expect(page.getByTestId("planning-detail-status")).toContainText(
        "In Progress",
        { timeout: 30_000 },
      );
    });

    let issueId = "";
    await test.step("a collaborator's CLI folds the same status", async () => {
      await expect
        .poll(
          async () => {
            const shown = await showAsAlice(seed);
            const issue = shown.open.find((item) => item.text === title);
            issueId = issue?.id ?? "";
            return issue?.status ?? null;
          },
          { timeout: 30_000 },
        )
        .toBe("in_progress");
    });

    await test.step("the detail assigns it to alice", async () => {
      // An assignee is data — no access grant, no p tag — held to the same
      // rule as a direct To-Do assignment; the app re-seals before signing.
      await page.getByTestId("planning-detail-assignee").click();
      await page
        .getByTestId(`todo-assignee-option-${TEST_IDENTITIES.alice.pubkey}`)
        .click();
      await expect
        .poll(
          async () => {
            const shown = await showAsAlice(seed);
            return (
              shown.open.find((item) => item.text === title)?.assignee ?? null
            );
          },
          { timeout: 30_000 },
        )
        .toBe(TEST_IDENTITIES.alice.pubkey);
    });

    await test.step("a reload reads the issue and its status back from the relay", async () => {
      await page.reload({ waitUntil: "domcontentloaded" });
      await expect(page.getByTestId("planning-screen")).toBeVisible({
        timeout: 60_000,
      });
      await expect(
        page.getByTestId("planning-list").getByText(title),
      ).toBeVisible({ timeout: 60_000 });
      await expect(
        page.getByTestId(`planning-row-status-${issueId}`),
      ).toHaveAccessibleName(/In Progress/, { timeout: 30_000 });
    });

    await test.step("the CLI cancels it through the Planning plugin; the UI and a collaborator's fold both read Canceled", async () => {
      const read = JSON.parse(
        await runCli(
          [
            "planning",
            "read",
            "--project",
            seed.coordinate,
            "--provider",
            "planning",
          ],
          TEST_IDENTITIES.tyler.privateKey,
        ),
      ) as { result: { issues: { key: string; title: string }[] } };
      const key = read.result.issues.find((i) => i.title === title)?.key;
      expect(key).toBeTruthy();
      const out = JSON.parse(
        await runCli(
          [
            "planning",
            "status",
            "--project",
            seed.coordinate,
            "--provider",
            "planning",
            "--issue",
            key as string,
            "--status",
            "canceled",
          ],
          TEST_IDENTITIES.tyler.privateKey,
        ),
      ) as { published?: { group: string }[]; failed?: unknown[] };
      // Two events drafted by the plugin, item.done then item.status, both
      // signed by tyler's own key and accepted.
      expect(out.published?.map((p) => p.group)).toEqual(["status", "status"]);
      expect(out.failed).toEqual([]);
      await expect
        .poll(
          async () => {
            const shown = await showAsAlice(seed);
            return (
              shown.completed.find((item) => item.id === issueId)?.status ??
              null
            );
          },
          { timeout: 30_000 },
        )
        .toBe("canceled");
      await page.reload({ waitUntil: "domcontentloaded" });
      await expect(
        page.getByTestId(`planning-row-status-${issueId}`),
      ).toHaveAccessibleName(/Canceled/, { timeout: 60_000 });
      mkdirSync("test-results/planning", { recursive: true });
      await waitForAnimations(page);
      await page.screenshot({
        path: "test-results/planning/11-live-canceled.png",
      });
    });
  });
});
