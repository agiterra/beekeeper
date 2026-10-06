import { expect, test, type Page } from "@playwright/test";

import { installMockBridge, TEST_IDENTITIES } from "../helpers/bridge";

const AGENT_PUBKEY = TEST_IDENTITIES.charlie.pubkey;
const RESTORED_UNSCOPED_AGENT_PUBKEY = TEST_IDENTITIES.outsider.pubkey;
const CHANNEL_AGENTS = "94a444a4-c0a3-5966-ab05-530c6ddc2301";
const CHANNEL_GENERAL = "9a1657ac-f7aa-5db0-b632-d8bbeb6dfb50";
const CHANNEL_FOREIGN = "1c7e1c02-87bb-5e88-b2da-5a7a9432d0c9";

type ControlRequest = {
  agentPubkey: string;
  payload: {
    type: "cancel_turn";
    channelId?: string;
    requestId?: string;
  };
};

async function waitForActiveTurnSeed(page: Page) {
  await page.waitForFunction(
    () => typeof window.__BEEKEEPER_E2E_SEED_ACTIVE_TURNS__ === "function",
    null,
    { timeout: 10_000 },
  );
}

async function seedActiveTurn(page: Page, channelId: string) {
  await page.evaluate(
    ({ agentPubkey, channelId }) => {
      return window.__BEEKEEPER_E2E_SEED_ACTIVE_TURNS__?.({
        agentPubkey,
        channelId,
        turnId: `e2e-stop-${channelId}`,
      });
    },
    { agentPubkey: AGENT_PUBKEY, channelId },
  );
}

async function openAgentActivity(
  page: Page,
  activityChannelId: string,
  seedChannels: string[] = [activityChannelId],
): Promise<ReturnType<Page["getByTestId"]>> {
  await page.goto("/", { waitUntil: "domcontentloaded" });
  await page.getByTestId("channel-agents").click();
  await expect(page.getByTestId("chat-title")).toHaveText("agents");
  await waitForActiveTurnSeed(page);
  for (const channelId of seedChannels) {
    await seedActiveTurn(page, channelId);
  }

  const messageRow = page
    .getByTestId("message-row")
    .filter({ hasText: "Indexing the channel catalog now." });
  await expect(messageRow.first()).toBeVisible({ timeout: 8_000 });
  await messageRow.first().getByRole("button").first().click();

  const profile = page.getByTestId("user-profile-panel");
  await expect(profile).toBeVisible({ timeout: 10_000 });
  if (activityChannelId !== CHANNEL_AGENTS) {
    const dot = page.getByRole("tab", {
      name: "Show #general activity",
    });
    await expect(dot).toBeVisible({ timeout: 5_000 });
    await dot.click();
  }
  const activity = page.getByRole("button", {
    name: /Open full activity\./,
  });
  await expect(activity).toBeVisible({ timeout: 5_000 });
  await activity.click();

  const panel = page.getByTestId("agent-session-thread-panel");
  await expect(panel).toBeVisible({ timeout: 10_000 });
  await expect(
    page.getByTestId("agent-session-settings-menu-trigger"),
  ).toBeVisible();
  return panel;
}

async function readControlRequests(page: Page): Promise<ControlRequest[]> {
  return page.evaluate(
    () =>
      (window.__BEEKEEPER_E2E_OBSERVER_CONTROLS__ ?? []) as ControlRequest[],
  );
}

async function clickStop(page: Page) {
  await page.getByTestId("agent-session-settings-menu-trigger").click();
  const stop = page.getByTestId("agent-session-stop-turn");
  await expect(stop).toBeVisible();
  await expect(stop).toBeEnabled();
  // Wait for the real menu item to become stable before activating it.
  // Programmatic focus during menu opening can be replaced by its autofocus.
  await stop.click();
}

