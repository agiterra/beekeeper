import { createHash } from "node:crypto";
import { readFile } from "node:fs/promises";

import { expect, test, type Locator, type Page } from "@playwright/test";
import { finalizeEvent, generateSecretKey } from "nostr-tools/pure";

import { buildCodingSessionTargetKey } from "@/features/coding-sessions/lib/codingSessionCommand";
import {
  BEEKEEPER_CODING_SESSION_METADATA_SCHEMA,
  codingSessionMetadataSemanticKey,
  CODING_SESSION_METADATA_TAG_VERSION,
} from "@/features/coding-sessions/lib/codingSessionIngressPayloads";
import {
  BEEKEEPER_CODING_SESSION_TRANSCRIPT_SCHEMA,
  codingSessionTranscriptSemanticKey,
  CODING_SESSION_TRANSCRIPT_TAG_VERSION,
} from "@/features/coding-sessions/lib/codingSessionTranscriptPresentation";
import {
  KIND_CODING_SESSION_METADATA,
  KIND_CODING_SESSION_TRANSCRIPT,
  KIND_PROJECT_ANNOUNCEMENT,
  KIND_REPO_ANNOUNCEMENT,
} from "@/shared/constants/kinds";
import type { RelayEvent } from "@/shared/api/types";
import { waitForAnimations } from "../helpers/animations";
import { installMockBridge } from "../helpers/bridge";
import { openObservedSession } from "./helpers/codingSessionObservationAssertions";
import {
  bottomPanelToggle,
  rightPanelToggle,
} from "./helpers/codingSessionWaveBHeader";

// Session-view parity Wave B, lane B1: the session header (SV-20).
//
// `project / title ● Working` on the left; Details, Reopen, `⋯`, and the
// bottom- and right-panel toggles (⌘J, ⌘⌥B) on the right; no surface
// toggles; the metadata line in Details; and the right toggle's dot when an
// off-screen surface's badge needs attention. Every shot is scoped to its
// subject, and the last test gates the set on distinct sha256 hashes.

test.describe.configure({ mode: "serial" });

const SHOTS = "test-results/session-parity-b";
const SHOT_NAMES = [
  "SV20-header",
  "SV20-panel-toggles",
  "SV20-details-metadata",
  "SV20-attention-dot",
] as const;

// The mock community's first project and its repository (`e2eBridge.ts`,
// `MOCK_PROJECT_SEEDS[0]`), owned by the mock identity.
const MOCK_OWNER = "deadbeef".repeat(8);
const PROJECT_REF = `${KIND_PROJECT_ANNOUNCEMENT}:${MOCK_OWNER}:buzz`;
const REPO_REF = `${KIND_REPO_ANNOUNCEMENT}:${MOCK_OWNER}:buzz`;

const secret = generateSecretKey();
const channelName = "engineering";
const channelId = "1c7e1c02-87bb-5e88-b2da-5a7a9432d0c9";
const session = {
  driver: "claude-agent-acp",
  instanceId: "b1b1b1b1b1b1b1b1",
  sessionId: "b1000000-0000-4000-8000-000000000020",
  generation: 1,
};
const targetKey = buildCodingSessionTargetKey(session);

function signed(kind: number, seq: number, content: unknown, tags: string[][]) {
  return finalizeEvent(
    {
      kind,
      created_at: 1_800_600_000 + seq,
      tags: [["h", channelId], ...tags],
      content: JSON.stringify(content),
    },
    secret,
  ) as unknown as RelayEvent;
}

function metadata(): RelayEvent {
  return signed(
    KIND_CODING_SESSION_METADATA,
    0,
    {
      schema: BEEKEEPER_CODING_SESSION_METADATA_SCHEMA,
      session,
      projectRef: PROJECT_REF,
      repoRef: REPO_REF,
      title: "Header parity",
      agentRef: null,
      provider: "claude-agent-acp",
      runtime: "claude-agent-acp",
      model: "claude-opus-5[high]",
      status: "running",
      branch: null,
      capabilities: {
        threadTurnStart: true,
        threadTurnInterrupt: true,
        threadSteer: true,
        promptImage: false,
        context: false,
        diff: false,
        plan: false,
      },
    },
    [
      ["csm-v", CODING_SESSION_METADATA_TAG_VERSION],
      ["cs-target", targetKey],
      ["csm-key", codingSessionMetadataSemanticKey(session)],
    ],
  );
}

