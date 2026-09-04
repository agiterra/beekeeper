import { expect, test } from "@playwright/test";

import { installMockBridge, TEST_IDENTITIES } from "../helpers/bridge";

/**
 * LANE-L33 — the shared home is disclosed, and there is a way out of it.
 *
 * Finding 68: on this machine every managed agent spawned in the one directory
 * they all share (`~/.beekeeper-dev`), so the pack-skills write was refused by
 * a correct guard and the agent ran with no role skills at all. The host log
 * said so; nothing on any screen did. An agent whose craft is silently absent
 * while the app shows its role is exactly the class of untruth this app treats
 * as a bug of the same severity as a crash.
 *
 * So this drives what an operator can actually see: the card names the
 * refusal, the profile names it and offers the remedy, and after the remedy
 * the app stops saying it — because the host has moved the agent, not because
 * the screen decided to be quiet.
 */

const REFUSED_AGENT = TEST_IDENTITIES.charlie;
const HEALTHY_AGENT = TEST_IDENTITIES.bob;

test("an agent whose shared home refuses its pack says so, and can be given a nest", async ({
  page,
}) => {
  await installMockBridge(page, {
    managedAgents: [
      {
        pubkey: REFUSED_AGENT.pubkey,
        name: "Bob the builder",
        status: "stopped",
        homeRole: "builder",
        hasRolePack: true,
        packRefusedSharedHome: true,
      },
      {
        pubkey: HEALTHY_AGENT.pubkey,
        name: "Keystone",
        status: "stopped",
        homeRole: "lead",
        hasRolePack: true,
        packRefusedSharedHome: false,
      },
    ],
  });
  await page.goto("/");
  await page.getByTestId("open-agents-view").click();

  // The card: the fact, without the remedy a thumbnail has no room for.
  const refusedBadge = page.getByTestId("agent-shared-home");
  await expect(refusedBadge).toHaveCount(1);
  await expect(refusedBadge).toContainText("Shared home — packs refused");
  // And exactly one: the agent in a nest of its own is accused of nothing.
  await expect(page.getByTestId("agent-no-role-pack")).toHaveCount(0);

  // The profile: the fact, the remedy, and the button that performs it.
  await page
    .getByRole("button", { name: "Bob the builder agent profile" })
    .click();
  const panel = page.getByTestId("user-profile-summary-scroll-layout");
  const action = panel.getByTestId("agent-give-own-nest");
  await expect(action).toBeVisible();
  await expect(panel.getByTestId("agent-shared-home")).toContainText(
    "give this agent its own nest, then restart it",
  );

  await action.click();

  // The disclosure clears because the host answered that the agent moved —
  // on the panel and on the card behind it, from the one refreshed record.
  await expect(page.getByTestId("agent-give-own-nest")).toHaveCount(0);
  await expect(page.getByTestId("agent-shared-home")).toHaveCount(0);
  // The role itself is untouched — moving an agent is not re-roling it.
  await expect(panel.getByTestId("agent-home-role")).toContainText(
    "Home role: Builder",
  );

  const commands = await page.evaluate(
    () =>
      (window.__BUZZ_E2E_COMMAND_LOG__ ?? [])
        .map((entry) => (entry as { command?: string }).command)
        .filter((command) => command === "give_agent_its_own_nest").length,
  );
  expect(commands).toBe(1);
});
