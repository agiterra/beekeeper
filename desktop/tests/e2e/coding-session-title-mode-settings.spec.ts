import { expect, test, type Page } from "@playwright/test";

import { waitForAnimations } from "../helpers/animations";
import { installMockBridge } from "../helpers/bridge";

/**
 * SV-56 / D9: Settings → Coding sessions → Session titles.
 *
 * The card shows the mode stored on this computer, and each mode says where
 * the first message goes for a title and on which machine. The endpoint
 * fields appear only in "Use my naming model". Choosing a mode applies it at
 * once (T3's settings do) and sends `titleMode` to the host — which is what
 * makes the host refuse the naming model and tell the agent host — so the
 * spec reads the call, not the pixels. A host that was not told stays on
 * screen until a write lands.
 *
 * One shot per mode, each scoped to the group; the three must hash apart.
 */

const SHOTS = "test-results/coding-session-title-mode";

type TitleMode = "agent" | "my-model" | "off";

const SEEDS: Record<
  TitleMode,
  {
    titleMode: TitleMode;
    provider: "off" | "anthropic" | "openai-compatible";
    baseUrl: string;
    model: string;
    hasApiKey: boolean;
  }
> = {
  agent: {
    titleMode: "agent",
    provider: "off",
    baseUrl: "",
    model: "",
    hasApiKey: false,
  },
  "my-model": {
    titleMode: "my-model",
    provider: "openai-compatible",
    baseUrl: "http://127.0.0.1:11434/v1",
    model: "llama3.2",
    hasApiKey: false,
  },
  off: {
    titleMode: "off",
    provider: "off",
    baseUrl: "",
    model: "",
    hasApiKey: false,
  },
};

async function openSessionTitles(
  page: Page,
  mode: TitleMode,
  hostModeMismatch: string | null = null,
) {
  await installMockBridge(page, {
    codingSessionNaming: { ...SEEDS[mode], hostModeMismatch },
  });
  await page.goto("/");
  await page.getByTestId("open-settings").click();
  await page.getByTestId("profile-popover-settings").click();
  await expect(page.getByTestId("settings-view")).toBeVisible();
  await page.getByTestId("settings-nav-sessions").click();
  const group = page.getByTestId("settings-coding-session-naming");
  await group.scrollIntoViewIfNeeded();
  await expect(group).toBeVisible();
  await expect(page.getByTestId("coding-session-naming-card")).toHaveAttribute(
    "data-title-mode",
    mode,
  );
  return group;
}

async function setCalls(page: Page) {
  return page.evaluate(
    () =>
      (
        window as typeof window & {
          __BEEKEEPER_E2E_CODING_SESSION_NAMING_SET_CALLS__?: Record<
            string,
            unknown
          >[];
        }
      ).__BEEKEEPER_E2E_CODING_SESSION_NAMING_SET_CALLS__ ?? [],
  );
}

test("agent mode: the default, said with the machine, the account and the env switch", async ({
  page,
}) => {
  const group = await openSessionTitles(page, "agent");
  await expect(
    page.getByTestId("coding-session-title-mode-agent").locator("input"),
  ).toBeChecked();
  const disclosure = page.getByTestId("coding-session-title-mode-disclosure");
  await expect(disclosure).toContainText(
    "The agent that runs the first turn titles the session on the computer it runs on",
  );
  await expect(disclosure).toContainText("nothing new leaves this computer");
  await expect(disclosure).toContainText(
    "a session on another computer follows that computer's setting",
  );
  await expect(disclosure).toContainText(
    "A host started with BUZZ_CSP_AUTO_TITLE=off, or a runtime configured with no title model, titles nothing.",
  );
  await expect(disclosure).toContainText(
    "A name someone types always wins, in every mode.",
  );
  await expect(
    page.getByTestId("coding-session-naming-model-fields"),
  ).toHaveCount(0);

  await waitForAnimations(page);
  await group.screenshot({ path: `${SHOTS}/sv56-title-mode-agent.png` });
});

