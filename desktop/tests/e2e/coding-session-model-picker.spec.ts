import { expect, test } from "@playwright/test";

import { installMockBridge } from "../helpers/bridge";

/**
 * The picker's keyboard, proven rather than drawn.
 *
 * The first version of this control rendered `⌘1`, `⌘2` hints beside every
 * favourite with nothing listening for them — a shortcut that was only a
 * picture of one (§2 item 47). A static render cannot tell the difference, so
 * the assertions here are keystrokes.
 */

const PROVIDER_PUBKEY = "f".repeat(64);

function runtime(input: {
  instanceRef: string;
  runtime: string;
  label: string;
  allowedModels: string[];
}) {
  return {
    instanceRef: input.instanceRef,
    runtime: input.runtime,
    driver: `${input.runtime}-acp`,
    label: input.label,
    authState: "ready" as const,
    defaultModel: input.allowedModels[0],
    allowedModels: input.allowedModels,
    capabilities: {
      threadTurnStart: true,
      threadTurnInterrupt: true,
      threadSteer: false,
      context: false,
      diff: false,
      plan: true,
    },
  };
}

async function openCreateScreen(page: import("@playwright/test").Page) {
  await installMockBridge(page, {
    globalAgentConfig: {
      env_vars: {},
      provider: null,
      model: null,
      "allowed-bridge-pubkeys": [
        { pubkey: PROVIDER_PUBKEY, label: "This computer (coding sessions)" },
      ],
    },
    codingSessionProviderStatus: {
      provisioned: true,
      running: true,
      providerPubkey: PROVIDER_PUBKEY,
      instanceId: "0123456789abcdef",
    },
    codingSessionProviderRuntimes: [
      runtime({
        instanceRef: "claude-primary",
        runtime: "claude",
        label: "Claude Code",
        allowedModels: ["default", "opus[1m]", "sonnet", "haiku"],
      }),
      runtime({
        instanceRef: "codex-primary",
        runtime: "codex",
        label: "Codex",
        allowedModels: [
          "gpt-5.6-terra",
          "gpt-5.6-luna[high]",
          "gpt-5.6-luna[max]",
        ],
      }),
    ],
  });
  await page.goto("/");
  await page.getByTestId("channel-engineering").click();
  await page.getByTestId("channel-coding-sessions-trigger").click();
  await page.getByTestId("channel-coding-sessions-new").click();
  await expect(page.getByTestId("coding-session-model-picker")).toBeVisible();
}

test("arrow keys and Enter pick a model without touching the mouse", async ({
  page,
}) => {
  await openCreateScreen(page);
  const trigger = page.getByTestId("coding-session-model-picker");
  const first = await trigger.textContent();

  await trigger.click();
  await expect(page.getByTestId("coding-session-model-search")).toBeVisible();
  // The first row is active on open, so one ArrowDown lands on the second.
  await page.keyboard.press("ArrowDown");
  await page.keyboard.press("Enter");

  await expect(
    page.getByTestId("coding-session-model-picker-panel"),
  ).toHaveCount(0);
  await expect(trigger).not.toHaveText(first ?? "");
});

test("a starred model gets a working ⌘N, not a picture of one", async ({
  page,
}) => {
  await openCreateScreen(page);
  const trigger = page.getByTestId("coding-session-model-picker");

  // Star the second row, which is not the current selection.
  await trigger.click();
  const rows = page.getByTestId("coding-session-model-row");
  const starred = rows.nth(1);
  const starredName = await starred
    .getByTestId("coding-session-model-row-select")
    .textContent();
  await starred.getByTestId("coding-session-model-favorite").click();
  // The hint appears the moment the pin does…
  await expect(starred).toContainText("⌘1");

  // …and the key it names picks that model, in the same breath.
  await page.keyboard.press("Meta+1");
  await expect(
    page.getByTestId("coding-session-model-picker-panel"),
  ).toHaveCount(0);
  // The trigger now names the model that was starred, by its display name.
  const picked = await trigger.textContent();
  expect(starredName ?? "").toContain(picked ?? "");
});

test("search narrows across providers and the rail filters to one", async ({
  page,
}) => {
  await openCreateScreen(page);
  await page.getByTestId("coding-session-model-picker").click();
  await page.getByTestId("coding-session-model-rail-codex").click();
  await expect(page.getByTestId("coding-session-model-row")).toHaveCount(2);

  await page.getByTestId("coding-session-model-search").fill("luna");
  await expect(page.getByTestId("coding-session-model-row")).toHaveCount(1);
  await expect(page.getByTestId("coding-session-model-row")).toContainText(
    "GPT-5.6 Luna",
  );

  // A token that matches nothing empties the list rather than widening it.
  await page.getByTestId("coding-session-model-search").fill("luna wombat");
  await expect(page.getByTestId("coding-session-model-empty")).toBeVisible();
});

test("Escape closes the panel and leaves the selection alone", async ({
  page,
}) => {
  await openCreateScreen(page);
  const trigger = page.getByTestId("coding-session-model-picker");
  const before = await trigger.textContent();

  await trigger.click();
  await expect(
    page.getByTestId("coding-session-model-picker-panel"),
  ).toBeVisible();
  await page.keyboard.press("Escape");

  await expect(
    page.getByTestId("coding-session-model-picker-panel"),
  ).toHaveCount(0);
  await expect(trigger).toHaveText(before ?? "");
});
