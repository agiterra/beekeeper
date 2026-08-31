import { expect, type Locator, type Page } from "@playwright/test";

import type { RelayEvent } from "@/shared/api/types";
import { waitForAnimations } from "../../helpers/animations";

type MissionLensHarness = {
  observedFile: string;
  openMockApp: (page: Page) => Promise<void>;
  screenshots: string;
  seedAndOpen: (page: Page) => Promise<void>;
};

async function elapsedSeconds(locator: Locator): Promise<number> {
  const text = (await locator.textContent()) ?? "";
  const match = text.match(/(\d+(?:\.\d+)?)s/);
  if (!match) throw new Error(`elapsed seconds missing from: ${text}`);
  return Number(match[1]);
}

export async function assertConversationAndMissionLenses(
  page: Page,
  harness: MissionLensHarness,
) {
  await harness.openMockApp(page);
  await harness.seedAndOpen(page);
  await expect
    .poll(() =>
      page.evaluate(() => getComputedStyle(document.documentElement).fontSize),
    )
    .toBe("20px");
  await expect(page.locator("html")).toHaveAttribute("data-buzz-theme", "buzz");

  const conversation = page.getByRole("button", {
    name: "Conversation lens",
  });
  const mission = page.getByRole("button", { name: "Mission lens" });
  await expect(conversation).toHaveAttribute("aria-pressed", "true");
  await expect(mission).toHaveAttribute("aria-pressed", "false");
  await expect(
    page.getByTestId("coding-session-disposition-strip"),
  ).toBeVisible();
  await expect(page.getByTestId("coding-session-participant-bar")).toHaveCount(
    0,
  );
  await expect(
    page.getByTestId("coding-session-umbrella-timeline"),
  ).toBeVisible();
  await expect(
    page.getByTestId("coding-session-umbrella-composer"),
  ).toBeVisible();
  await waitForAnimations(page);
  await page.getByTestId("coding-session-umbrella-workspace").screenshot({
    path: `${harness.screenshots}/conversation.png`,
  });

  await mission.focus();
  await page.keyboard.press("Enter");
  await expect(mission).toHaveAttribute("aria-pressed", "true");
  const inspector = page.getByTestId("coding-session-mission-inspector");
  await expect(inspector).toBeVisible({ timeout: 15_000 });
  await expect(inspector).toHaveAttribute("data-variant", "panel");
  await expect(inspector).not.toContainText("Loading signed Mission evidence");
  await expect(inspector).toContainText(
    "Ship a portable provider-neutral team loop.",
  );
  await expect(inspector).toContainText(
    "Mission inspector mounted with signed evidence.",
  );
  await expect(inspector).toContainText("Mission inspector smoke");
  await expect(inspector).toContainText(harness.observedFile);
  await expect(inspector).toContainText("Bob · Builder");
  await expect(inspector).toContainText("Parallax · Verifier");
  await waitForAnimations(page);
  await inspector.screenshot({
    path: `${harness.screenshots}/inspector-wide.png`,
  });
  await expect(page.getByTestId("coding-session-participant-chip")).toHaveCount(
    2,
  );
  await expect(
    page.getByTestId("coding-session-participant-bar"),
  ).toContainText("Bob · Builder");
  await expect(
    page.getByTestId("coding-session-participant-bar"),
  ).toContainText("Parallax · Verifier");
  const live = page.getByTestId("coding-session-live-activity-bar");
  await expect(live).toBeVisible();
  await expect(live).toContainText("Verify the signed live activity");
  await expect(live).toContainText("1 tool this turn");
  await expect(live).toContainText(/\d+(?:\.\d+)?s/);
  const firstElapsed = await elapsedSeconds(live);
  await expect
    .poll(() => elapsedSeconds(live), { timeout: 4_000 })
    .toBeGreaterThan(firstElapsed);
  await expect(
    page.getByTestId("coding-session-disposition-strip"),
  ).toHaveCount(0);
  await expect(page.getByTestId("coding-session-agent-focus")).toHaveCount(0);
  await expect(page.getByTestId("coding-session-active-work-dock")).toHaveCount(
    0,
  );
  const workspace = page.getByTestId("coding-session-umbrella-workspace");
  const briefDensity = page.getByTestId("coding-session-mission-density-brief");
  const liveDensity = page.getByTestId("coding-session-mission-density-live");
  const traceDensity = page.getByTestId("coding-session-mission-density-trace");
  await expect(liveDensity).toHaveAttribute("aria-pressed", "true");
  await waitForAnimations(page);
  await workspace.screenshot({
    path: `${harness.screenshots}/mission-live.png`,
  });

  await briefDensity.click();
  await expect(briefDensity).toHaveAttribute("aria-pressed", "true");
  await expect(
    page.getByTestId("coding-session-mission-trace-detail"),
  ).toHaveCount(0);
  await expect(
    page.getByTestId("coding-session-mission-transaction-card"),
  ).toContainText("Mission running");
  await expect(
    page.getByTestId("coding-session-mission-canonical-chain"),
  ).toContainText("report");
  await waitForAnimations(page);
  await workspace.screenshot({
    path: `${harness.screenshots}/mission-brief.png`,
  });

  await traceDensity.click();
  await expect(traceDensity).toHaveAttribute("aria-pressed", "true");
  await expect(
    page.getByTestId("coding-session-mission-trace-detail").first(),
  ).toBeVisible();
  await expect(
    page.getByTestId("coding-session-mission-trace-detail").first(),
  ).toContainText("Provider signer");
  await waitForAnimations(page);
  await workspace.screenshot({
    path: `${harness.screenshots}/mission-trace.png`,
  });

  const inspectorToggle = page.getByTestId(
    "coding-session-surface-toggle-mission-inspector",
  );
  await inspectorToggle.click();
  await expect(inspector).toBeVisible();
  await inspector
    .getByRole("button", { name: "Open Trace for source evidence" })
    .click();
  await expect(inspector).toHaveCount(0);
  await expect(traceDensity).toHaveAttribute("aria-pressed", "true");
  await inspectorToggle.focus();
  await page.keyboard.press("Enter");
  await expect(inspector).toBeVisible();
  await inspector
    .getByRole("button", { name: /Focus Parallax · Verifier/ })
    .click();
  await expect(
    page.getByTestId("coding-session-focused-agent-notice"),
  ).toContainText("Parallax");
  await waitForAnimations(page);
  await page.getByTestId("coding-session-umbrella-workspace").screenshot({
    path: `${harness.screenshots}/mission-focused-participant.png`,
  });

  await page.reload();
  await harness.seedAndOpen(page);
  await expect(
    page.getByRole("button", { name: "Mission lens" }),
  ).toHaveAttribute("aria-pressed", "true");
  await expect(
    page.getByTestId("coding-session-participant-bar"),
  ).toBeVisible();
  await expect(
    page.getByTestId("coding-session-mission-density-trace"),
  ).toHaveAttribute("aria-pressed", "true");

  const restoredConversation = page.getByRole("button", {
    name: "Conversation lens",
  });
  await restoredConversation.focus();
  await page.keyboard.press("Enter");
  await expect(restoredConversation).toHaveAttribute("aria-pressed", "true");
  await expect(page.getByTestId("coding-session-participant-bar")).toHaveCount(
    0,
  );
  await expect(
    page.getByTestId("coding-session-disposition-strip"),
  ).toBeVisible();
  await expect(
    page.getByTestId("coding-session-umbrella-timeline"),
  ).toContainText("The Mission hierarchy is sound.");
}

