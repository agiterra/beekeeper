import { createHash } from "node:crypto";

import { expect, type Locator, type Page, test } from "@playwright/test";

import type { RelayEvent } from "@/shared/api/types";
import { waitForAnimations } from "../helpers/animations";
import { installMockBridge } from "../helpers/bridge";
import { shotPath } from "../helpers/shotPath";
import {
  SV29_CHANNEL_NAME,
  SV29_HEAD,
  SV29_PROVIDER,
  sv29BeforeEvents,
  sv29Metadata,
  sv29Receipt,
  sv29RewindCommand,
  sv29Target,
  sv29Transcript,
  sv29Turn,
} from "./coding-session-sv29-rewind.fixtures";

// SV-29 (batch C3b): "Edit from here" on a prompt. The dialog offers "Chat
// only" and "Chat and files" exactly as the provider would accept them, says
// why either is off, and reports the provider's signed answer: a cut that
// could not reopen "still remembers"; a completed rewind opens generation 2
// with a collapsed "N turns rewound by … · files restored" row and a ↶ mark
// on the minimap. Real signed events through the mock relay; scoped shots,
// gated on distinct hashes.

const SHOTS = "test-results/sv29-rewind";

async function seed(page: Page, events: RelayEvent[]): Promise<void> {
  await page.evaluate(
    ({ name, signedEvents }) => {
      const hook = window.__BEEKEEPER_E2E_SEED_MOCK_SIGNED_EVENT__;
      if (!hook) throw new Error("signed-event seeding hook is missing");
      for (const event of signedEvents) hook({ channelName: name, event });
    },
    { name: SV29_CHANNEL_NAME, signedEvents: events },
  );
}

async function openSession(
  page: Page,
  events: RelayEvent[],
  expectText: string,
): Promise<Locator> {
  await page.setViewportSize({ width: 1440, height: 900 });
  await installMockBridge(page, {
    globalAgentConfig: {
      env_vars: {},
      provider: null,
      model: null,
      "allowed-bridge-pubkeys": [
        { pubkey: SV29_PROVIDER, label: "SV-29 provider" },
      ],
    },
  });
  await page.goto("/");
  await page.getByTestId(`channel-${SV29_CHANNEL_NAME}`).click();
  await seed(page, events);
  const trigger = page.getByTestId("channel-coding-sessions-trigger");
  await expect(trigger).toHaveAttribute(
    "aria-label",
    /Coding sessions \(\d+\)/,
    {
      timeout: 15_000,
    },
  );
  await trigger.click();
  await page.getByTestId("channel-coding-session-open").first().click();
  // A rewound execution has two generations, so it opens on the umbrella
  // surface; a single generation opens the single-session workspace.
  const workspace = page
    .getByTestId("coding-session-workspace")
    .or(page.getByTestId("coding-session-umbrella-workspace"));
  await expect(workspace).toContainText(expectText, { timeout: 15_000 });
  return workspace;
}

async function openRewind(page: Page, prompt: Locator): Promise<Locator> {
  await prompt.hover();
  await prompt.getByTestId("coding-session-user-message-rewind").click();
  const dialog = page.getByTestId("coding-session-rewind-dialog");
  await expect(dialog).toBeVisible();
  return dialog;
}

const hashes = new Map<string, string>();

function shooter(page: Page, testInfo: Parameters<typeof shotPath>[0]) {
  return async (name: string, locator: Locator) => {
    await expect(locator).toBeVisible();
    await waitForAnimations(page);
    const png = await locator.screenshot({
      path: shotPath(testInfo, SHOTS, name),
    });
    hashes.set(name, createHash("sha256").update(png).digest("hex"));
  };
}

test.describe.configure({ mode: "serial" });

