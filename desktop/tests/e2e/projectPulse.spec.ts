import { createHash } from "node:crypto";

import { expect, test, type Page } from "@playwright/test";
import { finalizeEvent, generateSecretKey } from "nostr-tools/pure";

import { KIND_PULSE_ENTRY } from "@/shared/constants/kinds";
import type { RelayEvent } from "@/shared/api/types";
import { waitForAnimations } from "../helpers/animations";
import { installMockBridge } from "../helpers/bridge";

/**
 * Project Pulse, end to end through the mock relay.
 *
 * The entries are really signed and really decoded: the screen fails closed on
 * an unsigned event, so seeding real bytes is the only way to see it paint.
 * The read-only states arrive through `__BUZZ_E2E_EXTRA_PROJECT_EVENTS__`,
 * which stores arbitrary kinds and matches them by `filter.kinds` + `#a` —
 * exactly the filter the Pulse read issues.
 */

const AUTHOR_SECRET = generateSecretKey();
const PEER_SECRET = generateSecretKey();
/** `DEFAULT_MOCK_IDENTITY.pubkey` in the bridge — the project's owner. */
const MOCK_IDENTITY_PUBKEY = "deadbeef".repeat(8);
const PROJECT_DTAG = "pulse-demo";
const QUIET_DTAG = "quiet-demo";
const PROJECT_COORDINATE = `30621:${MOCK_IDENTITY_PUBKEY}:${PROJECT_DTAG}`;

const SCREENSHOT_DIR = "test-results/project-pulse";
const hashes = new Map<string, string>();

function pulseEntry(input: {
  secret: Uint8Array;
  createdAtOffset: number;
  type: "plan" | "milestone" | "note" | "handoff" | "blocker";
  text: string;
  branch?: string | null;
  codeAreas?: string[];
  supersedes?: string | null;
}): RelayEvent {
  const branch = input.branch ?? null;
  const tags: string[][] = [
    ["a", PROJECT_COORDINATE],
    ["pu-v", "pu1-1"],
    ["pu-type", input.type],
  ];
  if (branch !== null) tags.push(["branch", branch]);
  return finalizeEvent(
    {
      kind: KIND_PULSE_ENTRY,
      created_at: Math.floor(Date.now() / 1_000) - input.createdAtOffset,
      tags,
      content: JSON.stringify({
        schema: "buzz-pulse-entry/v1",
        type: input.type,
        text: input.text,
        codeAreas: input.codeAreas ?? [],
        branch,
        supersedes: input.supersedes ?? null,
      }),
    },
    input.secret,
  ) as unknown as RelayEvent;
}

function seededEntries(): RelayEvent[] {
  const original = pulseEntry({
    secret: AUTHOR_SECRET,
    createdAtOffset: 3_600,
    type: "plan",
    text: "First pass at the wire contract.",
    branch: "wip/project-pulse",
  });
  const revision = pulseEntry({
    secret: AUTHOR_SECRET,
    createdAtOffset: 1_800,
    type: "plan",
    text: "Wire contract landed; starting relay ingest.",
    branch: "wip/project-pulse",
    codeAreas: ["crates/buzz-relay/src/handlers/ingest.rs"],
    supersedes: original.id,
  });
  const blocker = pulseEntry({
    secret: AUTHOR_SECRET,
    createdAtOffset: 900,
    type: "blocker",
    text: "Do not touch pool.rs; the creation path is half-migrated.",
    codeAreas: ["crates/buzz-acp/src/pool.rs"],
  });
  const peerClaim = pulseEntry({
    secret: PEER_SECRET,
    createdAtOffset: 300,
    type: "plan",
    text: "Picking pool.rs back up.",
    supersedes: blocker.id,
  });
  return [original, revision, blocker, peerClaim];
}

async function boot(page: Page, extras: RelayEvent[]) {
  await page.addInitScript((events) => {
    window.__BUZZ_E2E_EXTRA_PROJECT_EVENTS__ = events as never;
  }, extras as never);
  await installMockBridge(page);
  await page.goto("/", { waitUntil: "domcontentloaded" });
  await expect(page.getByTestId("project-group-general")).toBeVisible({
    timeout: 10_000,
  });
}

async function createProject(page: Page, name: string) {
  await page.getByTestId("project-container-new").click();
  await page.getByTestId("create-project-container-name").fill(name);
  await page.getByTestId("create-project-container-submit").click();
}

