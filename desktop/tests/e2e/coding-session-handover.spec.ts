import { expect, test } from "@playwright/test";
import { mkdirSync } from "node:fs";

import { waitForAnimations } from "../helpers/animations";
import {
  BASE_SHA,
  CHANNEL_NAME,
  CHECKOUT_PATH,
  PROVIDER_A_PUBKEY,
  REPO_REF,
  CHECKOUT_REPORT,
  PATCH_TEXT,
  WIP_REF,
  WIP_SHA,
  acceptPublishedTakeover,
  answerPublishedCreate,
  landForeignTakeoverLive,
  openHandoverSession,
  recordedCheckouts,
  recordedHints,
  stubHandoverCheckout,
} from "./helpers/handoverAssertions";

/**
 * Continuing an absent participant's work, on the screen (§5).
 *
 * The story this spec holds to: A founded a session, worked on their own
 * provider, checkpointed, and went quiet. B — the signed-in viewer, holding a
 * live operator grant — takes the whole session over, reconstructs it on this
 * machine, and the screen says exactly what came across and what did not.
 *
 * What it refuses to accept: a "Continued" label with no outcome word, a
 * missing line that disappears because a wip ref landed, and an action offered
 * over a session somebody else already holds.
 */

const SHOTS = "test-results/handover";

test.beforeAll(() => {
  mkdirSync(SHOTS, { recursive: true });
});

test("B takes the session over, reconstructs it, and the panel says what came across", async ({
  page,
}) => {
  await openHandoverSession(page, "claimable");
  await stubHandoverCheckout(page);

  const panel = page.getByTestId("coding-session-handover");
  await expect(panel).toBeVisible({ timeout: 20_000 });
  await expect(
    page.getByTestId("coding-session-handover-status"),
  ).toContainText("No handover");
  const action = page.getByTestId("coding-session-handover-continue");
  await waitForAnimations(page);
  await panel.screenshot({ path: `${SHOTS}/01-continue-offered.png` });

  // The action waits for a directory: with none chosen it says which
  // prerequisite is missing rather than publishing a claim that would land on
  // an empty checkout.
  await expect(
    page.getByTestId("coding-session-handover-blocked"),
  ).toContainText("choose a checkout directory");
  await expect(action).toHaveCount(0);
  await page.getByTestId("coding-session-workdir-input").fill(CHECKOUT_PATH);
  await expect(action).toBeVisible();
  // The consequence is on screen before the click, not after it.
  await expect(panel).toContainText(
    "This hands over the whole session: every execution and assignment under it is fenced until you release or someone takes it back.",
  );
  await action.click();

  // The relay serializes the claim; the provider answers the create. Both are
  // answered here in the order the app performs them.
  const takeover = await acceptPublishedTakeover(page);
  const claim = JSON.parse(takeover.content) as {
    type: string;
    granteePubkey: string;
    bodyPubkey: string;
  };
  expect(claim.type).toBe("takeover");
  expect(claim.granteePubkey).toBe(takeover.pubkey);
  await answerPublishedCreate(page);

  // The native command was asked for exactly the ref and sha the checkpoint
  // named — not for "the latest", and not for a sha this app made up.
  await expect
    .poll(async () => (await recordedCheckouts(page)).length, {
      timeout: 20_000,
    })
    .toBeGreaterThan(0);
  const [first] = await recordedCheckouts(page);
  expect(first.request).toMatchObject({
    cwd: CHECKOUT_PATH,
    wipRef: WIP_REF,
    sha: WIP_SHA,
    // The remote is resolved from the checkout against this coordinate, never
    // assumed to be called `origin`.
    repoRef: REPO_REF,
    // A's uncommitted bytes came across: the app fetched the NIP-34 patch the
    // checkpoint pointed at, verified its author, and handed the diff itself
    // to the native command — not the pointer.
    patchText: PATCH_TEXT,
    baseSha: BASE_SHA,
  });

  // The create is bound to the folder the person picked, by command id — the
  // provider resolves `pending[commandId]` before any project or channel
  // default, so the model cannot come up in the old mapped folder.
  await expect
    .poll(async () => (await recordedHints(page)).length, { timeout: 20_000 })
    .toBeGreaterThan(0);
  const [hint] = await recordedHints(page);
  expect(hint.path).toBe(CHECKOUT_PATH);
  const createCommandId = await page.evaluate(() => {
    const signed = window.__BUZZ_E2E_SIGNED_EVENTS__ ?? [];
    for (const event of signed) {
      if (event.kind !== 44221) continue;
      const content = JSON.parse(event.content) as {
        commandId: string;
        action?: { type?: string };
      };
      if (content.action?.type === "session.create") return content.commandId;
    }
    return null;
  });
  expect(hint.commandId).toBe(createCommandId);

  // The outcome is labelled, the evidence is linkable, and the disclosure
  // survives the wip ref having landed.
  await expect(
    page.getByTestId("coding-session-handover-outcome"),
  ).toContainText("Reconstructed", { timeout: 30_000 });
  await expect(panel).toContainText(
    "The original execution's own context stayed on its machine.",
  );
  await expect(panel).toContainText(`Recovered: wip-ref ${WIP_REF}`);
  const missing = page.getByTestId("coding-session-handover-missing");
  await expect(missing).toContainText("Not all uncommitted work was preserved");
  await expect(missing).toContainText(CHECKOUT_REPORT.missing[0]);
  const evidence = page.getByTestId("coding-session-handover-evidence");
  await expect(evidence).toContainText("Claim");
  await expect(evidence).toContainText("Checkpoint");
  await expect(evidence).toContainText("Continuation");
  await waitForAnimations(page);
  await panel.screenshot({ path: `${SHOTS}/02-reconstructed.png` });

  // Nothing was steered on A's execution: the whole point of reconstruction is
  // that the absent machine is not commanded.
  const commands = await page.evaluate(
    () => window.__BUZZ_E2E_COMMANDS__ ?? [],
  );
  expect(commands.filter((command) => command.includes("turn"))).toEqual([]);
  // The checkout itself is asserted above, from the stub's own record: it
  // never reaches the bridge's command log, because the stub answers first.
  expect(await recordedCheckouts(page)).toHaveLength(1);
});