test("my-model mode: the endpoint is named and its fields are shown", async ({
  page,
}) => {
  const group = await openSessionTitles(page, "my-model");
  await expect(
    page.getByTestId("coding-session-title-mode-my-model").locator("input"),
  ).toBeChecked();
  const disclosure = page.getByTestId("coding-session-title-mode-disclosure");
  await expect(disclosure).toContainText(
    "Your first message is sent to http://127.0.0.1:11434/v1",
  );
  await expect(disclosure).toContainText(
    "Your model suggests a name in the Name field while you write; nothing is titled after Start.",
  );
  await expect(page.getByTestId("coding-session-naming-base-url")).toHaveValue(
    "http://127.0.0.1:11434/v1",
  );
  await expect(page.getByTestId("coding-session-naming-model")).toHaveValue(
    "llama3.2",
  );
  await expect(page.getByTestId("coding-session-naming-test")).toBeVisible();

  await waitForAnimations(page);
  await group.screenshot({ path: `${SHOTS}/sv56-title-mode-my-model.png` });
});

test("off mode: nothing is sent for a title", async ({ page }) => {
  const group = await openSessionTitles(page, "off");
  await expect(
    page.getByTestId("coding-session-title-mode-off").locator("input"),
  ).toBeChecked();
  await expect(
    page.getByTestId("coding-session-title-mode-disclosure"),
  ).toContainText(
    "Nothing is sent anywhere for a title; sessions stay untitled until someone names them.",
  );
  await expect(
    page.getByTestId("coding-session-naming-model-fields"),
  ).toHaveCount(0);

  await waitForAnimations(page);
  await group.screenshot({ path: `${SHOTS}/sv56-title-mode-off.png` });
});

test("choosing a mode applies it at once, and Reset to default restores agent", async ({
  page,
}) => {
  await openSessionTitles(page, "agent");
  await expect(page.getByTestId("coding-session-title-mode-reset")).toHaveCount(
    0,
  );
  await expect(page.getByTestId("coding-session-naming-save")).toHaveCount(0);
  await page.getByTestId("coding-session-title-mode-off").click();
  await expect(page.getByTestId("coding-session-naming-card")).toHaveAttribute(
    "data-title-mode",
    "off",
  );
  await expect(
    page.getByTestId("coding-session-title-mode-unsaved"),
  ).toHaveCount(0);
  let calls = await setCalls(page);
  expect(calls).toHaveLength(1);
  expect(calls[0]).toMatchObject({ titleMode: "off", provider: "off" });
  // A mode other than "my model" leaves the stored endpoint alone.
  expect(calls[0]?.baseUrl ?? null).toBeNull();
  expect(calls[0]?.model ?? null).toBeNull();
  expect(calls[0]?.apiKey ?? null).toBeNull();

  await page.getByTestId("coding-session-title-mode-reset").click();
  await expect(page.getByTestId("coding-session-naming-card")).toHaveAttribute(
    "data-title-mode",
    "agent",
  );
  await expect(
    page.getByTestId("coding-session-title-mode-agent").locator("input"),
  ).toBeChecked();
  calls = await setCalls(page);
  expect(calls).toHaveLength(2);
  expect(calls[1]).toMatchObject({ titleMode: "agent", provider: "off" });
});

test("my naming model with no endpoint yet is unsaved until Save", async ({
  page,
}) => {
  await openSessionTitles(page, "agent");
  await page.getByTestId("coding-session-title-mode-my-model").click();
  await expect(
    page.getByTestId("coding-session-title-mode-unsaved"),
  ).toContainText("still on “Generate with the session's agent”");
  await expect(page.getByTestId("coding-session-naming-save")).toBeVisible();
  expect(await setCalls(page)).toEqual([]);
});

test("a host that was not told is said until a write lands", async ({
  page,
}) => {
  await openSessionTitles(
    page,
    "off",
    "this computer's agent host is not on off: /state/p1 is on agent",
  );
  const warning = page.getByTestId("coding-session-title-mode-host-mismatch");
  await expect(warning).toContainText(
    "“Off” is saved on this computer, but this computer's agent host is not on off",
  );
  await page.getByTestId("coding-session-title-mode-host-retry").click();
  await expect(warning).toHaveCount(0);
  const calls = await setCalls(page);
  expect(calls).toHaveLength(1);
  expect(calls[0]).toMatchObject({ titleMode: "off", provider: "off" });
});