type MissionRecoveryHarness = {
  baseEvents: RelayEvent[];
  channelName: string;
  observedFile: string;
  awaiting: {
    events: RelayEvent[];
    foldResponse: Record<string, unknown>;
  };
  acknowledged: {
    events: RelayEvent[];
    foldResponse: Record<string, unknown>;
  };
  completed: {
    events: RelayEvent[];
    foldResponse: Record<string, unknown>;
  };
  openMockApp: (page: Page) => Promise<void>;
  seedAndOpen: (
    page: Page,
    governed?: MissionRecoveryHarness["completed"],
  ) => Promise<void>;
  transactionKind: number;
};

type GovernedMissionFixture = {
  events: RelayEvent[];
  foldResponse: Record<string, unknown>;
};

export function buildGovernedMissionApprovalPhases(
  completed: GovernedMissionFixture,
  transactionKind: number,
) {
  const transactionEvents = completed.events.filter(
    (event) => event.kind === transactionKind,
  );
  const assignment = transactionEvents.find(
    (event) => JSON.parse(event.content).type === "assignment",
  );
  const report = transactionEvents.find(
    (event) => JSON.parse(event.content).type === "report",
  );
  const disposition = transactionEvents.find(
    (event) => JSON.parse(event.content).type === "verdict",
  );
  const acknowledgement = transactionEvents.find(
    (event) => JSON.parse(event.content).type === "acknowledgement",
  );
  if (!assignment || !report || !disposition || !acknowledgement) {
    throw new Error("approval phase fixture is incomplete");
  }
  const lifecycle = completed.events.filter(
    (event) => event.kind !== transactionKind,
  );
  const response = (events: RelayEvent[], settled: boolean) => {
    const inputEventIds = events.map((event) => event.id).sort();
    return {
      ...completed.foldResponse,
      inputEventIds,
      includedEventIds: inputEventIds,
      assignments: [
        {
          assignmentEventId: assignment.id,
          governedReportEventId: settled ? report.id : null,
          dispositionEventId: settled ? disposition.id : null,
          acknowledgementEventId: settled ? acknowledgement.id : null,
          settled,
        },
      ],
      canonicalTerminal: null,
    };
  };
  const awaitingTransactions = [assignment, report, disposition];
  const acknowledgedTransactions = [...awaitingTransactions, acknowledgement];
  return {
    awaiting: {
      events: [...lifecycle, ...awaitingTransactions],
      foldResponse: response(awaitingTransactions, false),
    },
    acknowledged: {
      events: [...lifecycle, ...acknowledgedTransactions],
      foldResponse: response(acknowledgedTransactions, true),
    },
    completed,
  };
}