function transcript(seq: number, item: unknown): RelayEvent {
  return signed(
    KIND_CODING_SESSION_TRANSCRIPT,
    seq,
    {
      schema: BEEKEEPER_CODING_SESSION_TRANSCRIPT_SCHEMA,
      session,
      eventSeq: seq,
      timestamp: 1_800_600_000_000 + seq * 1_000,
      turnId: "header-turn",
      item,
    },
    [
      ["cst-v", CODING_SESSION_TRANSCRIPT_TAG_VERSION],
      ["cs-target", targetKey],
      ["cst-seq", String(seq)],
      ["cst-key", codingSessionTranscriptSemanticKey(session, seq)],
    ],
  );
}

function events(): RelayEvent[] {
  return [
    metadata(),
    transcript(1, {
      kind: "user_prompt",
      content: "Port T3's header: breadcrumb, status, panel toggles.",
    }),
    transcript(2, {
      kind: "assistant_text",
      text: "Working on the breadcrumb first.",
    }),
  ];
}

async function openSession(page: Page): Promise<Locator> {
  await installMockBridge(page);
  await page.setViewportSize({ width: 1440, height: 900 });
  await page.goto("/");
  await page.getByTestId(`channel-${channelName}`).click();
  await page.evaluate(
    ({ channelName: name, events: signedEvents }) => {
      const seed = window.__BEEKEEPER_E2E_SEED_MOCK_SIGNED_EVENT__;
      if (!seed) throw new Error("signed-event seeding hook is missing");
      for (const event of signedEvents) seed({ channelName: name, event });
    },
    { channelName, events: events() },
  );
  const trigger = page.getByTestId("channel-coding-sessions-trigger");
  await expect(trigger).toHaveAttribute("aria-label", "Coding sessions (1)", {
    timeout: 15_000,
  });
  await trigger.click();
  await page.getByTestId("channel-coding-session-open").click();
  const header = page.getByTestId("coding-session-header");
  await expect(header).toContainText("Header parity", { timeout: 15_000 });
  return header;
}

async function shoot(page: Page, name: string, locator: Locator) {
  await expect(locator).toBeVisible();
  await waitForAnimations(page);
  await locator.screenshot({ path: `${SHOTS}/${name}.png` });
}

/** Rest the pointer away from every trigger (two moves; see the registry spec). */
async function rest(page: Page) {
  await page.mouse.move(2, 2);
  await page.mouse.move(3, 3);
}

test("SV-20: the header reads project / title ● status, with only its right-side controls", async ({
  page,
}) => {
  const header = await openSession(page);

  const breadcrumb = header.getByRole("navigation", {
    name: "Session breadcrumb",
  });
  await expect(breadcrumb).toBeVisible();
  // The project leads, and opens the project.
  const crumb = breadcrumb.getByTestId("coding-session-project-crumb");
  await expect(crumb).toHaveText("buzz");
  await expect(crumb).toHaveAttribute("title", "Open buzz");
  await expect(breadcrumb.getByRole("heading")).toHaveText("Header parity");
  // The status word stays on screen beside the title.
  const status = breadcrumb.getByTestId("coding-session-status-badge");
  await expect(status).toBeVisible();
  await expect(status).toHaveAttribute("aria-label", /^Session status: /);
  const order = await breadcrumb.evaluate((nav) =>
    Array.from(nav.querySelectorAll("li")).map((item) =>
      (item.textContent ?? "").trim(),
    ),
  );
  expect(order[0]).toBe("buzz");
  expect(order[1]).toBe("/");
  expect(order[2]).toContain("Header parity");

  // Surfaces left the header for the launcher; Plan with them.
  await expect(
    header.locator('[data-testid^="coding-session-surface-toggle-"]'),
  ).toHaveCount(0);
  await expect(
    header.getByTestId("coding-session-task-rail-toggle"),
  ).toHaveCount(0);
  // The metadata line left the row for Details.
  await expect(header).not.toContainText("claude-opus-5");
  await expect(header).not.toContainText("generation 1");

  // Right side, in order: Details, ⋯, bottom panel, right panel.
  const ids = await header.evaluate((root) =>
    Array.from(root.querySelectorAll("[data-testid]"))
      .map((element) => element.getAttribute("data-testid"))
      .filter((id) =>
        [
          "coding-session-provenance-toggle",
          "coding-session-overflow",
          "coding-session-panel-toggle-bottom",
          "coding-session-panel-toggle-right",
        ].includes(id ?? ""),
      ),
  );
  expect(ids).toEqual([
    "coding-session-provenance-toggle",
    "coding-session-overflow",
    "coding-session-panel-toggle-bottom",
    "coding-session-panel-toggle-right",
  ]);

  await rest(page);
  await shoot(page, "SV20-header", header);
});