test.describe("agent control browser regressions", () => {
  test.use({ viewport: { width: 1280, height: 720 } });

  test("Stop uses the channelId-only activity scope and carries a requestId", async ({
    page,
  }) => {
    await installMockBridge(page, {
      managedAgents: [
        {
          name: "Charlie",
          personaId: "control-persona",
          pubkey: AGENT_PUBKEY,
          status: "running",
          channelNames: ["agents"],
        },
      ],
      observerControlResults: [{ type: "cancel_turn", status: "sent" }],
    });

    // The visible route normalizes a requested #general activity scope back
    // to the active #agents channel. This covers route normalization and
    // effective active-channel targeting on the visible activity panel.
    const panel = await openAgentActivity(page, CHANNEL_GENERAL, [
      CHANNEL_AGENTS,
      CHANNEL_GENERAL,
    ]);
    // ChannelPane intentionally resolves a requested scope that differs from
    // the visible route back to the active channel. Stop must target that
    // effective channel scope, never the stale profile selection.
    await expect(page.getByTestId("agent-session-scope-label")).toHaveText(
      "Activity · #agents",
    );

    await clickStop(page);
    await expect
      .poll(() => readControlRequests(page))
      .toEqual(
        expect.arrayContaining([
          expect.objectContaining({
            agentPubkey: AGENT_PUBKEY,
            payload: expect.objectContaining({
              type: "cancel_turn",
              channelId: CHANNEL_AGENTS,
              requestId: expect.any(String),
            }),
          }),
        ]),
      );
    const request = (await readControlRequests(page)).find(
      (entry) => entry.payload.type === "cancel_turn",
    );
    expect(request?.payload.requestId).toBeTruthy();
    await expect(page.getByText(/Stop signal sent to Charlie/)).toBeVisible();
    await expect(panel).toBeVisible();
  });

  test("Stop reports ambiguous_target without claiming success", async ({
    page,
  }) => {
    await installMockBridge(page, {
      managedAgents: [
        {
          name: "Charlie",
          personaId: "control-persona",
          pubkey: AGENT_PUBKEY,
          status: "running",
          channelNames: ["agents"],
        },
      ],
      observerControlResults: [
        { type: "cancel_turn", status: "ambiguous_target" },
      ],
    });
    await openAgentActivity(page, CHANNEL_AGENTS);

    await clickStop(page);
    await expect(page.getByText(/multiple agent sessions/)).toBeVisible();
    await expect(page.getByText(/Stop signal sent to Charlie/)).toHaveCount(0);
  });

  test("Stop does not accept an unconfirmed or foreign-channel result", async ({
    page,
  }) => {
    await installMockBridge(page, {
      managedAgents: [
        {
          name: "Charlie",
          personaId: "control-persona",
          pubkey: AGENT_PUBKEY,
          status: "running",
          channelNames: ["agents"],
        },
      ],
      observerControlResults: [
        {
          type: "cancel_turn",
          status: "sent",
          channelId: CHANNEL_FOREIGN,
        },
      ],
    });
    await page.clock.install({ time: new Date("2026-08-30T17:00:00.000Z") });
    await openAgentActivity(page, CHANNEL_AGENTS);

    await clickStop(page);
    await expect
      .poll(() => readControlRequests(page))
      .toEqual(
        expect.arrayContaining([
          expect.objectContaining({
            payload: expect.objectContaining({
              channelId: CHANNEL_AGENTS,
              requestId: expect.any(String),
            }),
          }),
        ]),
      );

    // The configured result is emitted with CHANNEL_FOREIGN. The correlator
    // must ignore it, then report the bounded timeout honestly.
    await page.clock.fastForward(8_001);
    await expect(page.getByText(/hasn't confirmed it/)).toBeVisible();
    await expect(page.getByText(/Stop signal sent to Charlie/)).toHaveCount(0);
  });

  // Excluded upstream model-switch cases: Beekeeper has not imported the
  // ModelPicker control that gives users this action. Their upstream specs
  // invoked a test-only helper directly, so keeping them would claim a
  // browser workflow this fork does not expose. The channel-scoped Stop
  // cases above remain because they use the visible activity panel.

  test("Stop is disabled for an unscoped restored activity URL", async ({
    page,
  }) => {
    await installMockBridge(page, {
      managedAgents: [
        {
          name: "Outsider",
          pubkey: RESTORED_UNSCOPED_AGENT_PUBKEY,
          status: "running",
          channelNames: ["random"],
        },
      ],
    });
    await page.goto(
      `/#/channels/${CHANNEL_AGENTS}?agentSession=${RESTORED_UNSCOPED_AGENT_PUBKEY}`,
      { waitUntil: "domcontentloaded" },
    );
    await waitForActiveTurnSeed(page);
    // Seed real work for an agent that is not in the visible channel activity
    // list. The restored URL therefore mounts an unscoped panel (no
    // agentSessionChannel and no matching activity-list channel), so the
    // disabled state proves the missing scope guard rather than inactivity.
    await seedActiveTurn(page, CHANNEL_AGENTS);
    const panel = page.getByTestId("agent-session-thread-panel");
    await expect(panel).toBeVisible({ timeout: 10_000 });
    await expect(page.getByTestId("agent-session-scope-label")).toHaveText(
      "Activity · All channels",
    );
    await page.getByTestId("agent-session-settings-menu-trigger").click();
    await expect(page.getByTestId("agent-session-stop-turn")).toBeDisabled();
    await expect(page.getByTestId("agent-session-stop-turn")).toHaveAttribute(
      "aria-disabled",
      "true",
    );
  });
});
