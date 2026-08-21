import { createHash } from "node:crypto";

import { expect, test, type Locator, type Page } from "@playwright/test";
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
  lifecycleReceiptSemanticKey,
} from "@/features/coding-sessions/lib/codingSessionIngressPayloads";
import { buildCodingSessionCreateEvent } from "@/features/coding-sessions/lib/codingSessionLifecycleCommand";
import {
  BUZZ_CODING_SESSION_TRANSCRIPT_SCHEMA,
  CODING_SESSION_TRANSCRIPT_TAG_VERSION,
  codingSessionTranscriptSemanticKey,
} from "@/features/coding-sessions/lib/codingSessionTranscriptPresentation";
import type { RelayEvent } from "@/shared/api/types";
import {
  KIND_CODING_SESSION_CLOSURE,
  KIND_CODING_SESSION_LEASE,
  KIND_CODING_SESSION_LIFECYCLE_COMMAND,
  KIND_CODING_SESSION_LIFECYCLE_RECEIPT,
  KIND_CODING_SESSION_METADATA,
  KIND_CODING_SESSION_NAME,
  KIND_CODING_SESSION_TRANSCRIPT,
} from "@/shared/constants/kinds";

import { waitForAnimations } from "../helpers/animations";
import { installMockBridge } from "../helpers/bridge";

/**
 * Agent Progress, end to end through the mock relay.
 *
 * Every lane on this screen exists because signed events prove it exists, and
 * every `Reachable` on it exists because an unexpired kind-24223 lease proves
 * *that*. So the fixture signs real 44221/44223/44224/44229/44230 facts and
 * real leases and seeds the bytes; nothing about the path under test is
 * stubbed, only the relay carrying it.
 *
 * The five states below are the ones the panel had never been seen rendering:
 * reachable, unverified, closed, a partial read, and a durable session holding
 * several provider executions. Each is captured scoped to its own row, and the
 * hashes are asserted pairwise distinct — two "different" screenshots of the
 * same pixels is the most common way a screenshot pack lies.
 */

const PROVIDER_SECRET = generateSecretKey();
const PROVIDER_PUBKEY = getPublicKey(PROVIDER_SECRET);
const OPERATOR_SECRET = generateSecretKey();

/** `general` in the mock channel fixture; the `h` tag must match exactly. */
const CHANNEL_ID = "9a1657ac-f7aa-5db0-b632-d8bbeb6dfb50";
const CHANNEL_NAME = "general";
const MOCK_IDENTITY_PUBKEY = "deadbeef".repeat(8);
const PROJECT_COORDINATE = `30621:${MOCK_IDENTITY_PUBKEY}:agent-progress-demo`;

const REACHABLE_REF = "5b7e1c2a-90d4-4b0e-a1f3-7c2d8e6f4a10";
const UNVERIFIED_REF = "8c1d2e3f-4a5b-4c6d-8e9f-0a1b2c3d4e5f";
const RELEASED_REF = "7a8b9c0d-1e2f-4a3b-8c5d-6e7f8a9b0c1d";
const CLOSED_REF = "1d2e3f4a-5b6c-4d7e-8f90-a1b2c3d4e5f6";
const RESTARTED_REF = "2e3f4a5b-6c7d-4e8f-9a0b-1c2d3e4f5a6b";

const SCREENSHOT_DIR = "test-results/agent-progress";
const hashes = new Map<string, string>();

function nowSeconds(): number {
  return Math.floor(Date.now() / 1_000);
}

type SessionTarget = {
  driver: string;
  instanceId: string;
  sessionId: string;
  generation: number;
};

function sessionTarget(sessionId: string, generation = 1): SessionTarget {
  return {
    driver: "claude-agent-acp",
    instanceId: "agent-progress-instance",
    sessionId,
    generation,
  };
}

