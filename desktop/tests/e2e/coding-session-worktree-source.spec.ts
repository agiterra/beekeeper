import { expect, test } from "@playwright/test";

import { installMockBridge } from "../helpers/bridge";

/**
 * The worktree source picker, proven against the bug it exists for.
 *
 * Sessions used to branch from whatever the checkout had checked out, so a
 * checkout parked on an old topic branch silently became the ancestor of
 * every "new" session made from it. The picker must therefore sit on the
 * trunk by default — not the checkout's HEAD — while still offering every
 * existing branch.
 */

const PROVIDER_PUBKEY = "f".repeat(64);

async function openCreateDialog(page: import("@playwright/test").Page) {
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
      {
        instanceRef: "claude-primary",
        runtime: "claude",
        driver: "claude-acp",
        label: "Claude Code",
        authState: "ready",
        defaultModel: "default",
        allowedModels: ["default"],
        capabilities: {
          threadTurnStart: true,
          threadTurnInterrupt: true,
          threadSteer: false,
          context: false,
          diff: false,
          plan: true,
        },
      },
    ],
    // The scenario: the checkout is parked on `old-topic`, and the trunk
    // exists. The default must be the trunk.
    codingSessionWorktreeBranches: {
      branches: ["old-topic", "main", "feature-x"],
      defaultBranch: "main",
      headBranch: "old-topic",
    },
  });
  await page.goto("/");
  await page.getByTestId("channel-engineering").click();
  await page.getByTestId("channel-coding-sessions-trigger").click();
  await page.getByTestId("channel-coding-sessions-new").click();
  await expect(page.getByTestId("new-coding-session-form")).toBeVisible();
}

test("the source picker defaults to the trunk, not the parked checkout", async ({
  page,
}) => {
  await openCreateDialog(page);
  await page.getByRole("button", { name: "Change setup", exact: true }).click();

  // No workdir yet: there is no repository to list, so no picker.
  await expect(page.getByTestId("coding-session-worktree-source")).toHaveCount(
    0,
  );

  await page
    .getByTestId("coding-session-workdir-input")
    .fill("/Users/mock/Code/beekeeper");

  const picker = page.getByTestId("coding-session-worktree-source");
  await expect(picker).toBeVisible();
  // `main`, even though the mocked checkout is parked on `old-topic`.
  await expect(picker).toHaveValue("main");
  await expect(picker.locator("option")).toHaveText([
    "old-topic",
    "main (default)",
    "feature-x",
  ]);

  // An existing branch can be chosen instead.
  await picker.selectOption("feature-x");
  await expect(picker).toHaveValue("feature-x");
});
