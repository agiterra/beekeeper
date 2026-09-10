import { expect, test } from "@playwright/test";

import { installMockBridge, TEST_IDENTITIES } from "../helpers/bridge";
import { seedActiveIdentity } from "../helpers/onboarding";

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
  // A real signing key: the click founds a genesis the page reads back
  // through the signature-verifying catalog; the bridge's placeholder `sig`
  // would leave the page reporting the session missing.
  await seedActiveIdentity(page, TEST_IDENTITIES.tyler);
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
  // The founded page reads this computer's remembered working directories;
  // the shared bridge does not answer that command, so answer it empty here.
  await page.addInitScript(() => {
    type Invoke = (
      cmd: string,
      args?: Record<string, unknown>,
      options?: unknown,
    ) => Promise<unknown>;
    let internals: Record<string, unknown> | undefined;
    Object.defineProperty(window, "__TAURI_INTERNALS__", {
      configurable: true,
      get: () => internals,
      set: (value: Record<string, unknown>) => {
        internals = value;
        let real: Invoke | undefined;
        const wrapped: Invoke = async (cmd, args, options) => {
          if (cmd === "get_coding_session_workdir_state") {
            return {
              version: 1,
              byProject: {},
              byChannel: {},
              mru: [],
              pending: {},
            };
          }
          if (!real) throw new Error("mock invoke is not installed yet");
          return real(cmd, args, options);
        };
        Object.defineProperty(value, "invoke", {
          configurable: true,
          get: () => (real ? wrapped : undefined),
          set: (fn: Invoke) => {
            real = fn;
          },
        });
      },
    });
  });
  // The e2e static server cannot serve SPA subroutes directly — enter the
  // page the way a person does, through a channel's sessions menu. The click
  // founds the session and lands on its page (no dialog since 2026-09-10);
  // the runtime picker is on the page's setup card.
  await page.goto("/");
  await page.getByTestId("channel-engineering").click();
  await page.getByTestId("channel-coding-sessions-trigger").click();
  await page.getByTestId("channel-coding-sessions-new").click();
  await expect(
    page.getByTestId("coding-session-founded-workspace-founded"),
  ).toBeVisible({ timeout: 20_000 });

  // Provider and model are one control now, so a signed-out runtime shows as
  // disabled rows inside the picker rather than a disabled `<option>`.
  const picker = page.getByTestId("coding-session-model-picker");
  await expect(picker).toBeVisible();
  await picker.click();
  await expect(
    page
      .getByTestId("coding-session-model-row-select")
      .filter({ hasText: "sign-in needed" })
      .first(),
  ).toBeVisible({ timeout: 15_000 });
  await page.keyboard.press("Escape");

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
  // runtimes command reports ready and the rows re-enable.
  await expect(connect).toHaveCount(0, { timeout: 20_000 });
  await picker.click();
  await expect(
    page
      .getByTestId("coding-session-model-row-select")
      .filter({ hasText: "sign-in needed" }),
  ).toHaveCount(0, { timeout: 20_000 });
  await page.keyboard.press("Escape");
});