function createAuthorityEvents(input: {
  target: SessionTarget;
  sessionRef: string;
  commandId: string;
  title: string;
}): RelayEvent[] {
  const command = buildCodingSessionCreateEvent({
    channelId: CHANNEL_ID,
    commandId: input.commandId,
    projectRef: PROJECT_COORDINATE,
    repoRef: null,
    sessionRef: input.sessionRef,
    providerInstanceRef: input.target.instanceId,
    providerAuthorityPubkey: PROVIDER_PUBKEY,
    model: "sonnet",
    title: input.title,
    initialTurn: null,
  });
  return [
    finalizeEvent(
      {
        kind: command.kind,
        created_at: nowSeconds() - 14_400,
        tags: command.tags,
        content: command.content,
      },
      OPERATOR_SECRET,
    ) as unknown as RelayEvent,
    finalizeEvent(
      {
        kind: KIND_CODING_SESSION_LIFECYCLE_RECEIPT,
        created_at: nowSeconds() - 14_390,
        tags: [
          ["h", CHANNEL_ID],
          ["cslr-v", "cslr1-1"],
          ["csl-command", input.commandId],
          ["csl-key", lifecycleReceiptSemanticKey(input.commandId)],
        ],
        content: JSON.stringify({
          schema: "buzz-coding-session-lifecycle-receipt/v1",
          commandId: input.commandId,
          status: "created",
          session: input.target,
          error: null,
        }),
      },
      PROVIDER_SECRET,
    ) as unknown as RelayEvent,
  ];
}

/** One accepted `session.resume`: a second provider execution, same session. */
function resumeAuthorityEvents(input: {
  priorTarget: SessionTarget;
  target: SessionTarget;
  commandId: string;
}): RelayEvent[] {
  return [
    finalizeEvent(
      {
        kind: KIND_CODING_SESSION_LIFECYCLE_COMMAND,
        created_at: nowSeconds() - 14_000 + input.target.generation * 10,
        tags: [
          ["h", CHANNEL_ID],
          ["csl-v", "csl1-1"],
          ["csl-command", input.commandId],
        ],
        content: JSON.stringify({
          schema: "buzz-coding-session-lifecycle-command/v1",
          commandId: input.commandId,
          action: {
            type: "session.resume",
            session: input.priorTarget,
            providerAuthorityPubkey: PROVIDER_PUBKEY,
          },
        }),
      },
      OPERATOR_SECRET,
    ) as unknown as RelayEvent,
    finalizeEvent(
      {
        kind: KIND_CODING_SESSION_LIFECYCLE_RECEIPT,
        created_at: nowSeconds() - 13_999 + input.target.generation * 10,
        tags: [
          ["h", CHANNEL_ID],
          ["cslr-v", "cslr1-1"],
          ["csl-command", input.commandId],
          ["csl-key", lifecycleReceiptSemanticKey(input.commandId)],
        ],
        content: JSON.stringify({
          schema: "buzz-coding-session-lifecycle-receipt/v1",
          commandId: input.commandId,
          status: "resumed",
          session: input.target,
          error: null,
        }),
      },
      PROVIDER_SECRET,
    ) as unknown as RelayEvent,
  ];
}

function metadataEvent(input: {
  target: SessionTarget;
  sessionRef: string;
  title: string;
  status: string;
  branch: string | null;
  createdAtOffset: number;
}): RelayEvent {
  return finalizeEvent(
    {
      kind: KIND_CODING_SESSION_METADATA,
      created_at: nowSeconds() - input.createdAtOffset,
      tags: [
        ["h", CHANNEL_ID],
        ["csm-v", CODING_SESSION_METADATA_TAG_VERSION],
        ["cs-target", buildCodingSessionTargetKey(input.target)],
        ["csm-key", codingSessionMetadataSemanticKey(input.target)],
      ],
      content: JSON.stringify({
        schema: BUZZ_CODING_SESSION_METADATA_SCHEMA,
        session: input.target,
        projectRef: PROJECT_COORDINATE,
        repoRef: null,
        title: input.title,
        agentRef: null,
        provider: "claude-agent-acp",
        runtime: "claude-agent-acp",
        model: "sonnet",
        status: input.status,
        branch: input.branch,
        capabilities: {
          threadTurnStart: true,
          threadTurnInterrupt: true,
          threadSteer: true,
          context: false,
          diff: true,
          plan: true,
        },
        sessionRef: input.sessionRef,
      }),
    },
    PROVIDER_SECRET,
  ) as unknown as RelayEvent;
}