test("SV-29: the dialog, a turn this build cannot rewind, and a cut that did not reopen", async ({
  page,
}, testInfo) => {
  test.setTimeout(90_000);
  const shoot = shooter(page, testInfo);
  const before = sv29BeforeEvents();
  const workspace = await openSession(
    page,
    before.events,
    "Inlined five helpers.",
  );
  const prompts = workspace.getByTestId("coding-session-user-message");
  await expect(prompts).toHaveCount(3);

  // sv29-dialog: a restorable git checkpoint offers both choices.
  let dialog = await openRewind(page, prompts.nth(0));
  await expect(dialog).toContainText("Edit from here");
  await expect(dialog).toContainText("Add a failing test for the parser.");
  await expect(
    dialog.getByTestId("coding-session-rewind-choice-keep"),
  ).toHaveAttribute("data-enabled", "true");
  await expect(
    dialog.getByTestId("coding-session-rewind-choice-restore"),
  ).toHaveAttribute("data-enabled", "true");
  await expect(dialog).toContainText("detached, not erased");
  await shoot("sv29-dialog", dialog);
  await page.keyboard.press("Escape");
  await expect(dialog).toBeHidden();

  // sv29-disabled-not-restorable: turn 3's checkpoint came from a build that
  // cannot rewind — git trees, `restorable: false` — so neither is offered.
  dialog = await openRewind(page, prompts.nth(2));
  for (const files of ["keep", "restore"]) {
    await expect(
      dialog.getByTestId(`coding-session-rewind-choice-${files}`),
    ).toBeDisabled();
    await expect(
      dialog.getByTestId(`coding-session-rewind-choice-${files}-reason`),
    ).toHaveText("This provider build cannot rewind");
  }
  await shoot("sv29-disabled-not-restorable", dialog);
  await page.keyboard.press("Escape");
  await expect(dialog).toBeHidden();

  // sv29-still-remembers: rewind to before turn 2 with files; the provider
  // restores the files but cannot open generation 2, so it reopens 1.
  dialog = await openRewind(page, prompts.nth(1));
  await dialog.getByTestId("coding-session-rewind-choice-restore").click();
  await expect(
    dialog.getByTestId("coding-session-rewind-outcome"),
  ).toHaveAttribute("data-outcome", "pending");
  const readRewinds = () =>
    page.evaluate(() =>
      (window.__BEEKEEPER_E2E_SIGNED_EVENTS__ ?? [])
        .filter((event) => event.kind === 44221)
        .map((event) => JSON.parse(event.content)),
    );
  await expect.poll(readRewinds, { timeout: 10_000 }).toHaveLength(1);
  const [published] = await readRewinds();
  expect(published.action).toEqual({
    type: "session.rewind",
    session: sv29Target(1),
    providerAuthorityPubkey: SV29_PROVIDER,
    checkpoint: before.turn2Checkpoint.id,
    files: "restore",
  });
  await seed(page, [
    sv29Receipt(published.commandId, {
      status: "failed",
      session: null,
      error: {
        code: "REWIND_NOT_RESTARTED",
        message:
          "The new conversation did not open: the agent was not restarted; generation 1 was reopened and still remembers turns 2–3.",
      },
      rewind: {
        checkpoint: before.turn2Checkpoint.id,
        cutGeneration: 1,
        cutAfterSeq: 3,
        previousGeneration: 1,
        files: "restored",
        preRewindCheckpoint: "c2".repeat(32),
        head: SV29_HEAD,
      },
    }),
  ]);
  const outcome = dialog.getByTestId("coding-session-rewind-outcome");
  await expect(outcome).toHaveAttribute("data-outcome", "not-restarted");
  await expect(outcome).toContainText("still remembers turns 2–3");
  await expect(outcome).toContainText("Files restored to before this turn");
  await shoot("sv29-still-remembers", dialog);
});

test("SV-29: a rewound generation opens with a collapsed row and a minimap mark", async ({
  page,
}, testInfo) => {
  test.setTimeout(90_000);
  const shoot = shooter(page, testInfo);
  const before = sv29BeforeEvents();
  const g1 = sv29Target(1);
  const g2 = sv29Target(2);
  const commandId = "csl-sv29-rewind-1";
  const events = [
    ...before.events,
    sv29Metadata(g1, "disconnected"),
    sv29RewindCommand(commandId, g1, before.turn2Checkpoint.id, "restore"),
    sv29Receipt(commandId, {
      status: "resumed",
      session: g2,
      error: null,
      rewind: {
        checkpoint: before.turn2Checkpoint.id,
        cutGeneration: 1,
        cutAfterSeq: 3,
        previousGeneration: 1,
        files: "restored",
        preRewindCheckpoint: "c2".repeat(32),
        head: SV29_HEAD,
      },
    }),
    sv29Metadata(g2),
    sv29Transcript(g2, 1, null, {
      kind: "status",
      status: "session_rewound",
      commandId,
      checkpoint: before.turn2Checkpoint.id,
      cutGeneration: 1,
      cutAfterSeq: 3,
      previousGeneration: 1,
      files: "restored",
      memory: "seeded",
    }),
    ...sv29Turn(
      g2,
      2,
      "g2-turn-1",
      "Fix the parser, but keep the helpers.",
      "Fixed parse(); helpers kept.",
    ),
    ...sv29Turn(g2, 5, "g2-turn-2", "Run the suite.", "All 41 tests pass."),
  ];
  const workspace = await openSession(page, events, "All 41 tests pass.");

  const row = workspace.getByTestId("coding-session-rewound-row");
  await expect(row).toHaveCount(1);
  await expect(row).toContainText("rewound by", { timeout: 15_000 });
  await expect(row).toContainText("files restored");
  await row.getByTestId("coding-session-rewound-row-toggle").click();
  const details = row.getByTestId("coding-session-rewound-row-details");
  await expect(details).toContainText("seeded from the record");
  await expect(details).toContainText("HEAD unchanged at eeeeeee");
  await expect(details).toContainText("detached, not erased");
  await shoot("sv29-rewound-row", row);

  const mark = page.getByTestId("coding-session-minimap-mark-rewind");
  await expect(mark).toHaveCount(1);
  await expect(mark).toHaveText("↶");
  await shoot(
    "sv29-minimap-rewind-mark",
    page.getByTestId("coding-session-minimap"),
  );

  const seen = new Map<string, string>();
  for (const [name, hash] of hashes) {
    expect(
      seen.get(hash),
      `${name} is byte-identical to ${seen.get(hash)}`,
    ).toBeUndefined();
    seen.set(hash, name);
  }
  expect(hashes.size).toBe(5);
});
