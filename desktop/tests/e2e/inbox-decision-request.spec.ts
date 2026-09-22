import { expect, test } from "@playwright/test";

import {
  finalizeEvent,
  generateSecretKey,
  getPublicKey,
} from "nostr-tools/pure";

import { buildCodingSessionTargetKey } from "@/features/coding-sessions/lib/codingSessionCommand";
import {
  BUZZ_CODING_SESSION_METADATA_SCHEMA,
  CODING_SESSION_METADATA_TAG_VERSION,
  codingSessionMetadataSemanticKey,
} from "@/features/coding-sessions/lib/codingSessionIngressPayloads";
import { KIND_CODING_SESSION_METADATA } from "@/shared/constants/kinds";

import { installMockBridge } from "../helpers/bridge";

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

/** The provider that runs the lead's seat, and the lead (the asker). */
const PROVIDER_SECRET = generateSecretKey();
const PROVIDER_PUBKEY = getPublicKey(PROVIDER_SECRET);
const ASKER_PUBKEY = getPublicKey(generateSecretKey());
const LEAD_TARGET = {
  driver: "claude-agent-acp",
  instanceId: "l249-lead",
  sessionId: "55555555-6666-7777-8888-999999999999",
  generation: 1,
};

/** The lead's 44223: the seat the answer's wake must reach. */
function leadMetadata() {
  return finalizeEvent(
    {
      kind: KIND_CODING_SESSION_METADATA,
      created_at: Math.floor(Date.now() / 1000) - 60,
      tags: [
        ["h", GENERAL_CHANNEL_ID],
        ["csm-v", CODING_SESSION_METADATA_TAG_VERSION],
        ["cs-target", buildCodingSessionTargetKey(LEAD_TARGET)],
        ["csm-key", codingSessionMetadataSemanticKey(LEAD_TARGET)],
      ],
      content: JSON.stringify({
        schema: BUZZ_CODING_SESSION_METADATA_SCHEMA,
        session: LEAD_TARGET,
        projectRef: null,
        repoRef: null,
        title: "kettle-control-2",
        agentRef: ASKER_PUBKEY,
        role: "lead",
        provider: LEAD_TARGET.driver,
        runtime: LEAD_TARGET.driver,
        model: "sonnet",
        status: "running",
        branch: null,
        capabilities: {
          threadTurnStart: true,
          threadTurnInterrupt: true,
          threadSteer: true,
          context: false,
          diff: false,
          plan: true,
        },
        sessionRef: SESSION_REF,
      }),
    },
    PROVIDER_SECRET,
  );
}

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
    await installMockBridge(page, {
      globalAgentConfig: {
        env_vars: {},
        provider: null,
        model: null,
        "allowed-bridge-pubkeys": [{ pubkey: PROVIDER_PUBKEY, label: "lead" }],
      },
    });
    await page.goto("/#/?tab=inbox");
    await expect(page.getByTestId("home-inbox-list")).toBeVisible();
    await page.waitForFunction(() => {
      const win = window as MockWindow;
      return (
        typeof win.__BUZZ_E2E_EMIT_MOCK_MESSAGE__ === "function" &&
        typeof win.__BUZZ_E2E_PUSH_MOCK_FEED_ITEM__ === "function"
      );
    });
    await page.evaluate((event) => {
      const seed = window.__BUZZ_E2E_SEED_MOCK_SIGNED_EVENT__;
      if (!seed) throw new Error("signed-event seeding hook is missing");
      seed({ channelName: "general", event: event as never });
    }, leadMetadata());

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
        senderPubkey: ASKER_PUBKEY,
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
    await options.nth(1).click();
    await expect(card.getByTestId("decision-answer-published")).toBeVisible();
    // Answering is not done until the asker is told (`decide answer`'s
    // default): exactly one 44220 to the lead's execution, naming the answer.
    await expect(card.getByTestId("decision-request-wake-sent")).toContainText(
      "Woke lead",
    );
    const signed = await page.evaluate(
      () =>
        (
          window as unknown as {
            __BUZZ_E2E_SIGNED_EVENTS__: {
              kind: number;
              content: string;
              tags: string[][];
            }[];
          }
        ).__BUZZ_E2E_SIGNED_EVENTS__,
    );
    const answers = signed.filter(
      (event) =>
        event.kind === 44244 &&
        event.tags.some(
          (tag) => tag[0] === "cstx-type" && tag[1] === "decision.answer",
        ),
    );
    expect(answers).toHaveLength(1);
    expect(JSON.parse(answers[0]?.content ?? "{}").body.requestRef).toBe(
      REQUEST_ID,
    );
    const wakes = signed.filter((event) => event.kind === 44220);
    expect(wakes).toHaveLength(1);
    const wake = JSON.parse(wakes[0]?.content ?? "{}");
    expect(wake.target).toEqual(LEAD_TARGET);
    expect(wake.action.type).toBe("thread.turn.start");
    // Boundary is the contract default and is omitted on the wire.
    expect(wake.action.deliver).toBeUndefined();
    expect(JSON.parse(wake.action.text).type).toBe("decision.answer");
  });
});