function nameEvent(sessionRef: string, name: string): RelayEvent {
  return finalizeEvent(
    {
      kind: KIND_CODING_SESSION_NAME,
      created_at: nowSeconds() - 600,
      tags: [
        ["h", CHANNEL_ID],
        ["d", sessionRef],
        ["csnm-v", "csnm1-1"],
      ],
      content: name,
    },
    PROVIDER_SECRET,
  ) as unknown as RelayEvent;
}

function closureEvent(sessionRef: string): RelayEvent {
  return finalizeEvent(
    {
      kind: KIND_CODING_SESSION_CLOSURE,
      created_at: nowSeconds() - 300,
      tags: [
        ["h", CHANNEL_ID],
        ["d", sessionRef],
        ["csc-v", "csc1-1"],
      ],
      content: JSON.stringify({
        action: "closed",
        genesisRef: "a".repeat(64),
        sessionRef,
        v: 1,
      }),
    },
    PROVIDER_SECRET,
  ) as unknown as RelayEvent;
}

function leaseEvent(input: {
  target: SessionTarget;
  commandId: string;
  sequence: number;
  state: "live" | "released";
  createdAtOffset: number;
}): RelayEvent {
  return finalizeEvent(
    {
      kind: KIND_CODING_SESSION_LEASE,
      created_at: nowSeconds() - input.createdAtOffset,
      tags: [
        ["h", CHANNEL_ID],
        ["cslease-v", "cslease1-1"],
        ["cs-target", buildCodingSessionTargetKey(input.target)],
        ["csl-command", input.commandId],
        ["cslease-seq", String(input.sequence)],
      ],
      content: JSON.stringify({
        schema: "buzz-coding-session-lease/v1",
        target: input.target,
        state: input.state,
        leaseSequence: input.sequence,
      }),
    },
    PROVIDER_SECRET,
  ) as unknown as RelayEvent;
}

function transcriptEvent(input: {
  target: SessionTarget;
  eventSeq: number;
  item: Record<string, unknown>;
}): RelayEvent {
  return finalizeEvent(
    {
      kind: KIND_CODING_SESSION_TRANSCRIPT,
      created_at: nowSeconds() - 120 + input.eventSeq,
      tags: [
        ["h", CHANNEL_ID],
        ["cst-v", CODING_SESSION_TRANSCRIPT_TAG_VERSION],
        ["cs-target", buildCodingSessionTargetKey(input.target)],
        ["cst-seq", String(input.eventSeq)],
        [
          "cst-key",
          codingSessionTranscriptSemanticKey(input.target, input.eventSeq),
        ],
      ],
      content: JSON.stringify({
        schema: BUZZ_CODING_SESSION_TRANSCRIPT_SCHEMA,
        session: input.target,
        eventSeq: input.eventSeq,
        timestamp: (nowSeconds() - 120 + input.eventSeq) * 1_000,
        turnId: `turn-${input.target.sessionId}`,
        item: input.item,
      }),
    },
    PROVIDER_SECRET,
  ) as unknown as RelayEvent;
}

const REACHABLE_TARGET = sessionTarget("11111111-2222-3333-4444-555555555555");
const UNVERIFIED_TARGET = sessionTarget("22222222-3333-4444-5555-666666666666");
const RELEASED_TARGET = sessionTarget("55555555-6666-7777-8888-999999999999");
const CLOSED_TARGET = sessionTarget("33333333-4444-5555-6666-777777777777");
const RESTARTED_GEN1 = sessionTarget("44444444-5555-6666-7777-888888888888", 1);
const RESTARTED_GEN2 = sessionTarget("44444444-5555-6666-7777-888888888888", 2);
const RESTARTED_GEN3 = sessionTarget("44444444-5555-6666-7777-888888888888", 3);
const RESTARTED_SECOND_EXECUTION = sessionTarget(
  "66666666-7777-8888-9999-aaaaaaaaaaaa",
  1,
);

/**
 * Five durable sessions covering every coordination state.
 *
 * The reachable one's newest durable observation is deliberately three hours
 * old: only its separate, unexpired lease makes it reachable, which is exactly
 * the claim the old freshness window could not have made. The closed one holds
 * a *live* lease so closure's precedence over reachability is observable on
 * screen rather than merely asserted in a fold test.
 */
