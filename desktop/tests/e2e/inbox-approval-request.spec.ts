import { expect, test } from "@playwright/test";

import { installMockBridge, TEST_IDENTITIES } from "../helpers/bridge";

/**
 * Ledger 238(e). The kind-46010 approval request for the 2026-09-22
 * kettle-control run (13:41:56Z) never appeared in the founder's Inbox; he
 * found it only in the Actions tab, 49 minutes later.
 *
 * The Inbox admits an approval on one test — `"#p": [my_pubkey]`, in
 * `desktop/src-tauri/src/commands/messages.rs` — and the relay addressed the
 * request to the *workflow owner* alone, never to the `project-owner:<coord>`
 * creator the `approverSpec` names. This spec pins the desktop half of that
 * contract: an approval addressed to the viewer renders the card, and the
 * card names the step. It is the surface that was silent, so it is the
 * surface that gets a test — there was none before this lane.
 *
 * The relay half — the second `p` tag — is fenced in
 * `crates/beekeeper-relay/src/workflow_sink.rs`.
 */

const GENERAL_CHANNEL_ID = "9a1657ac-f7aa-5db0-b632-d8bbeb6dfb50";
/** The live run the founder never saw an Inbox card for. */
const RUN_ID = "59e52df0-7c21-4f4a-9b5f-2d0f6e3a1c88";
const WORKFLOW_ID = "0f6c1b94-3d52-4a77-9c10-6b2e8f0a4d31";
const APPROVAL_REF = "5f".repeat(32);
const APPROVAL_EVENT_ID = "7c".repeat(32);

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

test.describe("inbox approval request", () => {
  test("a project-owner approval addressed to the viewer renders its card", async ({
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

    // The relay's own wire shape (`approval_request_wire`): `d` is the
    // approval ref, `p` addresses the approver, `buzz:workflow` keeps the
    // event out of trigger matching, and the content is
    // `buzz-approval-request/v1`.
    const approval = await page.evaluate(
      ({
        viewer,
        senderPubkey,
        runId,
        workflowId,
        approvalRef,
        eventId,
        channelId,
      }) => {
        const win = window as MockWindow;
        const emit = win.__BUZZ_E2E_EMIT_MOCK_MESSAGE__;
        const push = win.__BUZZ_E2E_PUSH_MOCK_FEED_ITEM__;
        if (!emit || !push) throw new Error("Bridge helpers not ready");
        const event = emit({
          channelName: "general",
          kind: 46010,
          id: eventId,
          pubkey: senderPubkey,
          mentionPubkeys: [viewer],
          extraTags: [
            ["d", approvalRef],
            ["buzz:workflow", "true"],
          ],
          content: JSON.stringify({
            schema: "buzz-approval-request/v1",
            runId,
            workflowId,
            workflowName: "kettle-control",
            stepId: "control-run",
            stepIndex: 0,
            approverSpec: `project-owner:30621:${viewer}:kettle-control`,
            message: "Run the kettle-control gate on this computer?",
            expiresAt: Math.floor(Date.now() / 1000) + 3600,
            synthetic: true,
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
        viewer: TEST_IDENTITIES.tyler.pubkey,
        senderPubkey: TEST_IDENTITIES.alice.pubkey,
        runId: RUN_ID,
        workflowId: WORKFLOW_ID,
        approvalRef: APPROVAL_REF,
        eventId: APPROVAL_EVENT_ID,
        channelId: GENERAL_CHANNEL_ID,
      },
    );

    const row = page.getByTestId(`home-inbox-item-${approval.id}`);
    await expect(row).toBeVisible();
    await row.click();

    // The card, not the raw JSON body: this is what Brian was never shown.
    const card = page.getByTestId("host-step-approval-card");
    await expect(card).toBeVisible();
    await expect(card).toContainText("control-run");
    // And the request's own JSON must not be what the row renders.
    await expect(card).not.toContainText("buzz-approval-request/v1");
  });
});