export async function assertMissionRestartRecovery(
  page: Page,
  harness: MissionRecoveryHarness,
) {
  await harness.openMockApp(page);
  await harness.seedAndOpen(page);
  await page.getByRole("button", { name: "Mission lens" }).click();
  const card = page.getByTestId("coding-session-mission-transaction-card");
  const inspector = page.getByTestId("coding-session-mission-inspector");
  await expect(card).toContainText("Mission running");
  await expect(card).toContainText("accepted report awaiting disposition");
  await expect(
    card.getByTestId("coding-session-mission-canonical-chain"),
  ).toContainText("assignment");
  await expect(
    card.getByTestId("coding-session-mission-canonical-chain"),
  ).toContainText("report");
  await expect(inspector).toContainText(
    "Mission inspector mounted with signed evidence.",
  );
  await expect(inspector).toContainText("Mount the signed Mission inspector.");
  await expect(inspector).toContainText(
    "Preserve Conversation and show only canonical Mission facts.",
  );
  await expect(inspector).toContainText("Run the real mock-bridge smoke test");
  await expect(inspector).toContainText(harness.observedFile);
  await expect(inspector).toContainText("Mission inspector smoke");

  await page.reload();
  await harness.seedAndOpen(page);
  await expect(
    page.getByRole("button", { name: "Mission lens" }),
  ).toHaveAttribute("aria-pressed", "true");
  await expect(card).toContainText("Mission running");
  await expect(card).toContainText("accepted report awaiting disposition");
  await expect(
    page.getByTestId("coding-session-mission-inspector"),
  ).toContainText("Mission inspector mounted with signed evidence.");

  const priorIds = new Set(harness.baseEvents.map((event) => event.id));
  await expect
    .poll(() =>
      page.evaluate(
        ({ channelName, transactionKind }) =>
          window.__BUZZ_E2E_HAS_MOCK_LIVE_SUBSCRIPTION__?.({
            channelName,
            kind: transactionKind,
          }) ?? false,
        {
          channelName: harness.channelName,
          transactionKind: harness.transactionKind,
        },
      ),
    )
    .toBe(true);
  const publishPhase = async (phase: {
    events: RelayEvent[];
    foldResponse: Record<string, unknown>;
  }) => {
    const events = phase.events.filter((event) => !priorIds.has(event.id));
    await page.evaluate(
      ({ channelName, events, response }) => {
        const setResponse = window.__BUZZ_E2E_SET_MISSION_FOLD_RESPONSE__;
        const seed = window.__BUZZ_E2E_SEED_MOCK_SIGNED_EVENT__;
        if (!setResponse || !seed) throw new Error("Mission E2E hooks missing");
        setResponse(response);
        for (const event of events) seed({ channelName, event });
      },
      {
        channelName: harness.channelName,
        events,
        response: phase.foldResponse,
      },
    );
    for (const event of events) priorIds.add(event.id);
  };

  await publishPhase(harness.awaiting);
  await expect(card).toContainText("Acknowledgement required", {
    timeout: 15_000,
  });
  await expect(card).toContainText("builder seat must acknowledge");
  await expect(
    card.getByTestId("coding-session-mission-canonical-chain"),
  ).toContainText("disposition");

  await publishPhase(harness.acknowledged);
  await expect(card).toContainText("Waiting on a person", {
    timeout: 15_000,
  });
  await expect(card).toContainText("Publish the signed follow-up note.");
  await expect(
    card.getByTestId("coding-session-mission-canonical-chain"),
  ).toContainText("acknowledgement");

  await publishPhase(harness.completed);
  await expect(card).toContainText("Mission completed", { timeout: 15_000 });
  await expect(card).toContainText("Decision: approve-with-notes");
  await expect(card).toContainText("Publish the signed follow-up note.");
  await expect(card).toContainText("Approval received.");
  await expect(
    card.getByTestId("coding-session-mission-canonical-chain"),
  ).toContainText("acknowledgement");
  await expect(inspector).toContainText(
    "Portable Mission evidence is complete.",
  );

  await page.reload();
  await page.evaluate((response) => {
    const setResponse = window.__BUZZ_E2E_SET_MISSION_FOLD_RESPONSE__;
    if (!setResponse) throw new Error("Mission fold hook missing");
    setResponse(response);
  }, harness.completed.foldResponse);
  await harness.seedAndOpen(page, harness.completed);
  await expect(card).toContainText("Mission completed", { timeout: 15_000 });
  await expect(card).toContainText("Decision: approve-with-notes");
  await expect(card).toContainText("Publish the signed follow-up note.");
  await expect(card).toContainText("Approval received.");
  await expect(inspector).toContainText(
    "Mission inspector mounted with signed evidence.",
  );
  await expect(inspector).toContainText("Mount the signed Mission inspector.");
  await expect(inspector).toContainText("Run the real mock-bridge smoke test");
  await expect(inspector).toContainText(harness.observedFile);
  await expect(inspector).toContainText("Mission inspector smoke");
}
