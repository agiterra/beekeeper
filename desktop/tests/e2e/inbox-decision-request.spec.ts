import { expect, test } from "@playwright/test";

import { installMockBridge, TEST_IDENTITIES } from "../helpers/bridge";

/**
 * Ledger 249(A). The kettle-control run 2 lead asked the founder a question
 * (kind 44244 `decision.request`, `heldOn: founder`, event 85266e05…) and
 * the founder's Inbox never showed it: the envelope is exactly five tags, so
 * it cannot carry the `p` tag the Inbox admitted by. The feed now admits it
 * by whom it is held on (`commands/messages/decision_inbox.rs`, unit-tested
 * there); this spec pins the surface: the card shows the question and the
 * request's own options, and answers through the Mission queue's path.
 */

const GENERAL_CHANNEL_ID = "9a1657ac-f7aa-5db0-b632-d8bbeb6dfb50";
const SESSION_REF = "5b7e1c2a-90d4-4b0e-a1f3-7c2d8e6f4a10";
const GENESIS_REF = "ab".repeat(32);
const REQUEST_ID = "85266e05".repeat(8);

type MockWindow = Window & {
  __BUZZ_E2E_EMIT_MOCK_MESSAGE__?: (input: {
    channelName: string;
    content: string;
    pubkey?: string;
    mentionPubkeys?: string[];
    id?: string;
    kind?: number;
    extraTags?: string[][];
  }) => {
    id: string;
    kind: number;
    pubkey: string;
    content: string;
    created_at: number;
    tags: string[][];
  };
  __BUZZ_E2E_PUSH_MOCK_FEED_ITEM__?: (item: {
    category: "mention" | "needs_action" | "activity" | "agent_activity";
    channel_id: string | null;
    channel_name: string;
    content: string;
    created_at: number;
    id: string;
    kind: number;
    pubkey: string;
    tags: string[][];
  }) => unknown;
};

test.describe("inbox decision request", () => {
  test("a founder-held decision request shows its question and options and answers", async ({
    page,
  }) => {
    await installMockBridge(page);
    await page.goto("/#/?tab=inbox");
    await expect(page.getByTestId("home-inbox-list")).toBeVisible();
    await page.waitForFunction(() => {
      const win = window as MockWindow;
      return (
        typeof win.__BUZZ_E2E_EMIT_MOCK_MESSAGE__ === "function" &&
        typeof win.__BUZZ_E2E_PUSH_MOCK_FEED_ITEM__ === "function"
      );
    });

    const request = await page.evaluate(
      ({ senderPubkey, sessionRef, genesisRef, eventId, channelId }) => {
        const win = window as MockWindow;
        const emit = win.__BUZZ_E2E_EMIT_MOCK_MESSAGE__;
        const push = win.__BUZZ_E2E_PUSH_MOCK_FEED_ITEM__;
        if (!emit || !push) throw new Error("Bridge helpers not ready");
        // Run 2's wire shape: the five NIP-CSTX tags, no `p`.
        const event = emit({
          channelName: "general",
          kind: 44244,
          id: eventId,
          pubkey: senderPubkey,
          extraTags: [
            ["d", sessionRef],
            ["cstx-v", "buzz-coding-session-team-transaction/v1"],
            ["cstx-genesis", genesisRef],
            ["cstx-type", "decision.request"],
          ],
          content: JSON.stringify({
            schema: "buzz-coding-session-team-transaction/v1",
            sessionRef,
            genesisRef,
            type: "decision.request",
            supersedes: null,
            deliveryCommandId: null,
            body: {
              question:
                "The seat has no git identity. Which should it commit as?",
              options: [
                "Configure the seat's own identity",
                "Commit as the founder",
              ],
              heldOn: "founder",
              blocks: [],
              recommendation: null,
            },
          }),
        });
        push({
          id: event.id,
          kind: event.kind,
          pubkey: event.pubkey,
          content: event.content,
          created_at: event.created_at,
          channel_id: channelId,
          channel_name: "general",
          tags: event.tags,
          category: "needs_action",
        });
        return event;
      },
      {
        senderPubkey: TEST_IDENTITIES.alice.pubkey,
        sessionRef: SESSION_REF,
        genesisRef: GENESIS_REF,
        eventId: REQUEST_ID,
        channelId: GENERAL_CHANNEL_ID,
      },
    );

    const row = page.getByTestId(`home-inbox-item-${request.id}`);
    await expect(row).toBeVisible();
    await expect(row).toContainText("Which should it commit as?");
    await row.click();

    const card = page.getByTestId("decision-request-card");
    await expect(card).toBeVisible();
    await expect(page.getByTestId("decision-request-question")).toContainText(
      "Which should it commit as?",
    );
    const options = card.getByTestId("decision-answer-option");
    await expect(options).toHaveCount(2);
    await expect(options.nth(1)).toHaveText("Commit as the founder");
    await expect(card).not.toContainText(
      "buzz-coding-session-team-transaction/v1",
    );
    // What this path does not do, said where the answer is given.
    await expect(
      card.getByTestId("decision-request-wake-disclosure"),
    ).toContainText("does not wake the asker");

    await options.nth(1).click();
    await expect(card.getByTestId("decision-answer-published")).toBeVisible();
  });
});
