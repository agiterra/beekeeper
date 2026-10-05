import { createHash } from "node:crypto";

import { expect, test, type Page } from "@playwright/test";
import { generateSecretKey, getPublicKey } from "nostr-tools/pure";

import type { CodingSessionNamingSettings } from "@/shared/api/tauriCodingSessionNaming";

import { waitForAnimations } from "../helpers/animations";
import { installMockBridge } from "../helpers/bridge";

/**
 * The founded page under each session-title mode (D9, SV-56).
 *
 * The mode is this computer's preference (Settings → Coding sessions). On
 * the founded page it decides two things only:
 *
 * - whether the person's naming model is consulted at all — the Name-field
 *   suggestion while they write, and the Solo goal's one-line summary after
 *   Start. Only "Use my naming model" consults it; the agent mode (the
 *   default) and Off send no draft text anywhere for a name;
 * - what the sentence under a blank Name says will happen: untitled unless
 *   the agent's computer names it (agent), untitled with the model only
 *   suggesting in the field (my-model), untitled with titles Off (off).
 *
 * In every mode a blank Name publishes nothing after Start; that is pinned
 * by `useCodingSessionFoundedStart.test.mjs` across all three modes. This
 * spec records every Tauri command the page issues and asserts that
 * `generate_coding_session_name` / `generate_coding_session_goal` never run
 * in the agent and Off modes, even after a long prompt is written and left.
 *
 * Shots: sv56-founded-agent, sv56-founded-my-model, sv56-founded-off — the
 * Name field and the blank-Name sentence beneath it. Their hashes must
 * differ; the last test checks it.
 */

const SHOTS = "test-results/sv56";

const PROVIDER_PUBKEY = getPublicKey(generateSecretKey());

/** The bridge's own identity, so the founded genesis is signed by the driver. */
const FOUNDER_IDENTITY = {
  privateKey:
    "3dbaebadb5dfd777ff25149ee230d907a15a9e1294b40b830661e65bb42f6c03",
  pubkey: "e5ebc6cdb579be112e336cc319b5989b4bb6af11786ea90dbe52b5f08d741b34",
  username: "tyler",
};
const CHANNEL_NAME = "engineering";

const SUGGESTED_NAME = "Founded setup move";
const LONG_PROMPT = [
  "Move the create dialog onto the founded page and keep Solo and Team as a switch at the top.",
  "Publish the name and the prompt when each field is left, and make Start flush both first.",
].join("\n");

type TitleMode = CodingSessionNamingSettings["titleMode"];

const SETTINGS: Record<TitleMode, CodingSessionNamingSettings> = {
  // An endpoint still configured from before a switch to the default:
  // configured is not consent, so it must still be asked for nothing.
  agent: {
    titleMode: "agent",
    provider: "anthropic",
    baseUrl: "",
    model: "claude-haiku-4-5",
    hasApiKey: true,
  },
  "my-model": {
    titleMode: "my-model",
    provider: "anthropic",
    baseUrl: "",
    model: "claude-haiku-4-5",
    hasApiKey: true,
  },
  off: {
    titleMode: "off",
    provider: "off",
    baseUrl: "",
    model: "",
    hasApiKey: false,
  },
};

const SENTENCE: Record<TitleMode, string> = {
  agent:
    "Left blank, it stays untitled unless the agent's computer names it from the first message.",
  "my-model":
    "Left blank, it stays untitled; your naming model only suggests a name in this field while you write.",
  off: "Left blank, it stays untitled: session titles are Off on this computer.",
};

const NAMING_COMMANDS = [
  "generate_coding_session_name",
  "generate_coding_session_goal",
];

/**
 * Record every command, and answer the two naming-model commands so a
 * my-model run shows a suggestion rather than a refusal. Recording happens
 * before the answer, so an unexpected call is caught either way.
 */
