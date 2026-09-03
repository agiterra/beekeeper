import { expect, test, type Page } from "@playwright/test";

import { installMockBridge } from "../helpers/bridge";
import {
  capturePulseMissionShot,
  missionRow,
  mockPulseMissionRows,
  pulseMissionFixture,
  pulseMissionNoIdentityFixture,
} from "./helpers/pulseMissionAssertions";

/**
 * Project Pulse missions, end to end against the frozen contract.
 *
 * The point of this spec is that the *whole* model reaches the screen: not the
 * mission's headline state, but the line under it that says who it is waiting
 * on; not "two seats", but each seat's gate rows, including the one that says
 * no gate row exists; not "some commits", but which ref moved and whether a
 * landing carried a verdict.
 *
 * The native command is answered with `pulseMissionResponse.fixture.json` —
 * the same bytes `crates/buzz-core/src/pulse_mission_tests.rs` pins — so a
 * sentence that changes in Rust changes here, and a reader who trusts this
 * screenshot is trusting the producer rather than this spec.
 */

const MISSION_RUNNING = "0f1e2d3c-4b5a-4978-8796-a5b4c3d2e1f0";
const MISSION_BLOCKED = "1a2b3c4d-5e6f-4071-8293-a4b5c6d7e8f9";
const MISSION_UNREADABLE = "2b3c4d5e-6f70-4182-93a4-b5c6d7e8f901";

async function openMissionsPulse(page: Page, payload = pulseMissionFixture()) {
  // Before the bridge: the trap has to be on `window` when `mockIPC` installs.
  await mockPulseMissionRows(page, payload);
  await installMockBridge(page);
  await page.goto("/", { waitUntil: "domcontentloaded" });
  await expect(page.getByTestId("project-group-general")).toBeVisible({
    timeout: 10_000,
  });
  await page.getByTestId("project-container-new").click();
  await page.getByTestId("create-project-container-name").fill("Missions Demo");
  await page.getByTestId("create-project-container-submit").click();
  const group = page.getByTestId("project-group-missions-demo");
  await expect(group).toBeVisible({ timeout: 10_000 });
  await group.hover();
  await page.getByTestId("project-open-missions-demo").click();
  await page.getByTestId("project-tab-pulse").click();
  await expect(page.getByTestId("project-pulse-screen")).toBeVisible({
    timeout: 10_000,
  });
  await expect(page.getByTestId("pulse-missions")).toBeVisible({
    timeout: 10_000,
  });
}

