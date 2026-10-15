import { expect, test } from "@playwright/test";

import { waitForAnimations } from "../helpers/animations";
import { installMockBridge, TEST_IDENTITIES } from "../helpers/bridge";

/**
 * Round 2 Lane I: the four host-managed availability words a member pill can
 * show (NIP-CSL § Provider presence), one agent per state in `#agents`, each
 * captured as its own pill so the shots are distinct by construction.
 *
 * - Here: a signed execution lease (`runtimeAvailable: true`).
 * - Idle: no lease, but the provider's heartbeat is live and names the
 *   pair's execution.
 * - Not running: the heartbeat is live and does *not* name it.
 * - Provider not heard from: the heartbeat expired (not a reachability test).
 */
const SHOTS = "test-results/screenshots/round2";

const RELAY = (
  process.env.BEEKEEPER_E2E_RELAY_URL ?? "http://localhost:3000"
).replace(/^http/, "ws");

const STATES = [
  {
    slug: "here",
    word: "Here",
    identity: TEST_IDENTITIES.alice,
    runtime: { runtimeAvailable: true },
  },
  {
    slug: "idle",
    word: "Idle",
    identity: TEST_IDENTITIES.bob,
    runtime: {
      runtimeAvailable: null,
      providerPresence: "live" as const,
      execution: "idle" as const,
    },
  },
  {
    slug: "not-running",
    word: "Not running",
    identity: TEST_IDENTITIES.charlie,
    runtime: {
      runtimeAvailable: null,
      providerPresence: "live" as const,
      execution: "absent" as const,
    },
  },
  {
    slug: "provider-not-heard-from",
    word: "Provider not heard from",
    identity: TEST_IDENTITIES.outsider,
    runtime: { runtimeAvailable: null, providerPresence: "expired" as const },
  },
];

test("one member pill per host-managed availability word", async ({ page }) => {
  await installMockBridge(page, {
    managedAgents: STATES.map((state) => ({
      pubkey: state.identity.pubkey,
      name: `agent-${state.slug}`,
      status: "running",
      channelNames: ["agents"],
    })),
    managedAgentRuntimes: STATES.map((state) => ({
      pubkey: state.identity.pubkey,
      relayUrl: RELAY,
      lifecycle: "host_managed",
      ...state.runtime,
    })),
  });
  await page.goto("/");
  await page.getByTestId("channel-agents").click();
  await expect(page.getByTestId("chat-title")).toHaveText("agents");
  await page.getByTestId("channel-members-trigger").click();
  await expect(page.getByTestId("members-sidebar")).toBeVisible();

  for (const state of STATES) {
    const pill = page.getByTestId(
      `sidebar-managed-agent-status-${state.identity.pubkey}`,
    );
    await expect(pill).toHaveText(state.word);
    await waitForAnimations(page);
    const card = pill.locator("xpath=ancestor::*[@data-testid][1]");
    await card.screenshot({ path: `${SHOTS}/lane-i-${state.slug}.png` });
  }
});