test("SV-20: ⌘J and ⌘⌥B toggle the panels, and the toggles show pressed", async ({
  page,
}) => {
  await openSession(page);
  const right = rightPanelToggle(page);
  const bottom = bottomPanelToggle(page);
  await expect(right).toHaveAttribute("aria-pressed", "false");
  await expect(bottom).toHaveAttribute("aria-pressed", "false");

  // ⌘⌥B: the right panel opens on the launcher, and the toggle is pressed.
  await page.keyboard.press("ControlOrMeta+Alt+KeyB");
  await expect(right).toHaveAttribute("aria-pressed", "true");
  await expect(
    page.getByTestId("coding-session-surface-launcher"),
  ).toBeVisible();
  await page.keyboard.press("ControlOrMeta+Alt+KeyB");
  await expect(right).toHaveAttribute("aria-pressed", "false");
  await expect(page.getByTestId("coding-session-surface-host")).toHaveCount(0);

  // ⌘J: the drawer. Where it cannot work here, the toggle is dimmed with the
  // drawer's own reason, but never refused: ⌘J and a click both open the
  // drawer to say it in full.
  const dimmed = await bottom.getAttribute("data-unavailable");
  if (dimmed === "true") {
    await expect(bottom).toHaveAttribute("aria-label", /unavailable: .+/);
  }
  await expect(bottom).not.toHaveAttribute("aria-disabled", "true");
  await page.keyboard.press("ControlOrMeta+KeyJ");
  await expect(bottom).toHaveAttribute("aria-pressed", "true");
  await expect(bottom).not.toHaveAttribute("data-unavailable", "true");
  await expect(page.getByTestId("coding-session-drawer-host")).toBeVisible();

  // Clicking works as well as the keys do.
  await right.click();
  await expect(right).toHaveAttribute("aria-pressed", "true");
  await rest(page);
  await shoot(
    page,
    "SV20-panel-toggles",
    page.getByTestId("coding-session-panel-toggles"),
  );

  await bottom.click();
  await expect(bottom).toHaveAttribute("aria-pressed", "false");
  await right.click();
  await expect(right).toHaveAttribute("aria-pressed", "false");
});

test("SV-20: the metadata line is the first rows of Details", async ({
  page,
}) => {
  await openSession(page);
  await page.getByTestId("coding-session-provenance-toggle").click();
  const rows = page.getByTestId("coding-session-details-metadata");
  await expect(rows).toBeVisible();
  await expect(
    rows.getByTestId("coding-session-details-meta-model"),
  ).toContainText("claude-opus-5");
  await expect(
    rows.getByTestId("coding-session-details-meta-generation"),
  ).toBeVisible();
  // The repository the session's metadata names, by its NIP-34 identifier.
  await expect(
    rows.getByTestId("coding-session-details-meta-repo"),
  ).toContainText("buzz");
  // First: ahead of People, continuity and provenance.
  const firstChild = await page
    .getByRole("dialog")
    .evaluate((dialog) =>
      dialog.querySelector("[data-testid]")?.getAttribute("data-testid"),
    );
  expect(firstChild).toBe("coding-session-details-metadata");
  await rest(page);
  await shoot(page, "SV20-details-metadata", page.getByRole("dialog"));
  await page.keyboard.press("Escape");
  await expect(rows).toHaveCount(0);
});

