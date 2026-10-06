import { mkdirSync } from "node:fs";
import { execFileSync } from "node:child_process";
import { expect, test, type Page } from "@playwright/test";
import { waitForAnimations } from "../helpers/animations";
import { overridePreviewFeatures } from "../helpers/features";
import { installMockBridge } from "../helpers/bridge";
import {
  KIND_PROJECT,
  KIND_PROJECT_PACK_SOURCE,
} from "@/shared/constants/kinds";

const owner = "a1".repeat(32);
const address = `${KIND_PROJECT}:${owner}:general`;
const repo = `30617:${owner}:general-beekeeper-agents`;
const snapshotSha = "182304e";
// Real committed bytes; transport is explicitly the mock bridge.
const agents =
  process.env.MEMORY_EXPLORER_AGENTS_REPO ?? "../../agiterra-beekeeper-agents";
const readPublished = (path: string) =>
  execFileSync("git", ["-C", agents, "show", `${snapshotSha}:${path}`], {
    encoding: "utf8",
    maxBuffer: 8 * 1024 * 1024,
  });
const realFiles = Object.fromEntries(
  [
    "plans/CURRENT_STATE.md",
    "plans/SESSION_STATE.md",
    "plans/SESSION_VIEW_PARITY_PLAN.md",
  ].map((path) => [path, readPublished(path)]),
);
const fullSha = execFileSync("git", ["-C", agents, "rev-parse", snapshotSha], {
  encoding: "utf8",
}).trim();
const out = "test-results/memory-explorer";
mkdirSync(out, { recursive: true });

async function setup(
  page: Page,
  files: Record<string, string>,
  enabled = true,
  source = true,
) {
  const events = [
    {
      id: "project-memory".padEnd(64, "0"),
      pubkey: owner,
      created_at: 1,
      kind: KIND_PROJECT,
      tags: [
        ["d", "general"],
        ["name", "General"],
        ["channel", "9a1657ac-f7aa-5db0-b632-d8bbeb6dfb50"],
      ],
      content: "",
      sig: "0".repeat(128),
    },
  ];
  if (source)
    events.push({
      id: "source-memory".padEnd(64, "0"),
      pubkey: owner,
      created_at: 2,
      kind: KIND_PROJECT_PACK_SOURCE,
      tags: [
        ["d", address],
        ["repo", repo],
        ["ref", "refs/heads/main"],
        ["path", "."],
      ],
      content: "",
      sig: "0".repeat(128),
    });
  await page.addInitScript(
    ({ events, files, enabled, repo, fullSha }) => {
      localStorage.setItem(
        "buzz-feature-overrides-v1",
        JSON.stringify({ projects: true, "memory-explorer": enabled }),
      );
      (
        window as unknown as Record<string, unknown>
      ).__BEEKEEPER_E2E_EXTRA_PROJECT_EVENTS__ = events;
      (
        window as unknown as Record<string, unknown>
      ).__BEEKEEPER_E2E_AGENTS_REPO__ = {
        listing: {
          repo,
          branch: "main",
          commit: fullSha,
          syncedAt: "2026-10-06T00:00:00Z",
          entries: Object.entries(files).map(([path, text]) => ({
            path,
            blob: "a".repeat(40),
            size: new TextEncoder().encode(text).length,
            kind: "other",
          })),
        },
        files,
        commitResults: [],
      };
    },
    { events, files, enabled, repo, fullSha },
  );
  await overridePreviewFeatures(page, { "memory-explorer": enabled });
  await installMockBridge(page, {});
  await page.setViewportSize({ width: 1280, height: 900 });
  await page.goto("/");
  await expect(page.getByTestId("project-group-general")).toBeVisible({
    timeout: 15000,
  });
  await page.getByTestId("project-group-general").hover();
  await page.getByTestId("project-open-general").click();
  await page.getByTestId("project-tab-files").click();
  await expect(page.getByTestId("agents-repo-screen")).toBeVisible();
}
async function explore(page: Page) {
  await page.getByRole("button", { name: "Explore", exact: true }).click();
  await expect(page.getByTestId("memory-explorer")).toBeVisible();
  await expect(
    page.getByText("Reading and indexing committed documents…"),
  ).toHaveCount(0, { timeout: 30000 });
}
async function shot(page: Page, name: string) {
  await waitForAnimations(page);
  await page
    .getByTestId("memory-explorer")
    .screenshot({ path: `${out}/${name}.png` });
}