test("a takeover that lands while the session is open reaches the panel", async ({
  page,
}) => {
  // The panel is a live surface, not a snapshot: nothing else in this app
  // carries 44228 or 44247 on a live REQ, so without this surface's own
  // subscription a claim landing under an open workspace would be invisible
  // until a remount.
  const chain = await openHandoverSession(page, "claimable");
  const status = page.getByTestId("coding-session-handover-status");
  await expect(status).toContainText("No handover", { timeout: 20_000 });

  // The REQ itself, before anything is seeded: this surface carries the two
  // kinds nothing else in the app subscribes to.
  for (const kind of [44228, 44247]) {
    await expect
      .poll(
        async () =>
          page.evaluate(
            ({ channelName, one }) =>
              window.__BUZZ_E2E_HAS_MOCK_LIVE_SUBSCRIPTION__?.({
                channelName,
                kind: one,
              }) ?? false,
            { channelName: CHANNEL_NAME, one: kind },
          ),
        { timeout: 15_000 },
      )
      .toBe(true);
  }

  await landForeignTakeoverLive(page, chain);

  // No click, no navigation, no reload between the seed and this assertion.
  await expect(status).toContainText("Active", { timeout: 20_000 });
  await expect(
    page.getByTestId("coding-session-handover-fenced"),
  ).toContainText("Fenced", { timeout: 20_000 });
  await expect(
    page.getByTestId("coding-session-handover-continue"),
  ).toHaveCount(0);
});

test("A's execution reads fenced, in a word, and offers the way back", async ({
  page,
}) => {
  await openHandoverSession(page, "fenced");
  const panel = page.getByTestId("coding-session-handover");
  await expect(panel).toBeVisible({ timeout: 20_000 });
  await expect(
    page.getByTestId("coding-session-handover-status"),
  ).toContainText("Active");
  await expect(
    page.getByTestId("coding-session-handover-fenced"),
  ).toContainText("Fenced");
  await expect(panel).toContainText(
    "the body holding this session, so its turns will be refused",
  );
  await expect(
    page.getByTestId("coding-session-handover-take-back"),
  ).toHaveText("Take this session back");
  // One act, one button: with nothing checkpointed there is nothing to
  // reconstruct, so the way back is the only thing offered.
  await expect(
    page.getByTestId("coding-session-handover-continue"),
  ).toHaveCount(0);
  await waitForAnimations(page);
  await panel.screenshot({ path: `${SHOTS}/03-fenced.png` });
});

