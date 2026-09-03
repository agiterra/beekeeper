import { expect, test, type Page } from "@playwright/test";

import { waitForAnimations } from "../helpers/animations";
import { openObservedSession } from "./helpers/codingSessionObservationAssertions";

/**
 * The Audit tab is the observer's screen (L5.2), and `Structured tests` reads
 * gate rows (L5.3).
 *
 * Live-run finding 26 is the reason both exist: a seat reported
 * `cargo test -p buzz-cli` green after running one test *file*, a verifier
 * reproduced red on the same patch, and nothing on any screen could say which
 * was right. The fixture puts both statements on the wire — the seat's
 * `declared` pass and the provider's `observed` fail — and this spec proves the
 * screen shows both, says which is which, and never merges them.
 */

const SHOTS = "../test-results/l5-shots";

async function openMissionSurface(page: Page, tab: "Audit" | "Inspector") {
  const workspace = page.getByTestId("coding-session-umbrella-workspace");
  await expect(workspace).toBeVisible({ timeout: 15_000 });
  await page.getByTestId("coding-session-lens-mission").click();
  // Mission opens its Inspector by default; the Audit tab is the third.
  await page
    .getByTestId(
      `coding-session-surface-tab-mission-${tab === "Audit" ? "audit" : "inspector"}`,
    )
    .click();
  await waitForAnimations(page);
}

test("the Audit tab renders this session's signed observations, per seat", async ({
  page,
}) => {
  await openObservedSession(page, { withObservations: true });
  await openMissionSurface(page, "Audit");

  const sections = page.getByTestId("coding-session-observations");
  await expect(sections).toBeVisible();

  // Two blocks: the seat that declared, and the mechanism that watched. A
  // 44246 names the key that signed it and has no field for the subject it
  // watched, so the watcher's block says so rather than claiming a seat.
  const blocks = page.getByTestId("coding-session-observation-seat");
  await expect(blocks).toHaveCount(2);
  await expect(sections).toContainText(
    "The record names the watcher, not the seat whose work it watched.",
  );

  // Both statements about `cargo test` are present, and neither replaced the
  // other.
  const rows = page.getByTestId("coding-session-gate-row");
  await expect(rows).toHaveCount(2);
  // Matched on the row's own outcome attribute, not on its text: a *passing*
  // `cargo test` tail reads "0 failed", so a text filter matches both rows.
  // The outcome is a property of the row, and that is what is asserted.
  const row = (outcome: string) =>
    page.locator(
      `[data-testid="coding-session-gate-row"][data-outcome="${outcome}"]`,
    );
  await expect(row("failed")).toHaveCount(1);
  await expect(row("passed")).toHaveCount(1);
  await expect(row("failed")).toHaveAttribute("data-source", "observed");
  await expect(row("passed")).toHaveAttribute("data-source", "declared");
  await expect(sections).toContainText("cargo test -p buzz-cli");

  // REVIEW-L5 F6: the whole observations block is taller than the viewport, so
  // an element shot of it ended mid-row and the rest came out black — cutting
  // off the observed `failed` row the artefact exists to show. Each seat block
  // is captured on its own instead, and the watcher's block (the one holding
  // that row) is the one this shot is named for.
  const watcher = blocks.filter({ hasText: "The record names the watcher" });
  await expect(watcher).toHaveCount(1);
  await expect(
    watcher.locator(
      '[data-testid="coding-session-gate-row"][data-outcome="failed"]',
    ),
  ).toHaveCount(1);
  await watcher.screenshot({ path: `${SHOTS}/audit-gates.png` });

  // The findings section, with the disposition word and the reference count.
  const finding = page.getByTestId("coding-session-finding-row").first();
  await expect(finding).toContainText("A3");
  await expect(finding).toContainText("fixed");
  await finding.screenshot({ path: `${SHOTS}/audit-findings.png` });

  // Phase timing: a duration list labelled with who reported it, and the
  // dangling pointer disclosed rather than dropped.
  const phase = page.getByTestId("coding-session-phase-row").first();
  await expect(phase).toContainText("green");
  await expect(phase).toContainText("3m");
  await expect(
    page.getByTestId("coding-session-observations-unresolved"),
  ).toContainText("which is not in this session's records");
  await phase.screenshot({ path: `${SHOTS}/audit-phases.png` });
});

test("Structured tests reads gate rows, and stops describing the wire", async ({
  page,
}) => {
  await openObservedSession(page, { withObservations: true });
  await openMissionSurface(page, "Inspector");

  const card = page.getByTestId("coding-session-inspector-gates");
  await expect(card).toBeVisible();
  await expect(card).toContainText("cargo test -p buzz-cli");
  // The sentence that was true when written and false the day 44246 landed.
  await expect(card).not.toContainText("Nothing on the wire reports tests");
  await expect(card).not.toContainText("will not count");
  await card.screenshot({ path: `${SHOTS}/tests-card-rows.png` });
});

test("with no gate row the card says so, rather than describing the wire", async ({
  page,
}) => {
  await openObservedSession(page, { withObservations: false });
  await openMissionSurface(page, "Inspector");

  const card = page.getByTestId("coding-session-inspector-gates");
  await expect(card).toContainText("No gate row yet");
  await expect(card).toContainText(
    "Kind 44246 carries a gate’s name, its outcome and the command that produced it.",
  );
  await expect(card).not.toContainText("Nothing on the wire reports tests");
  await card.screenshot({ path: `${SHOTS}/tests-card-empty.png` });
});

test("the Route rail signs for a gate row, and a failed one is attention", async ({
  page,
}) => {
  // L5.6. A sign exists only for a row the stream itself renders, and a gate
  // row is a signed event two surfaces render — so it earns one, in the row's
  // own word. Checkpoints, findings and phase timings get none.
  await openObservedSession(page, { withObservations: true });
  await expect(
    page.getByTestId("coding-session-umbrella-workspace"),
  ).toBeVisible({ timeout: 15_000 });
  await page.getByTestId("coding-session-lens-mission").click();
  await expect(
    page.getByTestId("coding-session-mission-inspector"),
  ).toBeVisible({ timeout: 15_000 });
  // §9.2's two gates, both of them: the rail needs the surface host closed and
  // a body wide enough to give up 224 px without narrowing the reading column.
  // The app's own chrome takes ~310 px, so 2000 is the first viewport that
  // does (`coding-session-mission-lens.spec.ts:1736-1741`).
  await page.getByTestId("coding-session-surface-close").click();
  await page.setViewportSize({ width: 2000, height: 1000 });

  const rail = page.getByTestId("coding-session-route-rail");
  await expect(rail).toBeVisible();
  const gateSigns = rail.locator('[data-kind="gate"]');
  await expect(gateSigns).toHaveCount(2);
  // The row's own outcome, never a second vocabulary for the same fact.
  await expect(gateSigns.filter({ hasText: "failed" })).toHaveCount(1);
  await expect(gateSigns.filter({ hasText: "passed" })).toHaveCount(1);
  await waitForAnimations(page);
  await rail.screenshot({ path: `${SHOTS}/rail-failed-gate.png` });
});
