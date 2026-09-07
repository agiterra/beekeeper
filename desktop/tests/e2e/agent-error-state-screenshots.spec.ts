/**
 * Screenshot spec for structured agent provider error states (PR #1653).
 *
 * Exercises the two visible surfaces where friendly error copy appears:
 *   - Agent card avatar badge (CircleAlert icon + tooltip) for stopped agents
 *     with a lastError / lastErrorCode.
 *
 * An earlier note here promised a second surface, `ManagedAgentRow`, once a
 * follow-up wired it into a route. That never happened: the row and its only
 * caller were never reachable and were deleted, so the card badge below is the
 * whole of what this spec covers.
 */

import { expect, test } from "@playwright/test";

import { installMockBridge, TEST_IDENTITIES } from "../helpers/bridge";
import { waitForAnimations } from "../helpers/animations";
import { openDashboardTab } from "../helpers/dashboard";

const SHOTS = "test-results/pr-1653-screenshots";

// Two stopped agents — one with a structured -32002 code, one with a raw string.
const MODEL_NOT_FOUND_AGENT = {
  pubkey: TEST_IDENTITIES.alice.pubkey,
  name: "Databricks Agent",
  status: "stopped" as const,
  lastError:
    "Agent reported error (code -32002): llm model not found: (goose-databricks-llama-3-3-70b) 404 Not Found: model not found",
  lastErrorCode: -32002,
};

const GENERIC_ERROR_AGENT = {
  pubkey: TEST_IDENTITIES.bob.pubkey,
  name: "Local Agent",
  status: "stopped" as const,
  lastError: "harness exited with status 1",
  lastErrorCode: null,
};

async function gotoAgentsView(page: import("@playwright/test").Page) {
  await page.goto("/", { waitUntil: "domcontentloaded" });
  await expect(page.getByTestId("open-agents-view")).toBeVisible({
    timeout: 10_000,
  });
  await openDashboardTab(page, "agents");
  await expect(page.getByTestId("agent-filters")).toBeVisible({
    timeout: 10_000,
  });
}

test.describe("agent error state screenshots", () => {
  test.use({ viewport: { width: 1280, height: 900 } });

  test.beforeEach(async ({ page }) => {
    page.on("pageerror", (err) => {
      console.error(
        "PAGE ERROR:",
        err.message,
        err.stack?.split("\n").slice(0, 5).join("\n"),
      );
    });
  });

  // Shot 01: agent card with model-not-found error badge (red CircleAlert).
  // The badge title shows the friendly structured copy instead of the raw
  // JSON error string.
  test("01-model-not-found-error-badge", async ({ page }) => {
    await installMockBridge(page, {
      managedAgents: [MODEL_NOT_FOUND_AGENT],
    });

    await gotoAgentsView(page);

    // Wait for the error badge to appear (stopped agent with error).
    const errorBadge = page.getByTestId(
      `agent-runtime-error-${MODEL_NOT_FOUND_AGENT.pubkey}`,
    );
    await expect(errorBadge).toBeVisible({ timeout: 10_000 });
    await expect(errorBadge).toContainText(
      "The configured model is not available",
    );
    await waitForAnimations(page);

    // Capture the agent card element.
    const agentCard = page.getByTestId("agent-row").filter({ has: errorBadge });
    await agentCard.screenshot({
      path: `${SHOTS}/01-model-not-found-error-badge.png`,
    });
  });

  // Shot 02: agent card with generic (unclassified) error badge.
  // The badge is present but the tooltip shows the raw exit string, not
  // structured copy — demonstrating the error is still surfaced for any
  // harness exit.
  test("02-generic-error-badge", async ({ page }) => {
    await installMockBridge(page, {
      managedAgents: [GENERIC_ERROR_AGENT],
    });

    await gotoAgentsView(page);

    const errorBadge = page.getByTestId(
      `agent-runtime-error-${GENERIC_ERROR_AGENT.pubkey}`,
    );
    await expect(errorBadge).toBeVisible({ timeout: 10_000 });
    await expect(errorBadge).toContainText("harness exited with status 1");
    await waitForAnimations(page);

    const agentCard = page.getByTestId("agent-row").filter({ has: errorBadge });
    await agentCard.screenshot({
      path: `${SHOTS}/02-generic-error-badge.png`,
    });
  });

  // Shot 03: side-by-side — both agents in the same view so the reviewer can
  // see error badges on all stopped agents in a real usage context.
  test("03-agents-section-both-errors", async ({ page }) => {
    await installMockBridge(page, {
      managedAgents: [MODEL_NOT_FOUND_AGENT, GENERIC_ERROR_AGENT],
    });

    await gotoAgentsView(page);

    // Wait for both error badges to be present.
    await expect(
      page.getByTestId(`agent-runtime-error-${MODEL_NOT_FOUND_AGENT.pubkey}`),
    ).toBeVisible({ timeout: 10_000 });
    await expect(
      page.getByTestId(`agent-runtime-error-${GENERIC_ERROR_AGENT.pubkey}`),
    ).toBeVisible();
    await waitForAnimations(page);

    // Capture the full agents section (scroll-bounded crop to the section).
    const section = page.getByTestId("agent-filters").locator("..");
    await section.screenshot({
      path: `${SHOTS}/03-agents-section-both-errors.png`,
    });
  });
});
