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
 */

const PROVIDER_SECRET = generateSecretKey();
const PROVIDER_PUBKEY = getPublicKey(PROVIDER_SECRET);

async function openCreateDialog(page: import("@playwright/test").Page) {
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
  await page.goto("/");
  await page.getByTestId("channel-engineering").click();
  await page.getByTestId("channel-coding-sessions-trigger").click();
  await page.getByTestId("channel-coding-sessions-new").click();
  await expect(page.getByTestId("new-coding-session-form")).toBeVisible();
}

test("the source picker defaults to the trunk, not the parked checkout", async ({
  page,
}) => {
  await openCreateDialog(page);

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

test("a refused directory is repaired with its goal retained and a fresh signed request", async ({
  page,
}) => {
  await openCreateDialog(page);
  await page.evaluate(() => {
    const host = window as unknown as {
      __TAURI_INTERNALS__: {
        invoke: (command: string, args?: unknown) => Promise<unknown>;
      };
      __RECOVERY_HINTS__: Array<unknown>;
    };
    host.__RECOVERY_HINTS__ = [];
    const original = host.__TAURI_INTERNALS__.invoke.bind(
      host.__TAURI_INTERNALS__,
    );
    host.__TAURI_INTERNALS__.invoke = async (command, args) => {
      if (command === "stage_coding_session_create_hint")
        host.__RECOVERY_HINTS__.push(args);
      if (
        [
          "stage_coding_session_create_hint",
          "clear_coding_session_create_hint",
          "record_coding_session_workdir_use",
          "get_coding_session_workdir_state",
        ].includes(command)
      ) {
        return {
          version: 1,
          byProject: {},
          byChannel: {},
          mru: [],
          pending: {},
        };
      }
      return original(command, args);
    };
  });
  const goal =
    "Inspect the Windows checkout and report its structure. Do not modify files.";
  await page.getByTestId("new-coding-session-goal").fill(goal);
  await page.getByTestId("coding-session-worktree-toggle").click();
  await page
    .getByTestId("coding-session-workdir-input")
    .fill("C:\\missing\\beekeeper");
  await page.getByTestId("new-coding-session-submit").click();

  const creates = () =>
    page.evaluate(async () => {
      const query = window.__BUZZ_E2E_INVOKE_MOCK_COMMAND__;
      if (!query) throw new Error("mock query missing");
      const answer = await query("query_relay_filters", {
        filters: [
          {
            kinds: [44221],
            "#h": ["1c7e1c02-87bb-5e88-b2da-5a7a9432d0c9"],
            limit: 100,
          },
        ],
      });
      const events = Array.isArray(answer)
        ? answer
        : (answer as { events: unknown[] }).events;
      return events as Array<{ id: string; content: string; tags: string[][] }>;
    });
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
      const seed = window.__BUZZ_E2E_SEED_MOCK_SIGNED_EVENT__;
      if (!seed) throw new Error("signed-event seed missing");
      seed({ channelName: "engineering", event });
    }, receipt);
  };
  await answer(original.commandId, true);
  await expect(page.getByTestId("pending-coding-session-retry")).toBeDisabled();
  await expect(
    page.getByTestId("pending-coding-session-fix-workdir"),
  ).toBeEnabled({ timeout: 15_000 });
  await page.getByTestId("pending-coding-session-fix-workdir").click();
  await expect(page.getByTestId("new-coding-session-goal")).toHaveValue(goal);
  await expect(page.getByTestId("new-coding-session-status")).toHaveCount(0);
  await expect(page.getByTestId("new-coding-session-blocker-goal")).toHaveCount(
    0,
  );
  await expect(page.getByTestId("new-coding-session-blocker-busy")).toHaveCount(
    0,
  );
  await page
    .getByTestId("coding-session-workdir-input")
    .fill("C:\\work\\beekeeper");
  await page.getByTestId("new-coding-session-submit").click();
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
  await answer(repaired.commandId, false);
  await expect(page.getByTestId("pending-coding-session-retry")).toBeDisabled();
  await expect(
    page.getByTestId("pending-coding-session-fix-workdir"),
  ).toHaveCount(0);
  const receipts = await page.evaluate(async (kind) => {
    const answer = await window.__BUZZ_E2E_INVOKE_MOCK_COMMAND__?.(
      "query_relay_filters",
      {
        filters: [
          {
            kinds: [kind],
            "#h": ["1c7e1c02-87bb-5e88-b2da-5a7a9432d0c9"],
            limit: 100,
          },
        ],
      },
    );
    return Array.isArray(answer)
      ? answer
      : (answer as { events: Array<{ content: string }> }).events;
  }, KIND_CODING_SESSION_LIFECYCLE_RECEIPT);
  expect(
    receipts.filter(
      (event: { content: string }) =>
        JSON.parse(event.content).status === "created",
    ),
  ).toHaveLength(1);
});
