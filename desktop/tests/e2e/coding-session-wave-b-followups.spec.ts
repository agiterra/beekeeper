import { createHash } from "node:crypto";

import { expect, test, type Locator, type Page } from "@playwright/test";

import { KIND_CODING_SESSION_LEASE } from "@/shared/constants/kinds";

import { waitForAnimations } from "../helpers/animations";
import { installMockBridge } from "../helpers/bridge";
import { openDashboardTab } from "../helpers/dashboard";

/**
 * Session-view parity, Wave B follow-ups (lane followups-tooling).
 *
 * SV-49 — the Agent Progress footer may not say a read "did not complete"
 * while that read is still in flight. Pending is not a verdict; only a read
 * that settled short earns the words.
 *
 * SV-50 — a mention sent before the channel's member read answers must still
 * reach the Invite prompt. The send waits for the read instead of treating
 * "unknown membership" as "everyone is a member".
 *
 * Each shot is scoped to its subject and the hashes are asserted distinct.
 */

const SHOTS = "test-results/session-parity-b";
const CHANNEL_ID = "9a1657ac-f7aa-5db0-b632-d8bbeb6dfb50";
const MOCK_VIEWER_PUBKEY = "deadbeef".repeat(8);
const ALLOWLIST_RELAY_AGENT_PUBKEY = "e".repeat(64);

const hashes = new Map<string, string>();

async function shoot(page: Page, locator: Locator, name: string) {
  await waitForAnimations(page);
  const buffer = await locator.screenshot({ path: `${SHOTS}/${name}.png` });
  const digest = createHash("sha256").update(buffer).digest("hex");
  for (const [other, otherDigest] of hashes) {
    expect(digest, `${name} captured the same pixels as ${other}`).not.toBe(
      otherDigest,
    );
  }
  hashes.set(name, digest);
}

/**
 * Boot on a channel with Agent Progress enabled. `hang` holds every session
 * lease read open (the panel's read stays pending); `reject` closes it (the
 * read settles incomplete).
 */
async function bootAgentProgress(
  page: Page,
  mode: { hang?: number[]; reject?: number[] },
) {
  await page.addInitScript(
    ({ hang, reject }) => {
      window.localStorage.setItem(
        "buzz-feature-overrides-v1",
        JSON.stringify({ "agent-progress": true }),
      );
      if (hang) window.__BEEKEEPER_E2E_HANG_PROJECT_QUERY_KINDS__ = hang;
      if (reject) window.__BEEKEEPER_E2E_REJECT_PROJECT_QUERY_KINDS__ = reject;
    },
    { hang: mode.hang ?? null, reject: mode.reject ?? null },
  );
  await installMockBridge(page);
  await page.goto(`/#/channels/${CHANNEL_ID}`, {
    waitUntil: "domcontentloaded",
  });
  await expect(page.getByTestId("open-dashboard-view")).toBeVisible({
    timeout: 10_000,
  });
  await openDashboardTab(page, "agent-progress");
  await expect(page.getByTestId("agent-progress-panel")).toBeVisible({
    timeout: 10_000,
  });
}

test("SV-49: a pending read reads as reading, never as did not complete", async ({
  page,
}) => {
  await bootAgentProgress(page, { hang: [KIND_CODING_SESSION_LEASE] });
  const panel = page.getByTestId("agent-progress-panel");
  await expect(page.getByTestId("agent-progress-loading")).toBeVisible();
  await expect(page.getByTestId("agent-progress-footer-counts")).toHaveText(
    "Reading sessions…",
  );
  await expect(page.getByTestId("agent-progress-incomplete")).toHaveCount(0);
  await expect(panel).not.toContainText("did not complete");
  await shoot(page, panel, "SV-49-pending");
});

test("SV-49: a settled incomplete read says it did not complete", async ({
  page,
}) => {
  await bootAgentProgress(page, { reject: [KIND_CODING_SESSION_LEASE] });
  const panel = page.getByTestId("agent-progress-panel");
  // The panel's read queues behind boot's reads in the client's read budget
  // (see agentProgress.spec.ts), so it can take a few windows to settle.
  await expect(page.getByTestId("agent-progress-incomplete")).toBeVisible({
    timeout: 30_000,
  });
  await expect(page.getByTestId("agent-progress-footer-counts")).toHaveText(
    "This read did not complete — no sessions in what it returned",
  );
  await shoot(page, panel, "SV-49-incomplete");
});

test("SV-50: a mention sent before members resolve still asks to invite", async ({
  page,
}) => {
  await installMockBridge(page, {
    deferChannelMembersReads: true,
    relayAgents: [
      {
        pubkey: ALLOWLIST_RELAY_AGENT_PUBKEY,
        name: "quinn",
        respondTo: "allowlist",
        respondToAllowlist: [MOCK_VIEWER_PUBKEY],
        channelNames: ["general"],
      },
    ],
  });
  await page.goto("/");
  await page.getByTestId("channel-general").click();
  const input = page.getByTestId("message-input");
  await input.fill("@quinn");
  const quinnRow = page
    .getByTestId("message-composer")
    .getByTestId("mention-autocomplete")
    .locator("button", { hasText: "quinn" });
  await expect(quinnRow).toBeVisible();
  await quinnRow.click();
  await page.keyboard.type("hello");

  // The member read is still held. The send must wait on it rather than
  // treating the unknown list as "quinn is a member" and sending unprompted.
  await page.getByTestId("send-message").click();
  const invite = page.getByRole("button", { name: "Invite", exact: true });
  await expect(invite).toHaveCount(0);
  await expect(input).toContainText("hello");

  await page.evaluate(() =>
    window.__BEEKEEPER_E2E_RELEASE_CHANNEL_MEMBERS__?.(),
  );
  await expect(invite).toBeVisible({ timeout: 10_000 });
  await expect(page.getByRole("alertdialog")).toContainText("quinn");
  await shoot(page, page.getByRole("alertdialog"), "SV-50-invite-prompt");
});