test("every row the mission fold produced reaches the Pulse screen", async ({
  page,
}) => {
  const fixture = pulseMissionFixture() as never as {
    missions: {
      sessionKey: string;
      lines: { id: string; text: string }[];
      seats: { lines: { id: string; text: string }[] }[];
      moved: { lines: { id: string; text: string }[] }[];
      timing: { id: string; text: string }[];
    }[];
    missionScope: string;
    missionErrors: { message: string }[];
    openRulings: { question: string | null }[];
    overlaps: { lines: { text: string }[] }[];
  };
  await openMissionsPulse(page);

  // ── What needs you ───────────────────────────────────────────────────────
  const rulings = page.getByTestId("pulse-rulings-card");
  await expect(rulings.getByTestId("pulse-rulings-waiting-heading")).toHaveText(
    "Waiting on you",
  );
  await expect(rulings.getByTestId("pulse-rulings-open-heading")).toHaveText(
    "Open rulings",
  );
  await expect(page.getByTestId("pulse-ruling-waiting")).toHaveCount(1);
  await expect(page.getByTestId("pulse-ruling-open")).toHaveCount(1);
  await expect(page.getByTestId("pulse-ruling-question")).toHaveText(
    fixture.openRulings[0].question as string,
  );
  await capturePulseMissionShot(page, rulings, "pulse-rulings-card");

  // ── The mission that is waiting on a person ──────────────────────────────
  const running = missionRow(page, MISSION_RUNNING);
  await expect(running).toHaveAttribute("data-mission-state", "running");
  const runningLines = running.getByTestId("pulse-mission-lines");
  for (const line of fixture.missions[0].lines) {
    await expect(
      runningLines.getByTestId(`pulse-mission-line-${line.id}`),
    ).toContainText(line.text);
  }
  await capturePulseMissionShot(page, runningLines, "pulse-mission-waiting");

  // ── Gates as observed, including the seat with none ──────────────────────
  const seats = running.getByTestId("pulse-mission-seats");
  await expect(seats.getByTestId("pulse-mission-seat")).toHaveCount(2);
  await expect(seats.getByTestId("pulse-mission-line-gate")).toHaveCount(2);
  // A claim in prose is not a gate row, and the screen says which one this is.
  await expect(
    seats.getByTestId("pulse-mission-line-gate-missing"),
  ).toContainText("No gate row on the wire");
  await expect(
    seats.getByTestId("pulse-mission-line-gate-truncated"),
  ).toBeVisible();
  await expect(seats.getByTestId("pulse-mission-line-owed")).toBeVisible();
  await capturePulseMissionShot(page, seats, "pulse-gates-observed");

  // ── What moved: a landing without a verdict, and a wip commit ────────────
  const moved = running.getByTestId("pulse-mission-moved-list");
  await expect(moved.getByTestId("pulse-mission-moved")).toHaveCount(2);
  await expect(
    moved.getByTestId("pulse-mission-line-moved").first(),
  ).toContainText("no verdict on the wire for this commit");
  await capturePulseMissionShot(page, moved, "pulse-what-moved");

  // ── A completion that was excluded is not a completed mission ────────────
  const blocked = missionRow(page, MISSION_BLOCKED);
  await expect(blocked).toHaveAttribute("data-mission-state", "blocked");
  await expect(
    blocked.getByTestId("pulse-mission-line-excluded-completion"),
  ).toContainText("this mission is not completed");
  await expect(
    blocked.getByTestId("pulse-mission-line-seat-claims-refused"),
  ).toBeVisible();
  await expect(
    blocked.getByTestId("pulse-mission-line-timing-missing"),
  ).toBeVisible();
  await capturePulseMissionShot(page, blocked, "pulse-mission-excluded");

  // ── The overlap: read by both sides, actionable by neither ───────────────
  const overlap = page.getByTestId("pulse-overlap-card");
  await expect(overlap.getByTestId("pulse-mission-line-overlap")).toContainText(
    fixture.overlaps[0].lines[0].text,
  );
  await expect(overlap.locator("button")).toHaveCount(0);
  await expect(overlap.locator("a")).toHaveCount(0);
  await capturePulseMissionShot(page, overlap, "pulse-overlap");

  // ── A session that could not be read, and the read's own losses ──────────
  await expect(
    missionRow(page, MISSION_UNREADABLE).getByTestId(
      "pulse-mission-line-unreadable",
    ),
  ).toContainText("could not be read");
  await expect(page.getByTestId("pulse-missions-scope")).toHaveText(
    fixture.missionScope,
  );
  await expect(page.getByTestId("pulse-mission-error")).toHaveCount(
    fixture.missionErrors.length,
  );
  await expect(page.getByTestId("pulse-mission-error").last()).toContainText(
    "the newest 8 by observation time were read",
  );
});

test("an unknown viewer is never rendered as nothing waiting on them", async ({
  page,
}) => {
  // The real no-identity response, generated by the Rust renderer. Nothing is
  // injected here: the sentence under test is the one the model composes.
  const payload = pulseMissionNoIdentityFixture();
  await openMissionsPulse(page, payload as never);

  await expect(page.getByTestId("pulse-rulings-card")).toHaveAttribute(
    "data-viewer-known",
    "no",
  );
  await expect(page.getByTestId("pulse-rulings-waiting")).toHaveCount(0);
  await expect(page.getByTestId("pulse-ruling-waiting")).toHaveCount(0);
  await expect(page.getByTestId("pulse-rulings-viewer-unknown")).toContainText(
    "No identity on this surface, so nothing here can be held on you",
  );
  // The other rulings are still there: not knowing the reader does not empty
  // the queue, it only means this client cannot say which of them is theirs.
  await expect(page.getByTestId("pulse-ruling-open")).toHaveCount(2);
});

test("a mission read that fails is disclosed, never rendered as a quiet project", async ({
  page,
}) => {
  const payload = pulseMissionFixture() as never as Record<string, unknown>;
  // One key short of the contract: exactly the shape a producer/reader
  // disagreement takes, and the one that must never render as "no missions".
  delete payload.overlaps;
  await mockPulseMissionRows(page, payload);
  await installMockBridge(page);
  await page.goto("/", { waitUntil: "domcontentloaded" });
  await expect(page.getByTestId("project-group-general")).toBeVisible({
    timeout: 10_000,
  });
  await page.getByTestId("project-container-new").click();
  await page.getByTestId("create-project-container-name").fill("Broken Demo");
  await page.getByTestId("create-project-container-submit").click();
  const group = page.getByTestId("project-group-broken-demo");
  await expect(group).toBeVisible({ timeout: 10_000 });
  await group.hover();
  await page.getByTestId("project-open-broken-demo").click();
  await page.getByTestId("project-tab-pulse").click();

  const unreadable = page.getByTestId("pulse-missions-unreadable");
  await expect(unreadable).toBeVisible({ timeout: 10_000 });
  await expect(unreadable).toContainText("overlaps");
  await expect(page.getByTestId("pulse-rulings-card")).toHaveCount(0);
  await expect(page.getByTestId("pulse-overlap-card")).toHaveCount(0);
});
