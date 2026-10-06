/**
 * Contract test for the mock Tauri bridge's `get_global_agent_config`.
 *
 * The mock must answer with the same shape the real Rust command does.
 * `allowed-bridge-pubkeys` is declared `#[serde(default, rename = ...)]` on a
 * `Vec` (desktop/src-tauri/src/managed_agents/global_config/mod.rs:103), so the
 * real command always emits the key, and `GlobalAgentConfig`
 * (src/shared/api/types.ts:964) declares it required.
 *
 * When the mock omitted it, `CodingSessionTrustFields` called `.map` on
 * `undefined` as soon as Settings › Agents mounted, the app-level error boundary
 * replaced the whole window with "Something went wrong!", and ~60 smoke tests
 * failed for a bug the shipping product does not have. This test fails loudly
 * at the harness instead of scattering the damage across the suite.
 */

import { expect, test } from "@playwright/test";

import { installMockBridge } from "../helpers/bridge";

type MockGlobalAgentConfig = {
  "allowed-bridge-pubkeys"?: unknown;
};

async function readMockGlobalAgentConfig(
  page: import("@playwright/test").Page,
) {
  await page.goto("/", { waitUntil: "domcontentloaded" });
  // The bridge registers this hook when its module evaluates, which is after
  // DOMContentLoaded — reading straight through the optional call would return
  // `undefined` and make this contract test pass or fail on timing instead of
  // on the shape it is about.
  await page.waitForFunction(
    () =>
      typeof (
        window as unknown as {
          __BEEKEEPER_E2E_INVOKE_MOCK_COMMAND__?: unknown;
        }
      ).__BEEKEEPER_E2E_INVOKE_MOCK_COMMAND__ === "function",
  );
  return page.evaluate(() =>
    (
      window as unknown as {
        __BEEKEEPER_E2E_INVOKE_MOCK_COMMAND__?: (
          command: string,
          payload: unknown,
        ) => Promise<MockGlobalAgentConfig>;
      }
    ).__BEEKEEPER_E2E_INVOKE_MOCK_COMMAND__?.("get_global_agent_config", null),
  );
}

test("mock global agent config carries allowed-bridge-pubkeys with no seed", async ({
  page,
}) => {
  await installMockBridge(page);
  const config = await readMockGlobalAgentConfig(page);
  expect(Array.isArray(config?.["allowed-bridge-pubkeys"])).toBe(true);
});

test("mock global agent config carries allowed-bridge-pubkeys when a spec seeds one", async ({
  page,
}) => {
  await installMockBridge(page, {
    globalAgentConfig: {
      env_vars: {},
      model: null,
      preferred_runtime: "buzz-agent",
      provider: "anthropic",
    },
  });
  const config = await readMockGlobalAgentConfig(page);
  expect(Array.isArray(config?.["allowed-bridge-pubkeys"])).toBe(true);
});

test("settings agents panel mounts without the app error boundary", async ({
  page,
}) => {
  await installMockBridge(page);
  await page.goto("/", { waitUntil: "domcontentloaded" });
  await page.getByTestId("open-settings").click();
  await page.getByTestId("profile-popover-settings").click();
  await expect(page.getByTestId("settings-view")).toBeVisible();
  await page.getByTestId("settings-nav-agents").click();
  await expect(page.getByTestId("settings-global-agent-config")).toBeVisible({
    timeout: 10_000,
  });
  await expect(page.getByText("Something went wrong!")).toHaveCount(0);
});