function seededSessionFacts(): RelayEvent[] {
  return [
    // Reachable: an unexpired live lease, over an hours-old observation.
    ...createAuthorityEvents({
      target: REACHABLE_TARGET,
      sessionRef: REACHABLE_REF,
      commandId: "ap-create-reachable",
      title: "Relay ingest",
    }),
    metadataEvent({
      target: REACHABLE_TARGET,
      sessionRef: REACHABLE_REF,
      title: "Relay ingest",
      status: "running",
      branch: "wip/agent-sidebar",
      createdAtOffset: 10_800,
    }),
    nameEvent(REACHABLE_REF, "Relay ingest"),
    transcriptEvent({
      target: REACHABLE_TARGET,
      eventSeq: 1,
      item: { kind: "user_prompt", content: "Land the coordination extract" },
    }),
    transcriptEvent({
      target: REACHABLE_TARGET,
      eventSeq: 2,
      item: {
        kind: "tool_call",
        tool: {
          toolName: "Bash",
          toolId: "tool-1",
          input: { command: "cargo test -p buzz-core" },
        },
      },
    }),
    leaseEvent({
      target: REACHABLE_TARGET,
      commandId: "ap-create-reachable",
      sequence: 10,
      state: "live",
      createdAtOffset: 5,
    }),

    // Unverified: no lease was ever seen for this session.
    ...createAuthorityEvents({
      target: UNVERIFIED_TARGET,
      sessionRef: UNVERIFIED_REF,
      commandId: "ap-create-no-lease",
      title: "Conformance corpus",
    }),
    metadataEvent({
      target: UNVERIFIED_TARGET,
      sessionRef: UNVERIFIED_REF,
      title: "Conformance corpus",
      status: "running",
      branch: null,
      createdAtOffset: 172_800,
    }),
    nameEvent(UNVERIFIED_REF, "Conformance corpus"),

    // Unverified: the provider released its lease on purpose.
    ...createAuthorityEvents({
      target: RELEASED_TARGET,
      sessionRef: RELEASED_REF,
      commandId: "ap-create-released",
      title: "Desktop screenshots",
    }),
    metadataEvent({
      target: RELEASED_TARGET,
      sessionRef: RELEASED_REF,
      title: "Desktop screenshots",
      status: "idle",
      branch: "main",
      createdAtOffset: 3_600,
    }),
    nameEvent(RELEASED_REF, "Desktop screenshots"),
    leaseEvent({
      target: RELEASED_TARGET,
      commandId: "ap-create-released",
      sequence: 8,
      state: "released",
      createdAtOffset: 20,
    }),

    // Closed: a human closed it while its provider still holds a live lease.
    ...createAuthorityEvents({
      target: CLOSED_TARGET,
      sessionRef: CLOSED_REF,
      commandId: "ap-create-closed",
      title: "Lease expiry audit",
    }),
    metadataEvent({
      target: CLOSED_TARGET,
      sessionRef: CLOSED_REF,
      title: "Lease expiry audit",
      status: "completed",
      branch: "main",
      createdAtOffset: 7_200,
    }),
    nameEvent(CLOSED_REF, "Lease expiry audit"),
    leaseEvent({
      target: CLOSED_TARGET,
      commandId: "ap-create-closed",
      sequence: 12,
      state: "live",
      createdAtOffset: 10,
    }),
    closureEvent(CLOSED_REF),

    // Three generations of one provider execution, plus a genuinely distinct
    // second execution, all inside one durable session.
    ...createAuthorityEvents({
      target: RESTARTED_GEN1,
      sessionRef: RESTARTED_REF,
      commandId: "ap-create-restarted",
      title: "Shared coordination fold",
    }),
    ...resumeAuthorityEvents({
      priorTarget: RESTARTED_GEN1,
      target: RESTARTED_GEN2,
      commandId: "ap-resume-restarted-2",
    }),
    ...resumeAuthorityEvents({
      priorTarget: RESTARTED_GEN2,
      target: RESTARTED_GEN3,
      commandId: "ap-resume-restarted-3",
    }),
    ...createAuthorityEvents({
      target: RESTARTED_SECOND_EXECUTION,
      sessionRef: RESTARTED_REF,
      commandId: "ap-create-restarted-second-execution",
      title: "Shared coordination fold",
    }),
    metadataEvent({
      target: RESTARTED_GEN3,
      sessionRef: RESTARTED_REF,
      title: "Shared coordination fold",
      status: "waiting_for_input",
      branch: "wip/agent-sidebar",
      createdAtOffset: 1_800,
    }),
    nameEvent(RESTARTED_REF, "Shared coordination fold"),
  ];
}

