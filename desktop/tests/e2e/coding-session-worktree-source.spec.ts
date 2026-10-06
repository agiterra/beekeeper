import {
  finalizeEvent,
  generateSecretKey,
  getPublicKey,
} from "nostr-tools/pure";
import { seedActiveIdentity } from "../helpers/onboarding";
import {
  CODING_SESSION_LIFECYCLE_RECEIPT_SCHEMA,
  CODING_SESSION_LIFECYCLE_RECEIPT_TAG_VERSION,
  lifecycleReceiptSemanticKey,
} from "../../src/features/coding-sessions/lib/codingSessionIngressPayloads";
import { KIND_CODING_SESSION_LIFECYCLE_RECEIPT } from "../../src/shared/constants/kinds";
import { expect, test } from "@playwright/test";

import { installMockBridge, TEST_IDENTITIES } from "../helpers/bridge";

/**
 * The worktree source picker, proven against the bug it exists for.
 *
 * Sessions used to branch from whatever the checkout had checked out, so a
 * checkout parked on an old topic branch silently became the ancestor of
 * every "new" session made from it. The picker must therefore sit on the
 * trunk by default — not the checkout's HEAD — while still offering every
 * existing branch.
 *
 * Driven on the founded page since 2026-09-10: "New coding session" founds
 * the topic on the click and the Where field — worktree and working
 * directory — is edited there, before Start.
 */

const PROVIDER_SECRET = generateSecretKey();
const PROVIDER_PUBKEY = getPublicKey(PROVIDER_SECRET);
const CHANNEL_ID = "1c7e1c02-87bb-5e88-b2da-5a7a9432d0c9";

/**
 * The host's working-directory store, answered in front of the bridge.
 *
 * Every read answers an empty store and every write is a no-op that still
 * returns one, so nothing this spec does is remembered; the staged create
 * hints are recorded on `window.__RECOVERY_HINTS__` so the repair test can
 * read what the execution seam was told, under which `commandId`.
 */