function titleModeInvokeInitScript(suggestedName: string) {
  type Invoke = (
    cmd: string,
    args?: Record<string, unknown>,
    options?: unknown,
  ) => Promise<unknown>;
  const recorded: string[] = [];
  (window as unknown as { __SV56_COMMANDS__: string[] }).__SV56_COMMANDS__ =
    recorded;
  let internals: Record<string, unknown> | undefined;
  Object.defineProperty(window, "__TAURI_INTERNALS__", {
    configurable: true,
    get: () => internals,
    set: (value: Record<string, unknown>) => {
      internals = value;
      let real: Invoke | undefined;
      const wrapped: Invoke = async (cmd, args, options) => {
        recorded.push(cmd);
        switch (cmd) {
          case "generate_coding_session_name":
            return suggestedName;
          case "generate_coding_session_goal":
            return "Move session setup onto the founded page.";
          case "get_coding_session_workdir_state":
            return {
              version: 1,
              byProject: {},
              byChannel: {},
              mru: [],
              pending: {},
            };
          default:
            break;
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
}

async function openApp(page: Page, mode: TitleMode) {
  await page.addInitScript((identity) => {
    window.localStorage.setItem(
      "buzz:e2e-identity-override.v1",
      JSON.stringify(identity),
    );
  }, FOUNDER_IDENTITY);
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
        driver: "claude-agent-acp",
        label: "Claude Code",
        authState: "ready",
        defaultModel: "sonnet",
        allowedModels: ["default", "sonnet", "haiku"],
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
    codingSessionNaming: SETTINGS[mode],
  });
  await page.addInitScript(titleModeInvokeInitScript, SUGGESTED_NAME);
  await page.goto("/", { waitUntil: "domcontentloaded" });
}

/** Click "New coding session" and wait until the page's readers settle. */
async function foundSession(page: Page) {
  await page.getByTestId(`channel-${CHANNEL_NAME}`).click();
  await page.getByTestId("channel-coding-sessions-trigger").click();
  await page.getByTestId("channel-coding-sessions-new").click();
  await expect(
    page.getByTestId("coding-session-founded-workspace-founded"),
  ).toBeVisible({ timeout: 20_000 });
  // The name and goal readers sit behind the send budget on a fresh launch
  // (see coding-session-founded-setup.spec.ts); the suggestion is suppressed
  // until names settle, so nothing below is meaningful before this.
  await expect(
    page.getByTestId("new-coding-session-blocker-goal-unresolved"),
  ).toHaveCount(0, { timeout: 30_000 });
}

async function namingCommands(page: Page): Promise<string[]> {
  const recorded = await page.evaluate(
    () =>
      (window as unknown as { __SV56_COMMANDS__?: string[] })
        .__SV56_COMMANDS__ ?? [],
  );
  return recorded.filter((command) => NAMING_COMMANDS.includes(command));
}

/** Screenshot the Name field and the sentence beneath it. */
async function shootNameRegion(page: Page, name: string): Promise<string> {
  const region = page
    .getByTestId("coding-session-founded-name")
    .locator("xpath=..");
  await waitForAnimations(page);
  const png = await region.screenshot({ path: `${SHOTS}/${name}.png` });
  return createHash("sha256").update(png).digest("hex");
}

const hashes = new Map<string, string>();

test.describe("session-title mode on the founded page (SV-56, D9)", () => {
  test.describe.configure({ mode: "serial" });
  test.use({ viewport: { width: 1280, height: 1600 } });

  for (const mode of ["agent", "my-model", "off"] as const) {
    test(`sv56-founded-${mode}: the blank-Name sentence, and whether a naming model is asked`, async ({
      page,
    }) => {
      test.setTimeout(90_000);
      await openApp(page, mode);
      await foundSession(page);

      const name = page.getByTestId("coding-session-founded-name");
      await expect(name).toHaveValue("");
      const sentence = page.getByTestId("coding-session-founded-name-auto");
      await expect(sentence).toHaveText(SENTENCE[mode]);
      hashes.set(mode, await shootNameRegion(page, `sv56-founded-${mode}`));

      // Write a prompt worth naming and leave the field — the moment the
      // namer is asked when it is consulted at all.
      const prompt = page.getByTestId("coding-session-founded-prompt");
      await prompt.fill(LONG_PROMPT);
      await prompt.blur();

      if (mode === "my-model") {
        await expect(name).toHaveValue(SUGGESTED_NAME, { timeout: 15_000 });
        await expect(
          page.getByTestId("coding-session-founded-goal-auto"),
        ).toBeVisible();
        expect(await namingCommands(page)).toContain(
          "generate_coding_session_name",
        );
        return;
      }

      // Past one full suggestion cadence (5 s) after the blur: had the
      // namer been armed, its tick or the blur would have asked by now.
      await page.waitForTimeout(6_500);
      expect(await namingCommands(page)).toEqual([]);
      await expect(name).toHaveValue("");
      await expect(sentence).toHaveText(SENTENCE[mode]);
      await expect(
        page.getByTestId("coding-session-founded-name-suggestion"),
      ).toHaveCount(0);
      await expect(
        page.getByTestId("coding-session-founded-goal-auto"),
      ).toHaveCount(0);
    });
  }

  test("the three mode shots are three different pictures", () => {
    expect([...hashes.keys()].sort()).toEqual(["agent", "my-model", "off"]);
    expect(new Set(hashes.values()).size).toBe(3);
  });
});
