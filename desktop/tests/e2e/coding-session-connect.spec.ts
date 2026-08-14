import { expect, test } from "@playwright/test";

import { installMockBridge } from "../helpers/bridge";

/**
 * The guided runtime-login flow on the New Coding Session screen: a
 * signed-out runtime renders disabled with a Connect button, Connect
 * launches the runtime's own login through the mocked ACP auth plumbing,
 * and the post-login re-probe flips the runtime to ready without a reload.
 *
 * The bridge's provider-runtimes mock switches to the after-connect table
 * once `connect_acp_runtime` has run, so the flip exercises the real login
 * watch (poll + refetch), not a hand-delivered state update.
 */

const PROVIDER_PUBKEY = "f".repeat(64);

function claudeRuntime(authState: "ready" | "needs_auth") {
  return {
    instanceRef: "claude-primary",
    runtime: "claude",
    driver: "claude-agent-acp",
    label: "Claude Code",
    authState,
    defaultModel: "default",
    allowedModels: [],
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

test("a signed-out runtime offers Connect and flips ready after the login", async ({
  page,
}) => {
  await installMockBridge(page, {
    codingSessionProviderStatus: {
      provisioned: true,
      running: true,
      providerPubkey: PROVIDER_PUBKEY,
      instanceId: "0123456789abcdef",
    },
    codingSessionProviderRuntimes: [claudeRuntime("needs_auth")],
    codingSessionProviderRuntimesAfterConnect: [claudeRuntime("ready")],
    acpAuthMethods: {
      claude: {
        methods: [
          {
            id: "claude-login",
            name: "Log in with Claude",
            type: "terminal",
          },
          // An API-key method must never surface a Connect button — CLI
          // login is the only credential path for coding sessions.
          {
            id: "api-key",
            name: "API key",
            type: "api-key",
          },
        ],
      },
    },
    connectAcpRuntimeResult: { launched: true },
  });
  await page.goto("/coding-sessions/new");

  const provider = page.getByTestId("new-coding-session-provider");
  await expect(provider).toBeVisible();
  await expect(
    provider.locator("option", { hasText: "(sign-in needed)" }),
  ).toHaveCount(1, { timeout: 15_000 });

  const connect = page.getByTestId(
    "coding-session-runtime-connect-claude-claude-login",
  );
  await expect(connect).toBeVisible();
  await expect(
    page.getByTestId("coding-session-runtime-connect-claude-api-key"),
  ).toHaveCount(0);

  await connect.click();
  await expect(
    page.getByTestId("coding-session-runtime-connect-claude-guidance"),
  ).toBeVisible();

  // The login watch re-probes on an interval; after the mocked connect the
  // runtimes command reports ready and the picker option re-enables.
  await expect(
    provider.locator("option", { hasText: "(sign-in needed)" }),
  ).toHaveCount(0, { timeout: 20_000 });
  await expect(connect).toHaveCount(0);
});