test("real snapshot bytes through MOCK transport: evidence traversal, exact source, comparison and Back", async ({
  page,
}) => {
  await setup(page, realFiles);
  await explore(page);
  await expect(page.getByTestId("memory-explorer")).toContainText(
    fullSha.slice(0, 12),
  );
  await page.getByLabel("Search indexed documents").fill("SV-77");
  await page
    .locator(".explorer-list .explorer-node")
    .filter({ hasText: "SV-77 ·" })
    .first()
    .click();
  await expect(page.locator(".explorer-reader h2")).toContainText("SV-77");
  const graph = page.getByTestId("explorer-graph");
  await expect(graph.locator(".explorer-edges path")).not.toHaveCount(0);
  expect(await graph.locator(".explorer-node").count()).toBeLessThanOrEqual(7);
  const graphWidth =
    (await page.locator(".explorer-connections").boundingBox())?.width ?? 0;
  const readerWidth =
    (await page.locator(".explorer-reader").boundingBox())?.width ?? 0;
  expect(graphWidth).toBeGreaterThan(readerWidth);
  await expect(
    page.getByTestId("explorer-read").getByRole("heading").first(),
  ).toBeVisible();
  await expect(page.getByTestId("explorer-source")).toHaveCount(0);
  await expect(page.locator(".explorer-provenance")).not.toHaveAttribute(
    "open",
  );
  await page.locator(".explorer-provenance summary").click();
  await expect(page.locator(".explorer-provenance")).toContainText(fullSha);
  await page.locator(".explorer-provenance summary").click();
  await shot(page, "01-sv77");
  await page
    .locator(".explorer-connections .explorer-node")
    .filter({ hasText: "Ledger 336 ·" })
    .click();
  await page.getByRole("button", { name: "Source", exact: true }).click();
  await expect(page.getByTestId("explorer-source")).toContainText(
    "336. **2026-10-05",
  );
  await page.getByRole("button", { name: "Back", exact: true }).click();
  await page
    .locator(".explorer-connections .explorer-node")
    .filter({ hasText: "Ledger 340 ·" })
    .click();
  await page.getByRole("button", { name: "Compare", exact: true }).click();
  await page
    .locator(".explorer-connections .explorer-node")
    .filter({ hasText: "SV-89 ·" })
    .click();
  await expect(page.locator(".explorer-reader")).toContainText(
    "Source claims side by side",
  );
  await shot(page, "02-comparison");
  await page
    .locator(".explorer-connections .explorer-node")
    .filter({ hasText: "Ledger 341 ·" })
    .click();
  await page.getByRole("button", { name: "Source", exact: true }).click();
  await expect(page.getByTestId("explorer-source")).toContainText(
    "closes SV-89",
  );
  await shot(page, "03-ledger341");
});

