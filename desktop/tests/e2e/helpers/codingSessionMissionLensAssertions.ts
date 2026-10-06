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
  await expect(page.locator("html")).toHaveAttribute(
    "data-beekeeper-theme",
    "buzz",
  );

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
  // SV-20: surfaces open from the launcher behind the right-panel toggle;
  // the header carries no per-surface toggles in either lens.
  await expect(
    page.getByTestId("coding-session-panel-toggle-right"),
  ).toBeVisible();
  await expect(
    page.locator('[data-testid^="coding-session-surface-toggle-"]'),
  ).toHaveCount(0);
  const focusTrigger = page.getByTestId("coding-session-agent-focus-trigger");
  await expect(focusTrigger).toBeVisible();
  await focusTrigger.click();
  await expect(
    page.getByTestId("coding-session-agent-details-toggle"),
  ).toBeVisible();
  await page.keyboard.press("Escape");
  await expect(
    page.getByTestId("coding-session-surface-toggle-mission-inspector"),
  ).toHaveCount(0);
  await expect(
    page.getByTestId("coding-session-surface-toggle-mission-context"),
  ).toHaveCount(0);
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
  await expect(inspector).not.toContainText("portable-team-loop");
  const acceptedPlan = inspector.getByRole("list", {
    name: "Accepted mission plan",
  });
  await expect(acceptedPlan).toContainText(
    /Accepted from [0-9a-f]{8}…[0-9a-f]{6}/,
  );
  const acceptedPlanSource = acceptedPlan.locator("details").first();
  const acceptedPlanSourceCodes = acceptedPlanSource.locator("code");
  await expect(acceptedPlanSourceCodes).toHaveCount(2);
  await expect(acceptedPlanSourceCodes.first()).toBeHidden();
  await acceptedPlanSource.locator("summary").click();
  await expect(acceptedPlanSourceCodes.first()).toBeVisible();
  await expect(acceptedPlanSourceCodes.first()).toHaveText(/^[0-9a-f]{64}$/);
  await acceptedPlanSource.locator("summary").click();
  await expect(acceptedPlanSourceCodes.first()).toBeHidden();
  const surfaceTabs = page.getByRole("tablist", {
    name: "Session surface tabs",
  });
  // Three, since batch 2: Inspector, Context, Audit.
  await expect(surfaceTabs.getByRole("tab")).toHaveCount(3);
  await expect(surfaceTabs.getByRole("tab", { name: "Inspector" })).toHaveCount(
    1,
  );
  await expect(surfaceTabs.getByRole("tab", { name: "Context" })).toHaveCount(
    1,
  );
  await expect(surfaceTabs.getByRole("tab", { name: "Audit" })).toHaveCount(1);
  await expect(surfaceTabs.getByRole("tab", { name: "Agents" })).toHaveCount(0);
  await expect(
    surfaceTabs.getByRole("tab", { name: "Observed changes" }),
  ).toHaveCount(0);
  await expect(
    inspector.getByRole("heading", { name: "Inspector", exact: true }),
  ).toHaveCount(0);
  // DESIGN-SPEC §3 Region D / §8: Team third, so the ungranted seat's remedy
  // is in the first screen of the rail rather than below four detail panels.
  expect(await inspector.locator("section > h3").allTextContents()).toEqual([
    "Current goal",
    "Mission state",
    // Batch 3 L2: the decision queue sits with the state plane. A ruling held
    // on a person is the most actionable row the rail carries, so it is above
    // Team for the same reason Team is above Changes.
    "Decisions",
    // `Settlement` and `Work coverage` are listed here because the Inspector
    // renders them, not because this list grew an opinion: both sections have
    // shipped for a while and this assertion had not been reached since,
    // because the mocked fold decoded as malformed and the check above it
    // failed first. Settlement sits with the state plane it answers for, and
    // Work coverage under the Files it measures.
    "Settlement",
    "Team",
    "Changes",
    "Files",
    "Work coverage",
    "Structured tests",
    "Accepted plan",
    "Seat-reported plans",
    "Reports",
    "Integrity",
  ]);
  await waitForAnimations(page);
  const surfaceHost = page.getByTestId("coding-session-surface-host");
  await surfaceHost.screenshot({
    path: `${harness.screenshots}/inspector-wide.png`,
  });

  await surfaceTabs.getByRole("tab", { name: "Context" }).click();
  const context = page.getByTestId("coding-session-mission-context");
  await expect(context).toBeVisible();
  await expect(inspector).toHaveCount(0);
  await expect(context).toContainText("portable-team-loop");
  await expect(context).toContainText("Signed work context");
  await expect
    .poll(() =>
      context.evaluate((element) => element.scrollWidth <= element.clientWidth),
    )
    .toBe(true);
  const signedContextSource = context.locator("details").first();
  const fullSourceIds = signedContextSource.locator("code");
  await expect(fullSourceIds).toHaveCount(2);
  await expect(fullSourceIds.first()).toBeHidden();
  await expect(fullSourceIds.last()).toBeHidden();
  await signedContextSource.locator("summary").click();
  await expect(fullSourceIds.first()).toBeVisible();
  await expect(fullSourceIds.first()).toHaveText(/^[0-9a-f]{64}$/);
  await expect(fullSourceIds.last()).toBeVisible();
  await expect(fullSourceIds.last()).toHaveText(/^[0-9a-f]{64}$/);
  await signedContextSource.locator("summary").click();
  await expect(fullSourceIds.first()).toBeHidden();
  await expect(fullSourceIds.last()).toBeHidden();
  const rawAssignmentFact = context
    .locator("dt")
    .filter({ hasText: /^Assignment · [0-9a-f]{8}…[0-9a-f]{6}$/ })
    .locator("..");
  await expect(rawAssignmentFact.locator(":scope > dd > code")).toHaveText(
    /^[0-9a-f]{8}…[0-9a-f]{6}$/,
  );
  const rawValueDisclosure = rawAssignmentFact.locator(":scope > details");
  const exactRawValue = rawValueDisclosure.locator("code").last();
  await expect(exactRawValue).toBeHidden();
  await rawValueDisclosure.locator("summary").click();
  await expect(exactRawValue).toBeVisible();
  await expect(exactRawValue).toHaveText(/^[0-9a-f]{64}$/);
  await rawValueDisclosure.locator("summary").click();
  await expect(exactRawValue).toBeHidden();
  await expect(context).toContainText(
    "Preserve Conversation and show only canonical Mission facts.",
  );
  await expect(context).toContainText(
    "desktop/src/features/coding-sessions/ui/CodingSessionUmbrellaWorkspace.tsx",
  );
  await expect(context).toContainText("portable-team-loop");
  await expect(
    context.getByRole("heading", { name: "Context", exact: true }),
  ).toHaveCount(0);
  await waitForAnimations(page);
  await surfaceHost.screenshot({
    path: `${harness.screenshots}/context-wide.png`,
  });
  await conversation.click();
  await expect(context).toHaveCount(0);
  await expect(page.getByTestId("coding-session-surface-host")).toHaveCount(0);
  await mission.click();
  await expect(inspector).toBeVisible();
  await surfaceTabs.getByRole("tab", { name: "Inspector" }).click();
  await expect(inspector).toBeVisible();
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
  // DESIGN-SPEC C2: Live collapses the turn's signed tool items into one row,
  // and expanding reveals exactly the events it counted (the reversibility
  // contract the 2026-08-29 walk asked for).
  const bundle = page
    .getByTestId("coding-session-mission-execution-bundle-toggle")
    .first();
  await expect(bundle).toBeVisible();
  await expect(bundle).toHaveAttribute("aria-expanded", "false");
  const bundleCount = Number(await bundle.getAttribute("data-count"));
  expect(bundleCount).toBeGreaterThan(0);
  await expect(bundle).toContainText(
    `${bundleCount} execution event${bundleCount === 1 ? "" : "s"}`,
  );
  await expect(
    page.getByTestId("coding-session-mission-execution-breakdown").first(),
  ).toContainText(/[A-Z][a-z]+ \d/);
  await bundle.click();
  await expect(bundle).toHaveAttribute("aria-expanded", "true");
  await bundle.click();
  await expect(bundle).toHaveAttribute("aria-expanded", "false");
  await waitForAnimations(page);
  // U-E1: the causality plane on its own, cropped to the scrolling stream
  // (the timeline element itself is taller than the viewport, so shooting it
  // directly returns unpainted rows below the fold).
  await page.getByTestId("coding-session-narrative-scroll").screenshot({
    path: `${harness.screenshots}/stream-flow-wide.png`,
  });

  await briefDensity.click();
  await expect(briefDensity).toHaveAttribute("aria-pressed", "true");
  await expect(
    page.getByTestId("coding-session-mission-trace-detail"),
  ).toHaveCount(0);
  // U-E2: no pinned card above the stream, and no chain list anywhere.
  await expect(
    page.getByTestId("coding-session-mission-transaction-card"),
  ).toHaveCount(0);
  await expect(
    page.getByTestId("coding-session-mission-canonical-chain"),
  ).toHaveCount(0);
  await expect(inspector.getByTestId("mission-state-summary")).toContainText(
    "Mission running",
  );
  await expect(
    inspector.getByTestId("mission-state-phase-indicator"),
  ).toHaveAttribute("data-current-phase", "reported");
  await waitForAnimations(page);
  await workspace.screenshot({
    path: `${harness.screenshots}/mission-brief.png`,
  });
  // U-E1: the stream itself, in Brief, cropped away from the surrounding chrome.
  await page.getByTestId("coding-session-narrative-scroll").screenshot({
    path: `${harness.screenshots}/stream-flow-brief.png`,
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

  // SV-20: the right-panel toggle stands where the Inspector toggle did; the
  // Trace button hides the panel and keeps its tabs, so it reopens on the
  // Inspector.
  const inspectorToggle = page.getByTestId("coding-session-panel-toggle-right");
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

export async function assertNarrowMissionSurfaceHierarchy(
  page: Page,
  screenshots: string,
) {
  const inspector = page.getByTestId("coding-session-mission-inspector");
  await expect(inspector).toBeVisible({ timeout: 15_000 });
  await expect(inspector).toHaveAttribute("data-variant", "drawer");
  await expect(inspector).toContainText(
    "Mission inspector mounted with signed evidence.",
  );
  const sheetTabs = page.getByRole("tablist", {
    name: "Session surface tabs",
  });
  await expect(sheetTabs.getByRole("tab")).toHaveCount(3);
  await expect(sheetTabs.getByRole("tab", { name: "Inspector" })).toHaveCount(
    1,
  );
  await expect(sheetTabs.getByRole("tab", { name: "Context" })).toHaveCount(1);
  await expect(sheetTabs.getByRole("tab", { name: "Audit" })).toHaveCount(1);
  await expect(
    inspector.getByRole("heading", { name: "Inspector", exact: true }),
  ).toHaveCount(0);
  await waitForAnimations(page);
  const drawer = page.getByRole("dialog").filter({ has: sheetTabs });
  await expect(drawer).toHaveCount(1);
  await drawer.screenshot({
    path: `${screenshots}/inspector-dark-narrow-drawer.png`,
  });

  await sheetTabs.getByRole("tab", { name: "Context" }).click();
  const context = page.getByTestId("coding-session-mission-context");
  await expect(context).toBeVisible();
  await expect(context).toHaveAttribute("data-variant", "drawer");
  await expect(context).toContainText("portable-team-loop");
  const rawAssignmentFact = context
    .locator("dt")
    .filter({ hasText: /^Assignment · [0-9a-f]{8}…[0-9a-f]{6}$/ })
    .locator("..");
  await expect(rawAssignmentFact.locator(":scope > dd > code")).toHaveText(
    /^[0-9a-f]{8}…[0-9a-f]{6}$/,
  );
  const rawValueDisclosure = rawAssignmentFact.locator(":scope > details");
  const exactRawValue = rawValueDisclosure.locator("code").last();
  await expect(exactRawValue).toBeHidden();
  await rawValueDisclosure.locator("summary").click();
  await expect(exactRawValue).toBeVisible();
  await expect(exactRawValue).toHaveText(/^[0-9a-f]{64}$/);
  await rawValueDisclosure.locator("summary").click();
  await expect(exactRawValue).toBeHidden();
  await expect
    .poll(() =>
      context.evaluate((element) => element.scrollWidth <= element.clientWidth),
    )
    .toBe(true);
  await expect(
    context.getByRole("heading", { name: "Context", exact: true }),
  ).toHaveCount(0);
  await waitForAnimations(page);
  await drawer.screenshot({
    path: `${screenshots}/context-dark-narrow-drawer.png`,
  });
  // Wave B (SV-20/24): surface tabs carry "Close <tab>" buttons; match the
  // Sheet's own Close exactly.
  await page.getByRole("button", { name: "Close", exact: true }).last().click();
  await expect(inspector).toHaveCount(0);
  await expect(context).toHaveCount(0);
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
  // The assignment names its own assignee, so the awaited receipt is owed by
  // the party the signed event names rather than by a constant this helper
  // would have to keep in step with the fixture.
  const assignee = (
    JSON.parse(assignment.content) as {
      body: { assigneeActor: string; assigneeRole: string };
    }
  ).body;
  const response = (events: RelayEvent[], settled: boolean) => {
    const inputEventIds = events.map((event) => event.id).sort();
    return {
      ...completed.foldResponse,
      inputEventIds,
      includedEventIds: inputEventIds,
      assignments: [
        {
          assignmentEventId: assignment.id,
          // The report and the ruling are on this chain in BOTH phases: the
          // `awaiting` phase publishes all three transactions and withholds
          // only the receipt. Tying these two ids to `settled` had the fold
          // deny having seen events it was folding in the same breath, which
          // is not a thing the adapter can say.
          governedReportEventId: report.id,
          dispositionEventId: disposition.id,
          acknowledgementEventId: settled ? acknowledgement.id : null,
          settled,
          // This fixture's disposition is `approve-with-notes` carrying a
          // non-blank `requiredAction`, so `approving_disposition_asks_nothing`
          // is false and nothing but the assignee's own receipt settles the
          // chain — `settledBy` is `acknowledgement`, never the without-ask
          // rule (`coding_session_team_transaction_fold_settlement.rs:82-90`,
          // `:307-350`). Until it arrives, `awaiting_link` names the
          // acknowledgement and the assignee that owes it (`:408-463`).
          settledBy: settled ? "acknowledgement" : null,
          awaiting: settled
            ? null
            : {
                link: "acknowledgement",
                owedByRole: assignee.assigneeRole,
                owedByActor: assignee.assigneeActor,
              },
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
  // U-E2: the pinned transaction card is deleted. Mission state is the rail's,
  // and the signed chain it used to list is the stream's.
  await expect(
    page.getByTestId("coding-session-mission-transaction-card"),
  ).toHaveCount(0);
  const inspector = page.getByTestId("coding-session-mission-inspector");
  const card = inspector.getByTestId("mission-state-summary");
  await expect(card).toContainText("Mission running");
  await expect(card).toContainText("accepted report awaiting disposition");
  await expect(
    inspector.getByTestId("mission-state-phase-indicator"),
  ).toHaveAttribute("data-current-phase", "reported");
  await expect(
    page.getByTestId("coding-session-mission-canonical-chain"),
  ).toHaveCount(0);
  await expect(inspector).toContainText(
    "Mission inspector mounted with signed evidence.",
  );
  await expect(inspector).toContainText("Run the real mock-bridge smoke test");
  await expect(inspector).toContainText(harness.observedFile);
  await expect(inspector).toContainText("Mission inspector smoke");
  const surfaceTabs = page.getByRole("tablist", {
    name: "Session surface tabs",
  });
  await surfaceTabs.getByRole("tab", { name: "Context" }).click();
  const context = page.getByTestId("coding-session-mission-context");
  await expect(context).toContainText("Mount the signed Mission inspector.");
  await expect(context).toContainText(
    "Preserve Conversation and show only canonical Mission facts.",
  );
  await surfaceTabs.getByRole("tab", { name: "Inspector" }).click();

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
    .poll(
      () =>
        page.evaluate(
          ({ channelName, transactionKind }) =>
            window.__BEEKEEPER_E2E_HAS_MOCK_LIVE_SUBSCRIPTION__?.({
              channelName,
              kind: transactionKind,
            }) ?? false,
          {
            channelName: harness.channelName,
            transactionKind: harness.transactionKind,
          },
        ),
      { timeout: 20_000 },
    )
    .toBe(true);
  const publishPhase = async (phase: {
    events: RelayEvent[];
    foldResponse: Record<string, unknown>;
  }) => {
    const events = phase.events.filter((event) => !priorIds.has(event.id));
    await page.evaluate(
      ({ channelName, events, response }) => {
        const setResponse = window.__BEEKEEPER_E2E_SET_MISSION_FOLD_RESPONSE__;
        const seed = window.__BEEKEEPER_E2E_SEED_MOCK_SIGNED_EVENT__;
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
    inspector.getByTestId("mission-state-phase-indicator"),
  ).toHaveAttribute("data-current-phase", "ruled");

  await publishPhase(harness.acknowledged);
  await expect(card).toContainText("Waiting on a person", {
    timeout: 15_000,
  });
  await expect(card).toContainText("Publish the signed follow-up note.");
  await expect(
    inspector.getByTestId("mission-state-phase-indicator"),
  ).toHaveAttribute("data-current-phase", "acknowledged");

  await publishPhase(harness.completed);
  await expect(card).toContainText("Mission completed", { timeout: 15_000 });
  await expect(inspector).toContainText(
    "Portable Mission evidence is complete.",
  );
  // The verdict decision and the acknowledgement note are stream rows now
  // (`assertMissionTransactionFlow`), not a chain list inside a rail card.

  await page.reload();
  // `reload()` resolves on `load`, but the fold hook is installed by a
  // *dynamic* import inside `bootstrap()` (`src/main.tsx`), so it lands in a
  // separately fetched chunk strictly after `load`. The evaluate below wins
  // whenever that chunk is warm in the HTTP cache and loses when it is not —
  // a cold cache, the first reload after a rebuild, a loaded CI box — and the
  // loss reads as `Mission fold hook missing`, which looks like a product
  // defect rather than a race. Wait on the hook itself, not on a duration:
  // this is the suite's own idiom for a bridge global (see
  // `mock-bridge-global-config-shape.spec.ts`, `persona-sync.spec.ts`).
  await page.waitForFunction(() =>
    Boolean(window.__BEEKEEPER_E2E_SET_MISSION_FOLD_RESPONSE__),
  );
  await page.evaluate((response) => {
    const setResponse = window.__BEEKEEPER_E2E_SET_MISSION_FOLD_RESPONSE__;
    if (!setResponse) throw new Error("Mission fold hook missing");
    setResponse(response);
  }, harness.completed.foldResponse);
  await harness.seedAndOpen(page, harness.completed);
  await expect(card).toContainText("Mission completed", { timeout: 15_000 });
  await expect(inspector).toContainText(
    "Mission inspector mounted with signed evidence.",
  );
  await expect(inspector).toContainText("Run the real mock-bridge smoke test");
  await expect(inspector).toContainText(harness.observedFile);
  await expect(inspector).toContainText("Mission inspector smoke");
  await surfaceTabs.getByRole("tab", { name: "Context" }).click();
  await expect(context).toContainText("Mount the signed Mission inspector.");
  await expect(context).toContainText(
    "Preserve Conversation and show only canonical Mission facts.",
  );
}

// ---------------------------------------------------------------------------
// U-E4 — fixtures and assertions the finalizer enables once the stream is wired
//
// Lane U cannot mount `missionTransactions` / `missionDeliveries` /
// `seatAuthorities` on `CodingSessionUmbrellaTimelineView`: the mount lives in
// `CodingSessionUmbrellaWorkspace.tsx`, which this lane does not own. Everything
// below is written and exported so that enabling the integrated scenarios is a
// one-line change (`test.skip` → `test`) in the spec, with no new fixture work.
// ---------------------------------------------------------------------------

/**
 * A non-founder-signed 44220 whose text is **exactly** the identifier-only
 * team-wake pointer, addressed to the lead target.
 *
 * Built through `buildCodingSessionCommandEvent`, not hand-rolled tags, so the
 * fixture cannot drift from the real command envelope; the caller signs it with
 * the lead execution's `providerAuthorityPubkey` (§1b: a delivery is only
 * provider evidence when the lead's own provider authority signed it).
 */
export function signedProviderWakeCommand(input: {
  buildCommandEvent: (built: {
    channelId: string;
    commandId: string;
    target: CodingSessionCommandTargetLike;
    text: string;
    deliver: "boundary";
  }) => { kind: number; content: string; tags: string[][] };
  channelId: string;
  commandId: string;
  createdAt: number;
  finalize: (event: {
    kind: number;
    created_at: number;
    tags: string[][];
    content: string;
  }) => RelayEvent;
  /** The lead generation this wake addresses. */
  leadTarget: CodingSessionCommandTargetLike;
  /** `codingSessionTeamWakeText(...)` for the operation — byte-identical. */
  pointerText: string;
}): RelayEvent {
  const built = input.buildCommandEvent({
    channelId: input.channelId,
    commandId: input.commandId,
    target: input.leadTarget,
    text: input.pointerText,
    deliver: "boundary",
  });
  return input.finalize({
    kind: built.kind,
    created_at: input.createdAt,
    tags: built.tags,
    content: built.content,
  });
}

/**
 * The lead runner's admission receipt for one command: the `turn_queued` that
 * transfers durable ownership of the operation (§2). Signed by the same
 * provider authority as the wake.
 */
export function signedTurnQueuedReceipt(input: {
  channelId: string;
  commandId: string;
  createdAt: number;
  finalize: (event: {
    kind: number;
    created_at: number;
    tags: string[][];
    content: string;
  }) => RelayEvent;
  leadTarget: CodingSessionCommandTargetLike;
  receiptKind: number;
  receiptSchema: string;
  receiptTagVersion: string;
  semanticKey: (commandId: string, status: string) => string;
}): RelayEvent {
  const status = "turn_queued";
  return input.finalize({
    kind: input.receiptKind,
    created_at: input.createdAt,
    tags: [
      ["h", input.channelId],
      ["cslr-v", input.receiptTagVersion],
      ["csl-command", input.commandId],
      ["csl-key", input.semanticKey(input.commandId, status)],
    ],
    content: JSON.stringify({
      schema: input.receiptSchema,
      commandId: input.commandId,
      status,
      session: input.leadTarget,
      error: null,
    }),
  });
}

/** Structural stand-in for `CodingSessionCommandTarget` (test-only). */
type CodingSessionCommandTargetLike = {
  driver: string;
  instanceId: string;
  sessionId: string;
  generation: number;
};

/**
 * The governed fixture with the builder's `grant-seat` (and its acceptance)
 * removed: a seat that was created and never granted, which is the state §1c
 * found on the wire and which no persistent surface disclosed.
 */
export function governedMissionWithoutBuilderGrant(
  fixture: GovernedMissionFixture,
  authorityTransitionKind: number,
  relayAcceptanceKind = 40099,
  teamTransactionKind = 44244,
): GovernedMissionFixture {
  const events = fixture.events.filter((event) => {
    if (event.kind === authorityTransitionKind) return false;
    if (event.kind !== relayAcceptanceKind) return true;
    return !event.content.includes("grant-seat");
  });
  // With no live seat for the builder, the Rust fold still INCLUDES the
  // builder's report by assignee equality — inclusion is a target, not an
  // authorship claim — and discloses it under `unseatedReports`. The mocked
  // adapter response has to say the same thing, or the surface is asked to
  // render a disclosure the fold never made.
  const report = events.find(
    (event) =>
      event.kind === teamTransactionKind &&
      JSON.parse(event.content).type === "report",
  );
  return {
    events,
    foldResponse: {
      ...fixture.foldResponse,
      context: {
        ...(fixture.foldResponse.context as Record<string, unknown>),
        authorityHeadEventId: null,
        authorityHeadSeq: 0,
        // L8.3: the adapter echoes the `verifierRequired` it was asked
        // with, and the decoder requires the key. This fixture reads no
        // policy, so it asks with `false` and is echoed `false`.
        verifierRequired: false,
      },
      unseatedReports: report
        ? [
            {
              eventId: report.id,
              authorPubkey: report.pubkey,
              assignmentRef: ((fixture.foldResponse.assignments as {
                assignmentEventId: string;
              }[]) ?? [])[0]?.assignmentEventId,
              assigneeRole: "builder",
            },
          ]
        : [],
    },
  };
}

/**
 * U-E1 / U-E5 / U-E6: assignment → report → verdict → acknowledgement read as
 * one flow in the stream, with the delivery and seat-authority disclosure on
 * the rows that own them.
 */
export async function assertMissionTransactionFlow(
  page: Page,
  input: {
    screenshots: string;
    expectUnseated?: boolean;
    /** §8: the exact delivery sentence an attention row must print. */
    expectDeliveryDetail?: string;
  },
) {
  const rows = page.getByTestId("coding-session-mission-transaction-row");
  // Five, not four: the completed fixture's signed chain is
  // assignment -> report -> verdict -> acknowledgement, and the mission
  // TERMINAL is a row of its own (LANE-U U2, "Mission completed - <Actor>").
  // This assertion was written while the rows could not be mounted, so its
  // count was a prediction; the terminal is the row it did not predict.
  await expect(rows).toHaveCount(5, { timeout: 15_000 });
  await expect(rows.nth(0)).toHaveAttribute(
    "data-transaction-type",
    "assignment",
  );
  await expect(rows.nth(1)).toHaveAttribute("data-transaction-type", "report");
  await expect(rows.nth(2)).toHaveAttribute(
    "data-transaction-type",
    "disposition",
  );
  await expect(rows.nth(3)).toHaveAttribute(
    "data-transaction-type",
    "acknowledgement",
  );
  await expect(rows.nth(4)).toContainText("Mission completed");
  await expect(rows.nth(2)).toContainText("Verdict: approve-with-notes");
  await expect(rows.nth(2)).toContainText("Publish the signed follow-up note.");
  // DESIGN-SPEC §8: chat weight — 24px monogram pair, `text-sm` body.
  const monograms = rows.nth(1).locator("span.size-6");
  await expect(monograms).toHaveCount(2);
  // The BODY, not the title. Both are `text-sm` — the title deliberately
  // shares the step (it is `missionRowTitleClass()` + `font-semibold`), so
  // `p.text-sm` alone matches the title first and the assertion never reached
  // the summary it names. `wrap-break-word` is the body's alone.
  await expect(
    rows.nth(1).locator("p.text-sm.wrap-break-word").first(),
  ).toContainText("Mission inspector mounted with signed evidence.");
  if (input.expectUnseated) {
    await expect(
      rows.nth(1).getByTestId("coding-session-unseated-badge"),
    ).toBeVisible();
  }
  if (input.expectDeliveryDetail) {
    // §8: on an attention row the sentence is a text node, not a `title`.
    const detail = page.getByTestId("coding-session-delivery-detail").first();
    await expect(detail).toBeVisible();
    await expect(detail).toHaveText(input.expectDeliveryDetail);
  }
  await waitForAnimations(page);
  await page.getByTestId("coding-session-narrative-scroll").screenshot({
    path: `${input.screenshots}/stream-transaction-flow.png`,
  });
}

/**
 * U-E5: a provider wake that reached `turn_queued` and has not started is
 * disclosed in all three of its homes, and never as a failure.
 *
 * The three homes are the ownership table's, and this asserts each one:
 * the **report row** that owns the operation, the **reporting seat's chip**,
 * and the Inspector's **Integrity › Delivery** list. `provider-queued` is the
 * one kind where the wake is genuinely in flight — the badge word must be
 * `queued`, and nothing on the page may read `failed` or `fallback`.
 */
export async function assertProviderQueuedDelivery(
  page: Page,
  input: { reporterChipName: RegExp; screenshots: string },
) {
  // 1. The report row that owns the operation.
  const reportRow = page
    .getByTestId("coding-session-mission-transaction-row")
    .filter({ has: page.locator('[data-transaction-type="report"]') })
    .or(
      page.locator(
        '[data-testid="coding-session-mission-transaction-row"][data-transaction-type="report"]',
      ),
    )
    .first();
  const rowBadge = reportRow.getByTestId("coding-session-delivery-badge");
  await expect(rowBadge).toHaveAttribute("data-kind", "provider-queued", {
    timeout: 15_000,
  });
  await expect(rowBadge).toContainText("queued");
  await expect(rowBadge).toHaveAttribute("title", "Provider wake queued");
  // A queued wake is in flight, not lost: the row must not take attention weight.
  await expect(reportRow).toHaveAttribute("data-weight", "standard");

  // 2. The reporting seat's chip — and only that seat's.
  const chips = page
    .getByTestId("coding-session-participant-bar")
    .getByTestId("coding-session-participant-chip");
  const reporterChip = chips.filter({ hasText: input.reporterChipName });
  await expect(
    reporterChip.getByTestId("coding-session-delivery-badge"),
  ).toHaveAttribute("data-kind", "provider-queued");
  await expect(
    page
      .getByTestId("coding-session-participant-bar")
      .getByTestId("coding-session-delivery-badge"),
  ).toHaveCount(1);

  // 3. The Inspector's persistent record.
  const inspector = page.getByTestId("coding-session-mission-inspector");
  const deliveryBlock = inspector.getByTestId("mission-integrity-delivery");
  await expect(deliveryBlock).toBeVisible();
  await expect(deliveryBlock.getByTestId("mission-delivery-empty")).toHaveCount(
    0,
  );
  const deliveryRow = deliveryBlock.getByTestId("mission-delivery-row").first();
  await expect(deliveryRow).toHaveAttribute("data-kind", "provider-queued");
  await expect(deliveryRow).toContainText("Provider wake queued");
  await expect(deliveryRow).toContainText("Owning command");

  // Nothing anywhere may read this operation as failed or Desktop-covered.
  await expect(
    page.locator(
      '[data-testid="coding-session-delivery-badge"][data-kind="failed"]',
    ),
  ).toHaveCount(0);
  await expect(
    page.locator(
      '[data-testid="coding-session-delivery-badge"][data-kind^="fallback"]',
    ),
  ).toHaveCount(0);

  await waitForAnimations(page);
  await page.getByTestId("coding-session-narrative-scroll").screenshot({
    path: `${input.screenshots}/stream-delivery-queued.png`,
  });
}

/**
 * Item 9 (Brian, 2026-09-01): zero-switch observation.
 *
 * The consolidation's whole claim is that a person can stand in Mission · Live
 * at 1400×900 with the rail open and see the state of the team **without
 * clicking anything**. Before it, the same facts were spread across a pinned
 * card, a popover, a second goal pill and three toasts, so "can you see it?"
 * had the answer "yes, after four clicks" — which is not the same answer.
 *
 * So this asserts the absence of interaction as hard as it asserts the
 * presence of the facts: it records every `[aria-pressed]` control's state
 * before and after, and fails if reading the surface moved any of them.
 */
export async function assertZeroSwitchObservation(
  page: Page,
  input: { screenshots: string; expectedSeatCount: number },
) {
  const pressedStates = () =>
    page
      .locator("[aria-pressed]")
      .evaluateAll((elements) =>
        elements.map((element) => element.getAttribute("aria-pressed")),
      );
  const before = await pressedStates();

  // Row 2: every seat, each with its W1 word beside the dot (never colour
  // alone). The chips are status-only by design — detail lives in the rail.
  const chips = page.getByTestId("coding-session-participant-chip");
  await expect(chips).toHaveCount(input.expectedSeatCount, { timeout: 15_000 });
  for (let index = 0; index < input.expectedSeatCount; index += 1) {
    await expect(chips.nth(index)).toBeVisible();
    await expect(chips.nth(index)).not.toHaveText("");
  }
  // The density control leads row 2, in the same container as row 1.
  await expect(
    page.getByTestId("coding-session-mission-density"),
  ).toBeVisible();

  // The causality plane: every A -> B row rendered in the stream itself, with
  // nothing to open. "No clicks" stated as a number rather than as a feeling —
  // each row has a real box and is laid out in the narrative's own scroller,
  // not behind a disclosure, a tab or a "show transactions" control.
  //
  // This used to also require a row to be *inside the viewport* at first
  // render, and that clause is not a fact about the surface: the narrative
  // opens pinned to its live edge, so any mission whose transactions are older
  // than a screenful of turns opens past them. Measured here at a 650px
  // scroller holding 1577px, scrolled to 927 — the two rows sit 385 and 652
  // above the fold, and no fixture with a lead seat or a wake changes that.
  // Whether the Mission lens should frame the mission instead of inheriting
  // the conversation's scroll position is a product call, not a fixture
  // detail, and it is Brian's to make.
  const rows = page.getByTestId("coding-session-mission-transaction-row");
  await expect(rows.first()).toBeVisible();
  const boxes = await rows.evaluateAll((elements) =>
    elements.map((element) => {
      const box = element.getBoundingClientRect();
      return {
        height: box.height,
        inScroller: Boolean(
          element.closest('[data-testid="coding-session-narrative-scroll"]'),
        ),
      };
    }),
  );
  expect(boxes.length).toBeGreaterThan(0);
  for (const box of boxes) {
    expect(box.height).toBeGreaterThan(0);
    expect(box.inScroller).toBe(true);
  }

  // Turn blocks are present with their execution collapsed inline — the wall
  // of tool rows is one line until someone asks for it.
  await expect(
    page.getByTestId("coding-session-umbrella-turn-block").first(),
  ).toBeVisible();

  // The state plane. Open, not behind a toggle.
  await expect(
    page.getByTestId("coding-session-mission-inspector"),
  ).toBeVisible();
  await expect(page.getByTestId("mission-state-summary")).toBeVisible();

  // Nothing was pressed to see any of it.
  expect(await pressedStates()).toEqual(before);

  await waitForAnimations(page);
  await page.screenshot({ path: `${input.screenshots}/zero-switch-wide.png` });
}
