import { expect, test, type Page } from "@playwright/test";

import { installMockBridge } from "../helpers/bridge";
import {
  AGENT,
  captureDeclaredShot,
  CLOSED_SESSION_REF,
  declaredWorkFixture,
  GENERAL_CHANNEL_NAME,
  HUMAN,
  mockPulseNativeReads,
  OPEN_GENESIS_REF,
  OPEN_SESSION_REF,
  PROJECT_DTAG,
  projectHeadEvent,
  pulseMissionFixture,
  seededEntries,
  seededSessionFacts,
  setRootZoom,
  UNREADABLE_SESSION_REF,
} from "./helpers/pulseDeclaredWorkAssertions";

/**
 * Declared work on the Pulse screen, against the mock bridge.
 *
 * What this spec is for: two participants who never met — a human who posted a
 * plan and an agent an assignment named — are visible from the same records;
 * an assignment left unresolved when its session closed is still there, with
 * the closure as evidence rather than as an outcome; a settled one is behind a
 * disclosure instead of on top of the current work; and a read that lost a
 * session says so instead of rendering a quiet project.
 *
 * Stated limit: the bridge proves the surface against fixture bytes. It does
 * not establish cross-machine delivery or native responsiveness — those need
 * the two-machine runbook.
 */

async function openDeclaredPulse(page: Page): Promise<void> {
  // Before the bridge: both the trap and the seeded project events have to be
  // on `window` when `mockIPC` installs and the app mounts.
  await mockPulseNativeReads(page, {
    declaredWork: declaredWorkFixture(),
    missionRows: pulseMissionFixture(),
  });
  await page.addInitScript(
    (events) => {
      window.__BUZZ_E2E_EXTRA_PROJECT_EVENTS__ = events as never;
    },
    [projectHeadEvent(), ...seededEntries()] as never,
  );
  await installMockBridge(page);
  await page.goto("/", { waitUntil: "domcontentloaded" });
  await expect(page.getByTestId("project-group-general")).toBeVisible({
    timeout: 10_000,
  });
  await page.evaluate(
    ({ channelName, seeds }) => {
      const seed = window.__BUZZ_E2E_SEED_MOCK_SIGNED_EVENT__;
      if (!seed) throw new Error("mock signed-event seam is missing");
      for (const event of seeds as never[]) seed({ channelName, event });
    },
    { channelName: GENERAL_CHANNEL_NAME, seeds: seededSessionFacts() as never },
  );
  // The project is the *injected head*, not one created through the dialog:
  // a 30621 that names the seeded channel is what gives the Pulse read
  // somewhere to look, and creating a second one by hand would fork the dtag.
  const group = page.getByTestId(`project-group-${PROJECT_DTAG}`);
  await expect(group).toBeVisible({ timeout: 10_000 });
  await group.hover();
  await page.getByTestId(`project-open-${PROJECT_DTAG}`).click();
  await page.getByTestId("project-tab-pulse").click();
  await expect(page.getByTestId("project-pulse-screen")).toBeVisible({
    timeout: 10_000,
  });
  await expect(page.getByTestId("pulse-declared-work")).toBeVisible({
    timeout: 10_000,
  });
  await expect(page.getByTestId("pulse-declared-work")).toHaveAttribute(
    "data-state",
    "ready",
    { timeout: 10_000 },
  );
}

/** The declared-work row whose objective starts with this text. */
function declaredRow(page: Page, objective: string) {
  return page
    .getByTestId("pulse-declared-row")
    .filter({ hasText: objective })
    .first();
}