test("SV-20: a team session's header leads with its project, and Details names the focused seat's repo, runtime and model", async ({
  page,
}) => {
  await openObservedSession(page, {
    withObservations: false,
    refs: { projectRef: PROJECT_REF, repoRef: REPO_REF },
  });
  await expect(
    page.getByTestId("coding-session-umbrella-workspace"),
  ).toBeVisible({ timeout: 15_000 });
  const header = page.getByTestId("coding-session-header");
  const breadcrumb = header.getByRole("navigation", {
    name: "Session breadcrumb",
  });
  const crumb = breadcrumb.getByTestId("coding-session-project-crumb");
  await expect(crumb).toHaveText("buzz", { timeout: 15_000 });
  await expect(crumb).toHaveAttribute("title", "Open buzz");
  const order = await breadcrumb.evaluate((nav) =>
    Array.from(nav.querySelectorAll("li")).map((item) =>
      (item.textContent ?? "").trim(),
    ),
  );
  expect(order[0]).toBe("buzz");
  expect(order[1]).toBe("/");

  await page.getByTestId("coding-session-provenance-toggle").click();
  const rows = page.getByTestId("coding-session-details-metadata");
  await expect(rows).toBeVisible();
  await expect(
    rows.getByTestId("coding-session-details-meta-repo"),
  ).toContainText("buzz");
  await expect(
    rows.getByTestId("coding-session-details-meta-runtime"),
  ).toBeVisible();
  await expect(
    rows.getByTestId("coding-session-details-meta-model"),
  ).toBeVisible();
  await page.keyboard.press("Escape");
});

test("SV-20: a failed gate on the head dots the closed right panel's toggle", async ({
  page,
}) => {
  // A failed observed gate row, served through the header lane's bridge
  // module; Landing's own badge decides the tone, and the toggle reads it.
  await openObservedSession(page, {
    withObservations: true,
    refs: { projectRef: PROJECT_REF, repoRef: REPO_REF },
    foldThroughWaveBHeader: true,
  });
  const right = rightPanelToggle(page);
  await expect(right).toHaveAttribute("aria-pressed", "false", {
    timeout: 15_000,
  });
  const dot = page.getByTestId("coding-session-panel-toggle-right-dot");
  await expect(dot).toHaveAttribute("data-tone", "attention", {
    timeout: 15_000,
  });
  await expect(right).toHaveAttribute("aria-label", /not on screen: .*Landing/);

  // The tooltip lists what is off screen.
  await rest(page);
  await right.hover();
  const tooltip = page.getByTestId("coding-session-panel-toggle-right-tooltip");
  await expect(tooltip.first()).toBeVisible();
  await expect(tooltip.first()).toContainText("Landing");
  await waitForAnimations(page);
  const toggles = await page
    .getByTestId("coding-session-panel-toggles")
    .boundingBox();
  const tip = await tooltip.first().boundingBox();
  if (!toggles || !tip) throw new Error("toggle or tooltip geometry missing");
  const x = Math.min(toggles.x, tip.x) - 4;
  const y = Math.min(toggles.y, tip.y) - 4;
  await page.screenshot({
    clip: {
      x,
      y,
      width: Math.max(toggles.x + toggles.width, tip.x + tip.width) - x + 4,
      height: Math.max(toggles.y + toggles.height, tip.y + tip.height) - y + 4,
    },
    path: `${SHOTS}/SV20-attention-dot.png`,
  });

  // Open the panel: the launcher shows Landing's badge itself, so the dot
  // that stood in for it goes.
  await rest(page);
  await right.click();
  await expect(
    page.getByTestId("coding-session-surface-launcher"),
  ).toBeVisible();
  await expect(dot).toHaveCount(0);
});

test("SV-20 screenshots are hash-distinct", async () => {
  const hashes = new Map<string, string>();
  for (const name of SHOT_NAMES) {
    const png = await readFile(`${SHOTS}/${name}.png`);
    hashes.set(name, createHash("sha256").update(png).digest("hex"));
  }
  expect(new Set(hashes.values()).size).toBe(SHOT_NAMES.length);
});