async function openProjectScreen(page: Page, dtag: string) {
  const group = page.getByTestId(`project-group-${dtag}`);
  await expect(group).toBeVisible({ timeout: 10_000 });
  await group.hover();
  await page.getByTestId(`project-open-${dtag}`).click();
  await expect(page).toHaveURL(/\/projects\/[^/?]+\/?$/);
}

async function capture(page: Page, name: string) {
  await waitForAnimations(page);
  const path = `${SCREENSHOT_DIR}/${name}.png`;
  const buffer = await page.screenshot({ path });
  const digest = createHash("sha256").update(buffer).digest("hex");
  for (const [other, otherDigest] of hashes) {
    expect(digest, `${name} captured the same pixels as ${other}`).not.toBe(
      otherDigest,
    );
  }
  hashes.set(name, digest);
}

test("the project home card opens a Pulse that keeps every claim honest", async ({
  page,
}) => {
  await boot(page, seededEntries());
  await createProject(page, "Pulse Demo");
  await openProjectScreen(page, PROJECT_DTAG);

  const card = page.getByTestId("project-pulse-card");
  await expect(card).toBeVisible({ timeout: 10_000 });
  await expect(
    card.getByTestId("project-pulse-card-entry").first(),
  ).toBeVisible({ timeout: 10_000 });
  await capture(page, "01-project-home-card");

  await page.getByTestId("project-screen-open-pulse").click();
  await expect(page).toHaveURL(/\/projects\/[^/?]+\/pulse$/);
  const screen = page.getByTestId("project-pulse-screen");
  await expect(screen).toBeVisible({ timeout: 10_000 });
  await expect(page.getByTestId("pulse-header-subtitle")).toHaveText(
    "Explicit updates and observed session state.",
  );

  // The peer's supersession is shown as a claim, and the blocker it names is
  // still in the active set.
  await expect(page.getByTestId("pulse-entry-row")).toHaveCount(3, {
    timeout: 10_000,
  });
  await expect(
    page.getByTestId("pulse-entry-supersession-claimed"),
  ).toContainText("supersession claimed by");
  await expect(screen).toContainText(
    "Do not touch pool.rs; the creation path is half-migrated.",
  );
  await expect(page.getByTestId("pulse-empty")).toHaveCount(0);
  await expect(screen).not.toContainText("Live summaries");
  await expect(screen).not.toContainText("Automatic summary");
  await capture(page, "02-pulse-screen");

  // The author's own revision retired its predecessor — disclosed, not deleted.
  const toggle = page.getByTestId("pulse-superseded-toggle");
  await expect(toggle).toContainText("1 superseded entry");
  await toggle.click();
  await expect(page.getByTestId("pulse-entry-row")).toHaveCount(4);
  await expect(screen).toContainText("First pass at the wire contract.");
  await capture(page, "03-superseded-disclosed");

  // "no branch" is a real group, not a merge into a named branch.
  await expect(
    page.getByTestId("pulse-branch-chip").filter({ hasText: "no branch" }),
  ).toBeVisible();
});

test("the sidebar row reaches the same Pulse the home card does", async ({
  page,
}) => {
  await boot(page, seededEntries());
  await createProject(page, "Pulse Demo");
  // The row is present whether or not the project has Pulse content, so it is
  // asserted against the group rather than against seeded entries.
  const group = page.getByTestId(`project-group-${PROJECT_DTAG}`);
  await expect(group).toBeVisible({ timeout: 10_000 });
  const row = group.getByTestId("project-pulse-row");
  await expect(row).toBeVisible({ timeout: 10_000 });
  await row.click();
  await expect(page).toHaveURL(/\/projects\/[^/?]+\/pulse$/);
  await expect(page.getByTestId("project-pulse-screen")).toBeVisible({
    timeout: 10_000,
  });
});

test("a quiet project reads as confirmed empty, never as unavailable", async ({
  page,
}) => {
  await boot(page, []);
  await createProject(page, "Quiet Demo");
  await openProjectScreen(page, QUIET_DTAG);
  await page.getByTestId("project-screen-open-pulse").click();

  const empty = page.getByTestId("pulse-empty");
  await expect(empty).toBeVisible({ timeout: 10_000 });
  await expect(empty).toContainText("This read completed");
  await expect(page.getByTestId("pulse-unavailable")).toHaveCount(0);
  await expect(page.getByTestId("pulse-entry-row")).toHaveCount(0);
  await capture(page, "04-confirmed-empty");
});