test("generic repository: explicit links, anchors, unindexed archives and keyboard/narrow reader", async ({
  page,
}) => {
  await setup(page, {
    "README.md":
      "# Field notes\n\n[Garden](notes/garden.md#observations)\n\n[Archive](archive/old.md)\n",
    "notes/garden.md":
      "# Garden\n\n## Observations\n\nRain helped the beans.\n\n| Thing | Note |\n| --- | --- |\n| Beans | a \\| b |\n",
    "archive/old.md": "# Earlier\n\nHistorical note.\n",
    "archive/unvisited.md": "# Unvisited\n",
  });
  await explore(page);
  await expect(
    page.getByText("Partially indexed", { exact: false }),
  ).toBeVisible();
  await page
    .locator(".explorer-reader")
    .getByText("Garden", { exact: true })
    .click();
  await expect(page.locator(".explorer-reader")).toContainText(
    "Rain helped the beans",
  );
  await expect(page.locator(".explorer-reader")).not.toContainText(
    "Source status claim",
  );
  await page.getByRole("button", { name: "Source", exact: true }).focus();
  await page.keyboard.press("Enter");
  await expect(page.getByTestId("explorer-source")).toContainText(
    "## Observations",
  );
  await page.getByRole("button", { name: "Read", exact: true }).click();
  await expect(
    page
      .getByTestId("explorer-read")
      .getByRole("heading", { name: "Observations", exact: true }),
  ).toBeVisible();
  await expect(page.getByTestId("explorer-read").locator("table")).toHaveCount(
    1,
  );
  await shot(page, "04-generic");
  await page.setViewportSize({ width: 800, height: 700 });
  await shot(page, "05-narrow");
  await page.getByRole("button", { name: "Back", exact: true }).click();
  await expect(page.locator(".explorer-reader h2")).toContainText(
    "Field notes",
  );
});

test("flag off and unsupported direct Explore URL preserve Artifacts", async ({
  page,
}) => {
  await setup(page, { "README.md": "# Ordinary\n" }, false);
  await expect(
    page.getByRole("button", { name: "Explore", exact: true }),
  ).toHaveCount(0);
  await page.goto("/#/projects/general/files?view=explore");
  await expect(page.getByTestId("agents-repo-screen")).toBeVisible();
  await expect(page.getByTestId("memory-explorer")).toHaveCount(0);
});

test("no source is disclosed without a fabricated graph", async ({ page }) => {
  await setup(page, {}, true, false);
  await page.getByRole("button", { name: "Explore", exact: true }).click();
  await expect(
    page.getByText("This project has no configured agents repository.", {
      exact: false,
    }),
  ).toBeVisible();
  await expect(page.locator(".explorer-node")).toHaveCount(0);
});

test("flag off during a pending mock read unmounts and ignores its late result", async ({
  page,
}) => {
  await setup(page, { "README.md": "# Pending old project\n" });
  await page.evaluate(() => {
    const seed = window.__BEEKEEPER_E2E_AGENTS_REPO__;
    if (seed) seed.explorerReadDelayMs = 800;
  });
  await page.getByRole("button", { name: "Explore", exact: true }).click();
  await expect(page.getByTestId("memory-explorer")).toBeVisible();
  await page.evaluate(() => {
    const key = "buzz-feature-overrides-v1";
    localStorage.setItem(
      key,
      JSON.stringify({
        ...JSON.parse(localStorage.getItem(key) ?? "{}"),
        "memory-explorer": false,
      }),
    );
    window.dispatchEvent(new StorageEvent("storage", { key }));
  });
  await expect(page.getByTestId("agents-repo-screen")).toBeVisible();
  await expect(page.getByTestId("memory-explorer")).toHaveCount(0);
  await page.getByTestId("agents-repo-refresh").click();
  await expect(page.getByTestId("memory-explorer")).toHaveCount(0);
});

test("unavailable source and over-limit blob disclose failure", async ({
  page,
}) => {
  await setup(page, { "README.md": "# Not a successful read\n" });
  await page.evaluate(() => {
    const seed = window.__BEEKEEPER_E2E_AGENTS_REPO__;
    if (seed) seed.explorerError = "Repository unavailable";
  });
  await page.getByRole("button", { name: "Explore", exact: true }).click();
  await expect(page.getByRole("alert")).toContainText("Repository unavailable");
  await page.evaluate(() => {
    const seed = window.__BEEKEEPER_E2E_AGENTS_REPO__;
    if (seed) {
      delete seed.explorerError;
      seed.listing.entries[0].size = 4 * 1024 * 1024 + 1;
    }
  });
  await page.getByRole("button", { name: "Refresh", exact: true }).click();
  await expect(page.locator(".explorer-list")).toContainText("too-large");
  await expect(page.locator(".explorer-reader")).not.toContainText(
    "Not a successful read",
  );
});