async function boot(page: Page, options: { rejectKinds?: number[] } = {}) {
  await page.addInitScript(
    ({ rejectKinds }) => {
      window.localStorage.setItem(
        "buzz-feature-overrides-v1",
        JSON.stringify({ "agent-progress": true }),
      );
      if (rejectKinds) {
        window.__BUZZ_E2E_REJECT_PROJECT_QUERY_KINDS__ = rejectKinds;
      }
    },
    { rejectKinds: options.rejectKinds ?? null },
  );
  await installMockBridge(page);
  await page.goto("/", { waitUntil: "domcontentloaded" });
  await expect(page.getByTestId("open-agent-progress-view")).toBeVisible({
    timeout: 10_000,
  });
}

async function seedSessionFacts(page: Page, events: RelayEvent[]) {
  await page.evaluate(
    ({ channelName, seeds }) => {
      const seed = window.__BUZZ_E2E_SEED_MOCK_SIGNED_EVENT__;
      if (!seed) throw new Error("mock signed-event seam is missing");
      for (const event of seeds as never[]) {
        seed({ channelName, event });
      }
    },
    { channelName: CHANNEL_NAME, seeds: events as never },
  );
}

async function openPanel(page: Page) {
  await page.getByTestId("open-agent-progress-view").click();
  await expect(page).toHaveURL(/\/agent-progress$/);
  await expect(page.getByTestId("agent-progress-panel")).toBeVisible({
    timeout: 10_000,
  });
}

function remember(name: string, buffer: Buffer) {
  const digest = createHash("sha256").update(buffer).digest("hex");
  for (const [other, otherDigest] of hashes) {
    expect(digest, `${name} captured the same pixels as ${other}`).not.toBe(
      otherDigest,
    );
  }
  hashes.set(name, digest);
}

/** Scoped shot — the only way two states of one screen differ in the pixels. */
async function captureLocator(page: Page, locator: Locator, name: string) {
  await waitForAnimations(page);
  remember(
    name,
    await locator.screenshot({ path: `${SCREENSHOT_DIR}/${name}.png` }),
  );
}

async function capture(page: Page, name: string) {
  await waitForAnimations(page);
  remember(
    name,
    await page.screenshot({ path: `${SCREENSHOT_DIR}/${name}.png` }),
  );
}

function laneNamed(page: Page, label: string): Locator {
  return page.getByTestId("agent-progress-lane").filter({ hasText: label });
}