test("a human's plan and an agent's assignment are both visible, each labelled", async ({
  page,
}) => {
  await openDeclaredPulse(page);
  const section = page.getByTestId("pulse-declared-work");

  // The plan: its author is the owner, and it is the row `PulseEntryRow` draws.
  const plan = section.locator(
    '[data-testid="pulse-declared-row"][data-kind="plan"]',
  );
  await expect(plan).toHaveCount(1);
  await expect(plan.getByTestId("pulse-declared-label")).toHaveText(
    "Plan posted",
  );
  await expect(plan.getByTestId("pulse-entry-row")).toContainText(
    "Paging the declared-work read over visible sessions.",
  );
  await expect(plan.getByTestId("pulse-entry-row")).toContainText(
    HUMAN.username,
  );

  // The assignment: the assigned actor is responsible, the assigner inspectable.
  const assignment = declaredRow(page, "Build the declared-work wire");
  await expect(assignment).toHaveAttribute("data-kind", "assignment");
  await expect(assignment.getByTestId("pulse-declared-label")).toHaveText(
    "Assigned",
  );
  const responsible = assignment.getByTestId("pulse-declared-responsible");
  await expect(responsible).toContainText(
    `Assigned to ${AGENT.username} as builder`,
  );
  await expect(
    assignment.getByTestId("pulse-declared-assigner"),
  ).toHaveAttribute("title", HUMAN.pubkey);
  // The umbrella's own identifier reaches the disclosure, in full.
  await assignment
    .getByTestId("pulse-declared-details")
    .locator("summary")
    .click();
  await expect(assignment.getByTestId("pulse-declared-genesis")).toHaveText(
    `Genesis ${OPEN_GENESIS_REF}`,
  );

  // Two owners, two sessions, one section — and never the same row twice.
  // Four rows: the plan, two unsettled assignments, and one settled assignment
  // inside its collapsed group.
  await expect(
    section.locator('[data-testid="pulse-declared-row"]'),
  ).toHaveCount(4);
  await expect(
    page
      .getByTestId("pulse-declared-settled")
      .locator('[data-testid="pulse-declared-row"]'),
  ).toHaveCount(1);

  // The plan left the entries list rather than being drawn a second time.
  const entries = page.getByTestId("pulse-entries");
  await expect(entries.getByTestId("pulse-entry-row")).toHaveCount(1);
  await expect(entries.getByTestId("pulse-entry-row")).toContainText(
    "Picked up the surface lane.",
  );

  // Tall enough that the whole section is in the viewport: a scoped shot of an
  // element taller than the window comes back with the overflow blacked out.
  await page.setViewportSize({ width: 1280, height: 1600 });
  await captureDeclaredShot(page, section, "declared-work-1280");
});

test("closing a session settles nothing: the unresolved assignment stays current", async ({
  page,
}) => {
  await openDeclaredPulse(page);
  const row = declaredRow(page, "Finish the session read while the umbrella");
  await expect(row).toHaveAttribute("data-status", "unresolved");
  await expect(row).toHaveAttribute("data-session-key", CLOSED_SESSION_REF);
  await expect(row.getByTestId("pulse-declared-status")).toHaveText(
    "Unresolved",
  );
  await expect(row.getByTestId("pulse-declared-session")).toContainText(
    "Session closed",
  );
  // Current, not settled: the settled disclosure does not contain it.
  const settled = page.getByTestId("pulse-declared-settled");
  await expect(
    settled.locator('[data-testid="pulse-declared-row"]', {
      hasText: "Finish the session read",
    }),
  ).toHaveCount(0);
  await captureDeclaredShot(page, row, "declared-unresolved-in-closed-session");
});

test("a settled assignment is behind its own collapsed group", async ({
  page,
}) => {
  await openDeclaredPulse(page);
  const settled = page.getByTestId("pulse-declared-settled");
  await expect(settled).toBeVisible();
  // Collapsed: history is present without competing with current work.
  expect(
    await settled.evaluate((node) => (node as HTMLDetailsElement).open),
  ).toBe(false);
  // The group's own summary, not the per-row disclosure summaries inside it.
  await expect(settled.locator("> summary")).toHaveText("Settled (1)");
  await settled.locator("> summary").click();
  const row = settled.locator('[data-testid="pulse-declared-row"]');
  await expect(row).toHaveCount(1);
  await expect(row.getByTestId("pulse-declared-status")).toHaveText("Settled");
  await expect(
    row.getByTestId("pulse-declared-evidence").first(),
  ).toBeVisible();
  await captureDeclaredShot(page, settled, "declared-settled-group");
});

