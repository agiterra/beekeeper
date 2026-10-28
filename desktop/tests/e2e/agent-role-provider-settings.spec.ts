import { expect, test } from "@playwright/test";

import { installMockBridge, TEST_IDENTITIES } from "../helpers/bridge";

/**
 * Ledger 397: an installed role agent's provider could not be changed
 * anywhere. Its Edit opens the role definition it shares (no provider), the
 * instance editor that has the Provider field was reachable only for agents
 * without a resolved definition, and a role reinstall renames in place
 * without repinning. The directory detail now opens the agent's own settings;
 * Edit still opens the definition.
 */

const AGENT = TEST_IDENTITIES.charlie;

test("a role agent's provider is reachable from the directory and saved on the same agent", async ({
  page,
}) => {
  await installMockBridge(page, {
    personas: [
      {
        id: "crew-role:builder",
        displayName: "Builder",
        systemPrompt: "Implement the assigned outcome.",
        runtime: null,
      },
    ],
    managedAgents: [
      {
        pubkey: AGENT.pubkey,
        name: "Role builder",
        status: "stopped",
        personaId: "crew-role:builder",
        homeRole: "builder",
        hasRolePack: true,
        runtime: "goose",
      },
    ],
  });
  await page.goto("/");
  await page.getByTestId("open-agents-view").click();
  const row = page.getByTestId("agent-row").filter({
    has: page.getByTestId("agent-row-name").getByText("Role builder", {
      exact: true,
    }),
  });
  await row.click();

  // Edit opens the shared role: instructions only, and it says where the
  // runtime and model live instead of offering them (ledger 401).
  await page.getByTestId("agent-detail-edit").click();
  await expect(
    page.getByTestId("role-definition-execution-note"),
  ).toBeVisible();
  await expect(page.locator("#edit-agent-runtime")).toHaveCount(0);
  await expect(
    page.locator('[data-testid$="-configuration-section"]'),
  ).toBeHidden();
  await page.keyboard.press("Escape");

  // The agent's own settings carry the provider.
  await page.getByTestId("agent-detail-settings").click();
  const provider = page.locator("#edit-agent-runtime");
  await expect(provider).toBeVisible();
  await provider.click();
  await page.getByText("Beekeeper Agent", { exact: true }).click();
  await page.getByRole("button", { name: "Save changes" }).click();
  await expect(page.locator("#edit-agent-runtime")).toHaveCount(0);

  const updates = await page.evaluate(
    () =>
      (window.__BEEKEEPER_E2E_COMMAND_LOG__ ?? []).filter(
        (entry: { command: string }) =>
          entry.command === "update_managed_agent",
      ) as Array<{
        command: string;
        payload: { input: Record<string, unknown> };
      }>,
  );
  expect(updates.length).toBeGreaterThan(0);
  const input = updates[updates.length - 1].payload.input;
  // The same agent, not a new one, pinned deliberately to the chosen
  // provider's command (the backend derives the runtime from it).
  expect(input.pubkey).toBe(AGENT.pubkey);
  expect(input.agentCommand).toBe("beekeeper-agent");
  expect(input.harnessOverride).toBe(true);
});

test("a role agent whose shared role diverged offers Re-apply role source on the same agent", async ({
  page,
}) => {
  await installMockBridge(page, {
    personas: [
      {
        id: "crew-role:builder",
        displayName: "Builder",
        systemPrompt: "Implement the assigned outcome.",
        runtime: null,
      },
    ],
    managedAgents: [
      {
        pubkey: AGENT.pubkey,
        name: "Role builder",
        status: "stopped",
        personaId: "crew-role:builder",
        homeRole: "builder",
        hasRolePack: true,
        runtime: "goose",
        personaSourceState: "diverged",
      },
    ],
  });
  await page.goto("/");
  await page.getByTestId("open-agents-view").click();
  await page
    .getByTestId("agent-row")
    .filter({
      has: page.getByTestId("agent-row-name").getByText("Role builder", {
        exact: true,
      }),
    })
    .click();

  const notice = page.getByTestId("agent-role-source");
  await expect(notice).toHaveAttribute("data-state", "diverged");
  await notice.getByTestId("agent-role-source-reapply").click();
  await expect(notice).toContainText("Role re-applied from its pack.");

  const calls = await page.evaluate(
    () =>
      (window.__BEEKEEPER_E2E_COMMAND_LOG__ ?? []).filter(
        (entry: { command: string }) => entry.command === "reapply_role_source",
      ) as Array<{ command: string; payload: Record<string, unknown> }>,
  );
  expect(calls).toHaveLength(1);
  expect(calls[0].payload.pubkey).toBe(AGENT.pubkey);
  expect(calls[0].payload.discardInstructionEdits).toBe(false);
});

test("an accessibility press opens the agent's Provider picker", async ({
  page,
}) => {
  await installMockBridge(page, {
    managedAgents: [
      {
        pubkey: AGENT.pubkey,
        name: "Role builder",
        status: "stopped",
        homeRole: "builder",
        hasRolePack: true,
        runtime: "goose",
      },
    ],
  });
  await page.goto("/");
  await page.getByTestId("open-agents-view").click();
  await page
    .getByTestId("agent-row")
    .filter({
      has: page.getByTestId("agent-row-name").getByText("Role builder", {
        exact: true,
      }),
    })
    .click();
  await page.getByTestId("agent-detail-settings").click();

  // VoiceOver (and an agent through the same AX action) delivers a click with
  // no pointer, which Radix alone ignores; the picker must still open.
  await page.locator("#edit-agent-runtime").dispatchEvent("click");
  await expect(
    page.getByRole("menuitemradio", { name: "Beekeeper Agent" }),
  ).toBeVisible();
});

test("a stopped agent's Start stays usable when its unchanged host status is read again", async ({
  page,
}) => {
  await installMockBridge(page, {
    personas: [
      {
        id: "crew-role:builder",
        displayName: "Builder",
        systemPrompt: "Implement the assigned outcome.",
        runtime: null,
      },
    ],
    managedAgents: [
      {
        pubkey: AGENT.pubkey,
        name: "Role builder",
        status: "stopped",
        personaId: "crew-role:builder",
        homeRole: "builder",
        hasRolePack: true,
        runtime: "codex",
      },
    ],
  });
  await page.goto("/");
  await page.getByTestId("open-agents-view").click();
  const row = page.getByTestId("agent-row").filter({
    has: page.getByTestId("agent-row-name").getByText("Role builder", {
      exact: true,
    }),
  });
  await row.click();
  await expect(page.getByTestId("agent-detail-start")).toBeEnabled();
  // Reopen: the cached status is cleared and re-read with an identical
  // answer. Start used to stay disabled under its "Start" label (ledger 403).
  await page.getByTestId("agent-detail-close").click();
  await row.click();
  await expect(page.getByTestId("agent-host-status")).toContainText(
    "Host work is stopped",
  );
  await expect(page.getByTestId("agent-detail-start")).toBeEnabled();
});