test("taking a dead machine's session back says the fence moved, not that it resumed", async ({
  page,
}) => {
  // The composition's finding, on the screen: A's machine died, the session is
  // taken back onto it, and the very next turn would come back
  // `turn_dropped / NO_LIVE_EXECUTION`. The panel must not read a claim
  // receipt as a running execution.
  await openHandoverSession(page, "fenced", undefined, {
    localBodyPubkey: PROVIDER_A_PUBKEY,
  });
  const takeBack = page.getByTestId("coding-session-handover-take-back");
  await expect(takeBack).toBeVisible({ timeout: 20_000 });
  await takeBack.click();
  await acceptPublishedTakeover(page);

  const nextAction = page.getByTestId("coding-session-handover-next-action");
  await expect(nextAction).toBeVisible({ timeout: 20_000 });
  await expect(nextAction).toHaveAttribute("data-liveness", "not-live");
  await expect(nextAction).toContainText("No live execution on");
  await expect(nextAction).toContainText(
    "Reconnect it, or re-address an owed turn.",
  );
  // Both routes are named as the controls really are.
  await expect(nextAction).toContainText("Reconnect");
  await expect(nextAction).toContainText("Resend to the resumed execution");
  await waitForAnimations(page);
  await page
    .getByTestId("coding-session-handover")
    .screenshot({ path: `${SHOTS}/07-taken-back-no-live-execution.png` });
});

test("taking back a live machine's session sends the holder to the composer", async ({
  page,
}) => {
  await openHandoverSession(page, "fenced", undefined, {
    localBodyPubkey: PROVIDER_A_PUBKEY,
    liveLease: true,
  });
  const takeBack = page.getByTestId("coding-session-handover-take-back");
  await expect(takeBack).toBeVisible({ timeout: 20_000 });
  await takeBack.click();
  await acceptPublishedTakeover(page);

  const nextAction = page.getByTestId("coding-session-handover-next-action");
  await expect(nextAction).toBeVisible({ timeout: 20_000 });
  await expect(nextAction).toHaveAttribute("data-liveness", "live");
  await expect(nextAction).toContainText(
    "is live — continue from the composer.",
  );
  await expect(nextAction).not.toContainText("Reconnect");
  await waitForAnimations(page);
  await page
    .getByTestId("coding-session-handover")
    .screenshot({ path: `${SHOTS}/08-taken-back-live.png` });
});

test("a deleted session says so and offers nothing", async ({ page }) => {
  await openHandoverSession(page, "retired");
  const panel = page.getByTestId("coding-session-handover");
  await expect(panel).toBeVisible({ timeout: 20_000 });
  await expect(
    page.getByTestId("coding-session-handover-retired"),
  ).toContainText("Deleted");
  await expect(panel).toContainText("Nothing here can be continued");
  await expect(
    page.getByTestId("coding-session-handover-continue"),
  ).toHaveCount(0);
  await expect(
    page.getByTestId("coding-session-handover-take-back"),
  ).toHaveCount(0);
  await waitForAnimations(page);
  await panel.screenshot({ path: `${SHOTS}/04-retired.png` });
});

test("the panel survives a narrow window and 250% text", async ({ page }) => {
  await openHandoverSession(page, "fenced", { width: 640, height: 900 });
  const panel = page.getByTestId("coding-session-handover");
  await expect(panel).toBeVisible({ timeout: 20_000 });
  await waitForAnimations(page);
  await panel.screenshot({ path: `${SHOTS}/05-narrow-640.png` });

  // Zoom is a root font-size change in this app; rem-based text scales with
  // it, and anything frozen in px would be obvious in this shot.
  await page.evaluate(() => {
    document.documentElement.style.fontSize = "40px";
  });
  await waitForAnimations(page);
  await expect(panel).toBeVisible();
  await expect(
    page.getByTestId("coding-session-handover-fenced"),
  ).toContainText("Fenced");
  await panel.screenshot({ path: `${SHOTS}/06-zoom-250.png` });
});