test("a read that lost a session discloses it and never says there is no work", async ({
  page,
}) => {
  await openDeclaredPulse(page);
  const limitations = page.getByTestId("pulse-declared-limitations");
  await expect(limitations).toBeVisible();
  await expect(limitations).toContainText("could not be read");
  await expect(page.getByTestId("pulse-declared-scan")).toContainText(
    "visible sessions",
  );
  await expect(page.getByTestId("pulse-declared-work")).not.toContainText(
    "No declared work",
  );
  // The unreadable session contributes a disclosure and no rows: a session
  // whose records could not be read must never be summarised as empty work.
  // Named the way a reader knows the session, not by its hex: the projection
  // labels it by name when it has one.
  await expect(limitations).toContainText("Unreadable records");
  await expect(
    page.locator(
      `[data-testid="pulse-declared-row"][data-session-key="${UNREADABLE_SESSION_REF}"]`,
    ),
  ).toHaveCount(0);
  await captureDeclaredShot(page, limitations, "declared-limitations");
});

test("Open session navigates to the execution and publishes nothing", async ({
  page,
}) => {
  await openDeclaredPulse(page);
  const row = declaredRow(page, "Build the declared-work wire");
  await expect(row).toHaveAttribute("data-session-key", OPEN_SESSION_REF);
  const before = await page.evaluate(() => ({
    commands: (window.__BUZZ_E2E_COMMANDS__ ?? []).length,
    signed: (window.__BUZZ_E2E_SIGNED_EVENTS__ ?? []).length,
  }));

  await row.getByTestId("pulse-declared-open-session").click();
  await expect(page).toHaveURL(/\/coding-sessions\//, { timeout: 10_000 });

  const after = await page.evaluate(
    (start) => ({
      commands: (window.__BUZZ_E2E_COMMANDS__ ?? []).slice(start.commands),
      signed: (window.__BUZZ_E2E_SIGNED_EVENTS__ ?? []).length,
    }),
    before,
  );
  // Reading a source is not scheduling an agent: no publish, no signature, no
  // turn. The bridge records every command it was asked to run.
  // An allowlist, not a denylist: a write the denylist's regex had not been
  // taught to recognise would have passed silently. Every command the bridge
  // recorded between the click and the route change has to be one of these
  // three reads — a huddle-state poll and the two deep-link queue drains the
  // shell runs on every navigation. Nothing publishes, signs or starts a turn.
  const READ_ONLY_ON_NAVIGATION = new Set([
    "get_huddle_state",
    "take_pending_entity_deep_link",
    "take_pending_navigation_deep_link",
  ]);
  const unexpected = [...new Set(after.commands)].filter(
    (command) => !READ_ONLY_ON_NAVIGATION.has(command),
  );
  expect(
    unexpected,
    `navigation invoked commands outside the read-only set: ${unexpected.join(", ")}`,
  ).toEqual([]);
  expect(after.signed).toBe(before.signed);
});

test("the section stays readable at 640px and at 250% zoom", async ({
  page,
}) => {
  await openDeclaredPulse(page);
  const section = page.getByTestId("pulse-declared-work");
  const row = declaredRow(page, "Build the declared-work wire");

  await page.setViewportSize({ width: 640, height: 1600 });
  await expect(row.getByTestId("pulse-declared-label")).toBeVisible();
  await expect(row.getByTestId("pulse-declared-objective")).toBeVisible();
  await expect(row.getByTestId("pulse-declared-responsible")).toBeVisible();
  await expect(row.getByTestId("pulse-declared-open-session")).toBeVisible();
  await captureDeclaredShot(page, section, "declared-work-640");

  // 250% type needs the room: the shot is of the whole section, not a slice.
  await page.setViewportSize({ width: 1280, height: 4000 });
  await setRootZoom(page, 250);
  // Zoom scales the type because the section is rem-only: the label, the
  // objective and the responsible line are all still there, wrapped.
  await expect(row.getByTestId("pulse-declared-label")).toBeVisible();
  await expect(row.getByTestId("pulse-declared-objective")).toBeVisible();
  await expect(row.getByTestId("pulse-declared-responsible")).toBeVisible();
  await expect(
    row
      .getByTestId("pulse-declared-recheck")
      .or(page.getByTestId("pulse-declared-recheck"))
      .first(),
  ).toBeVisible();
  await captureDeclaredShot(page, section, "declared-work-zoom-250");
  await setRootZoom(page, 100);
});