test("every coordination state renders, each from the evidence that proves it", async ({
  page,
}) => {
  await boot(page);
  await seedSessionFacts(page, seededSessionFacts());
  await openPanel(page);

  await expect(page.getByTestId("agent-progress-lane")).toHaveCount(5, {
    timeout: 15_000,
  });

  // Reachable — an unexpired lease, over a three-hour-old observation. The old
  // fold's 30-minute freshness window would have called this stale.
  const reachable = laneNamed(page, "Relay ingest");
  await expect(
    reachable.getByTestId("agent-progress-lane-coordination"),
  ).toHaveText("Reachable");
  await expect(reachable).toHaveAttribute(
    "data-lane-coordination",
    "provider_reachable",
  );
  await expect(
    reachable.getByTestId("agent-progress-lane-reported"),
  ).not.toContainText("last reported");
  // The activity line is the locally-held transcript, decoded — not a
  // placeholder. A fixture that stopped decoding would otherwise pass with the
  // panel's honest "could not read this item" text in its place.
  await expect(
    reachable.getByTestId("agent-progress-lane-activity"),
  ).toHaveText("\u25b8 Bash");
  await captureLocator(page, reachable, "01-reachable");

  // Unverified — a *recent* claim of `running` with nothing to prove it. The
  // row states the report as a report and never as a live condition.
  const unverified = laneNamed(page, "Conformance corpus");
  await expect(
    unverified.getByTestId("agent-progress-lane-coordination"),
  ).toHaveText("Unverified");
  await expect(
    unverified.getByTestId("agent-progress-lane-reported"),
  ).toContainText("last reported");
  await expect(unverified).not.toContainText("Reachable");
  await captureLocator(page, unverified, "02-unverified");

  // Closed — the human closure outranks the live lease this session still
  // holds. If reachability ever won here, the row would read Reachable.
  const closed = laneNamed(page, "Lease expiry audit");
  await expect(
    closed.getByTestId("agent-progress-lane-coordination"),
  ).toHaveText("Closed");
  await captureLocator(page, closed, "03-closed");

  // Multi-execution — one durable session, two authority-proven execution
  // identities. Three resumed generations of the first still count once.
  const restarted = laneNamed(page, "Shared coordination fold");
  await expect(restarted).toHaveAttribute("data-lane-executions", "2");
  await expect(
    restarted.getByTestId("agent-progress-lane-executions"),
  ).toHaveText("2 exec");
  await captureLocator(page, restarted, "04-multi-execution");

  // The footer counts sessions, discloses executions separately, and — because
  // this read completed — states the count without hedging.
  const footer = page.getByTestId("agent-progress-footer-counts");
  await expect(footer).toHaveText(
    "5 sessions · 1 reachable · 3 unverified · 1 closed",
  );
  await expect(footer).not.toContainText("At least");
  await expect(page.getByTestId("agent-progress-footer-executions")).toHaveText(
    "6 executions",
  );
  // Nothing this surface reads carries usage, so no total may appear.
  await expect(page.getByTestId("agent-progress-footer")).not.toContainText(
    /tok|\$/,
  );
  // The retired vocabulary must not have come back as a coordination value.
  // `Working` still appears on the *reported* axis, because that is the one
  // shared wire-to-label mapping the session header uses and a second spelling
  // here would be the drift this whole change exists to remove — but no row
  // may answer "is this alive?" with it.
  for (const value of await page
    .getByTestId("agent-progress-lane-coordination")
    .allTextContents()) {
    expect(["Reachable", "Unverified", "Closed"]).toContain(value);
  }
  await captureLocator(
    page,
    page.getByTestId("agent-progress-panel"),
    "05-panel",
  );
});

test("a lease read that failed is a floor, never a quiet fleet", async ({
  page,
}) => {
  await boot(page, { rejectKinds: [KIND_CODING_SESSION_LEASE] });
  await seedSessionFacts(page, seededSessionFacts());
  await openPanel(page);

  await expect(page.getByTestId("agent-progress-lane")).toHaveCount(5, {
    timeout: 15_000,
  });

  // The notice says what failed, and refuses the reassuring reading.
  const notice = page.getByTestId("agent-progress-incomplete");
  await expect(notice).toBeVisible();
  await expect(notice).toContainText("did not complete");
  await expect(notice).toContainText("not a claim that nothing is running");

  // With the lease snapshot missing, nothing can be called reachable — and the
  // counts say "at least" rather than presenting a floor as a census.
  await expect(page.getByTestId("agent-progress-footer-counts")).toContainText(
    "At least 5 sessions",
  );
  await expect(page.getByTestId("agent-progress-panel")).not.toContainText(
    "Reachable",
  );
  // The adjacent project shelf has metadata but no lease projection. It must
  // report that history neutrally instead of contradicting this panel with a
  // green current-liveness claim in the same window.
  await expect(
    page.getByRole("region", { name: "Open Sessions" }),
  ).toBeVisible();
  await expect(page.getByText("Reported working").first()).toBeVisible();
  await expect(
    page.getByRole("region", { name: "Active Sessions" }),
  ).toHaveCount(0);
  await capture(page, "06-partial-read");
});
