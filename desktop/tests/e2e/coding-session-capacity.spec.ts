import { expect, test } from "@playwright/test";

import { installMockBridge } from "../helpers/bridge";

/**
 * The session ceiling as a setting.
 *
 * It used to be a compiled-in 4 that surfaced only as a refusal, and read as
 * the model vendor's limit ("there can only be 4 concurrent Claude sessions?",
 * 2026-08-24). The panel has to do three things: say how many are running, let
 * a person set the number or remove it, and admit when a saved change is not
 * yet in force — the provider reads its ceiling at startup, so saving is a
 * promise about the next start, not a change to the running one.
 */

const PROVIDER_PUBKEY = "f".repeat(64);

async function openSessionsSettings(page: import("@playwright/test").Page) {
  await installMockBridge(page, {
    codingSessionProviderStatus: {
      provisioned: true,
      running: true,
      providerPubkey: PROVIDER_PUBKEY,
      instanceId: "0123456789abcdef",
    },
  });
  await page.goto("/");
  await page.getByTestId("open-settings").click();
  await page.getByTestId("profile-popover-settings").click();
  await expect(page.getByTestId("settings-view")).toBeVisible();
  await page.getByTestId("settings-nav-sessions").click();
  await expect(page.getByTestId("settings-coding-sessions")).toBeVisible();
}

test("Sessions settings state the limit, the running count, and who set it", async ({
  page,
}) => {
  await openSessionsSettings(page);

  await expect(page.getByTestId("settings-coding-sessions")).toContainText(
    "not your model provider's",
  );
  await expect(page.getByTestId("coding-session-running-count")).toBeVisible();
  // Nothing stored yet, so the panel names the provider's own default.
  await expect(
    page.getByTestId("settings-coding-session-capacity"),
  ).toContainText("Limit: 4 sessions");
});

test("a saved limit admits it is not yet in force", async ({ page }) => {
  await openSessionsSettings(page);

  const input = page.getByTestId("coding-session-capacity-input");
  await input.fill("9");
  await page.getByTestId("coding-session-capacity-save").click();

  await expect(
    page.getByTestId("settings-coding-session-capacity"),
  ).toContainText("Limit: 9 sessions");
  // The running provider started with the default, and the panel says so
  // rather than implying nine is being enforced.
  await expect(
    page.getByTestId("coding-session-capacity-pending"),
  ).toContainText("started with 4 sessions");
  await expect(
    page.getByTestId("coding-session-capacity-pending"),
  ).toContainText("next time it starts");
});

test("unlimited is a choice, and it says what it costs", async ({ page }) => {
  await openSessionsSettings(page);

  await page.getByTestId("coding-session-capacity-unlimited").click();

  await expect(
    page.getByTestId("settings-coding-session-capacity"),
  ).toContainText("Limit: Unlimited");
  await expect(
    page.getByTestId("coding-session-capacity-unlimited-note"),
  ).toContainText("agent process on this computer");
  // The number field is meaningless while unlimited, so it is disabled rather
  // than left showing a limit that is not applied.
  await expect(
    page.getByTestId("coding-session-capacity-input"),
  ).toBeDisabled();
  // And the way back is offered.
  await expect(
    page.getByTestId("coding-session-capacity-unlimited"),
  ).toHaveText("Set a limit");
});

// Reported 2026-08-24: two turns died as "Idle timeout — no agent activity for
// 900s" while a long command ran. The budget is the person's now, in the same
// panel and with the same honesty about when it takes effect.
test("the silent-turn budget is settable and says what it measures", async ({
  page,
}) => {
  await openSessionsSettings(page);

  const panel = page.getByTestId("settings-coding-session-capacity");
  await expect(panel).toContainText("Now: 15 minutes");
  await expect(panel).toContainText("budget for silence");
  await expect(panel).toContainText("build, a test suite");

  await page.getByTestId("coding-session-idle-timeout-input").fill("45");
  await page.getByTestId("coding-session-idle-timeout-save").click();

  await expect(panel).toContainText("Now: 45 minutes");
  // The running provider read 15 minutes at startup, and the panel says so
  // rather than implying 45 is already in force.
  await expect(
    page.getByTestId("coding-session-idle-timeout-pending"),
  ).toContainText("gives up after 15 minutes of silence");
  await expect(
    page.getByTestId("coding-session-idle-timeout-pending"),
  ).toContainText("next time it starts");

  // And it can be handed back to the provider.
  await page.getByTestId("coding-session-idle-timeout-default").click();
  await expect(panel).toContainText("Now: 15 minutes");
  await expect(
    page.getByTestId("coding-session-idle-timeout-pending"),
  ).toHaveCount(0);
});