function workdirStoreInitScript() {
  type Invoke = (
    cmd: string,
    args?: Record<string, unknown>,
    options?: unknown,
  ) => Promise<unknown>;
  const hints: unknown[] = [];
  (window as unknown as { __RECOVERY_HINTS__: unknown[] }).__RECOVERY_HINTS__ =
    hints;
  const emptyState = () => ({
    version: 1,
    byProject: {},
    byChannel: {},
    mru: [],
    pending: {},
  });
  let internals: Record<string, unknown> | undefined;
  Object.defineProperty(window, "__TAURI_INTERNALS__", {
    configurable: true,
    get: () => internals,
    set: (value: Record<string, unknown>) => {
      internals = value;
      let real: Invoke | undefined;
      const wrapped: Invoke = async (cmd, args, options) => {
        if (cmd === "stage_coding_session_create_hint") hints.push(args);
        if (
          [
            "stage_coding_session_create_hint",
            "clear_coding_session_create_hint",
            "record_coding_session_workdir_use",
            "get_coding_session_workdir_state",
          ].includes(cmd)
        ) {
          return emptyState();
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

async function foundSession(page: import("@playwright/test").Page) {
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
  await page.addInitScript(workdirStoreInitScript);
  await page.goto("/");
  await page.getByTestId("channel-engineering").click();
  await page.getByTestId("channel-coding-sessions-trigger").click();
  await page.getByTestId("channel-coding-sessions-new").click();
  await expect(
    page.getByTestId("coding-session-founded-workspace-founded"),
  ).toBeVisible({ timeout: 20_000 });
  await expect(page.getByTestId("coding-session-founded-where")).toBeVisible();
  // The goal reader is held behind the client's send budget on a fresh
  // launch (`relaySendBudget.ts`, 25 sends per 5 s) and settles ~10 s in;
  // a prompt left before then publishes nothing, by design.
  await expect(
    page.getByTestId("new-coding-session-blocker-goal-unresolved"),
  ).toHaveCount(0, { timeout: 30_000 });
}

test("the source picker defaults to the trunk, not the parked checkout", async ({
  page,
}) => {
  await foundSession(page);

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

test("a refused directory is repaired with its prompt retained and a fresh signed request", async ({
  page,
}) => {
  test.setTimeout(90_000);
  await foundSession(page);
  const goal =
    "Inspect the Windows checkout and report its structure. Do not modify files.";
  // The prompt publishes when the field is left; from then on it is on the
  // wire under the founder's key, which is why a repaired Start still has it.
  const prompt = page.getByTestId("coding-session-founded-prompt");
  await prompt.fill(goal);
  await prompt.blur();
  await expect
    .poll(
      async () =>
        (
          await page.evaluate(
            () => window.__BEEKEEPER_E2E_SIGNED_EVENTS__ ?? [],
          )
        ).filter((event) => event.kind === 44227).length,
    )
    .toBe(1);
  await page.getByTestId("coding-session-worktree-toggle").click();
  await page
    .getByTestId("coding-session-workdir-input")
    .fill("C:\\missing\\beekeeper");
  await page.getByTestId("coding-session-founded-start").click();

  const creates = () =>
    page.evaluate(async (channelId) => {
      const query = window.__BEEKEEPER_E2E_INVOKE_MOCK_COMMAND__;
      if (!query) throw new Error("mock query missing");
      const answer = await query("query_relay_filters", {
        filters: [{ kinds: [44221], "#h": [channelId], limit: 100 }],
      });
      const events = Array.isArray(answer)
        ? answer
        : (answer as { events: unknown[] }).events;
      return events as Array<{ id: string; content: string; tags: string[][] }>;
    }, CHANNEL_ID);
  await expect.poll(async () => (await creates()).length).toBe(1);
  const first = (await creates())[0];
  const original = JSON.parse(first.content);
  const channelId = first.tags.find((tag) => tag[0] === "h")?.[1];
  if (!channelId) throw new Error("create channel missing");
  const answer = async (commandId: string, failed: boolean) => {
    const receipt = finalizeEvent(
      {
        kind: KIND_CODING_SESSION_LIFECYCLE_RECEIPT,
        created_at: Math.floor(Date.now() / 1000),
        tags: [
          ["h", channelId],
          ["cslr-v", CODING_SESSION_LIFECYCLE_RECEIPT_TAG_VERSION],
          ["csl-command", commandId],
          ["csl-key", lifecycleReceiptSemanticKey(commandId)],
        ],
        content: JSON.stringify({
          schema: CODING_SESSION_LIFECYCLE_RECEIPT_SCHEMA,
          commandId,
          status: failed ? "failed" : "created",
          session: failed
            ? null
            : {
                driver: "claude-acp",
                instanceId: "0123456789abcdef",
                sessionId: "5eb303b6-2ff7-4514-80b3-8461886656e3",
                generation: 1,
              },
          error: failed
            ? {
                code: "PROJECT_CWD_UNRESOLVED",
                message: "No working directory is configured.",
              }
            : null,
        }),
      },
      PROVIDER_SECRET,
    );
    await page.evaluate((event) => {
      const seed = window.__BEEKEEPER_E2E_SEED_MOCK_SIGNED_EVENT__;
      if (!seed) throw new Error("signed-event seed missing");
      seed({ channelName: "engineering", event });
    }, receipt);
  };
  await answer(original.commandId, true);
  // A failed create holds the page's busy blocker; the one way out is named
  // as what it is, and a working-directory failure leaves the directory
  // field editable so the repair can be typed before the attempt is dropped.
  const discardAttempt = page.getByTestId(
    "coding-session-founded-discard-attempt",
  );
  await expect(discardAttempt).toBeVisible({ timeout: 15_000 });
  await expect(page.getByTestId("coding-session-workdir-input")).toBeEnabled();
  await page
    .getByTestId("coding-session-workdir-input")
    .fill("C:\\work\\beekeeper");
  await discardAttempt.click();
  await expect(page.getByTestId("coding-session-founded-start")).toBeEnabled();
  // The prompt is retained: it is the 44227 on the wire, not the attempt's.
  await expect(page.getByTestId("coding-session-founded-prompt")).toHaveValue(
    goal,
  );
  await expect(page.getByTestId("new-coding-session-blocker-goal")).toHaveCount(
    0,
  );
  await expect(page.getByTestId("new-coding-session-blocker-busy")).toHaveCount(
    0,
  );
  await expect(page.getByTestId("coding-session-workdir-input")).toHaveValue(
    "C:\\work\\beekeeper",
  );
  await page.getByTestId("coding-session-founded-start").click();
  await expect.poll(async () => (await creates()).length).toBe(2);
  const second = (await creates()).find((event) => event.id !== first.id);
  if (!second) throw new Error("replacement create missing");
  const repaired = JSON.parse(second.content);
  expect(repaired.commandId).not.toBe(original.commandId);
  expect(repaired.action.initialTurn).toBe(goal);
  expect(repaired.action.title).toBe(original.action.title);
  expect(repaired.action.model).toBe(original.action.model);
  expect(repaired.action.providerInstanceRef).toBe(
    original.action.providerInstanceRef,
  );
  expect(repaired.action.sessionRef).toBe(original.action.sessionRef);
  expect(repaired.action.genesisRef).toBe(original.action.genesisRef);
  const hints = await page.evaluate(
    () =>
      (
        window as unknown as {
          __RECOVERY_HINTS__: Array<{ commandId: string; path: string }>;
        }
      ).__RECOVERY_HINTS__,
  );
  expect(hints).toEqual([
    {
      commandId: original.commandId,
      path: "C:\\missing\\beekeeper",
      projectRef: null,
      rememberPath: "C:\\missing\\beekeeper",
    },
    {
      commandId: repaired.commandId,
      path: "C:\\work\\beekeeper",
      projectRef: null,
      rememberPath: "C:\\work\\beekeeper",
    },
  ]);
  // The prompt was published once, by the field — never by either Start.
  expect(
    (
      await page.evaluate(() => window.__BEEKEEPER_E2E_SIGNED_EVENTS__ ?? [])
    ).filter((event) => event.kind === 44227),
  ).toHaveLength(1);
  await answer(repaired.commandId, false);
  const receipts = await page.evaluate(
    async ({ kind, channelId }) => {
      const answer = await window.__BEEKEEPER_E2E_INVOKE_MOCK_COMMAND__?.(
        "query_relay_filters",
        {
          filters: [{ kinds: [kind], "#h": [channelId], limit: 100 }],
        },
      );
      return Array.isArray(answer)
        ? answer
        : (answer as { events: Array<{ content: string }> }).events;
    },
    { kind: KIND_CODING_SESSION_LIFECYCLE_RECEIPT, channelId: CHANNEL_ID },
  );
  expect(
    receipts.filter(
      (event: { content: string }) =>
        JSON.parse(event.content).status === "created",
    ),
  ).toHaveLength(1);
});
