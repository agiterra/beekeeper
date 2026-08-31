import { expect, test, type Page } from "@playwright/test";
import {
  finalizeEvent,
  generateSecretKey,
  getPublicKey,
} from "nostr-tools/pure";

import { buildCodingSessionTargetKey } from "@/features/coding-sessions/lib/codingSessionCommand";
import { buildCodingSessionGenesisEvent } from "@/features/coding-sessions/lib/codingSessionGenesis";
import { buildCodingSessionGoalEvent } from "@/features/coding-sessions/lib/codingSessionGoal";
import { buildCodingSessionCreateEvent } from "@/features/coding-sessions/lib/codingSessionLifecycleCommand";
import {
  BUZZ_CODING_SESSION_METADATA_SCHEMA,
  CODING_SESSION_LIFECYCLE_RECEIPT_SCHEMA,
  CODING_SESSION_LIFECYCLE_RECEIPT_TAG_VERSION,
  CODING_SESSION_METADATA_TAG_VERSION,
  codingSessionMetadataSemanticKey,
  codingSessionReceiptSemanticKey,
  lifecycleReceiptSemanticKey,
} from "@/features/coding-sessions/lib/codingSessionIngressPayloads";
import { CODING_SESSION_TEAM_TRANSACTION_SCHEMA } from "@/features/coding-sessions/lib/codingSessionTeamTransactionWire";
import {
  BUZZ_CODING_SESSION_TRANSCRIPT_SCHEMA,
  CODING_SESSION_TRANSCRIPT_TAG_VERSION,
  codingSessionTranscriptSemanticKey,
} from "@/features/coding-sessions/lib/codingSessionTranscriptPresentation";
import type { CodingSessionCommandTarget } from "@/features/coding-sessions/lib/codingSessionCommand";
import type { RelayEvent } from "@/shared/api/types";
import {
  KIND_CODING_SESSION_LIFECYCLE_RECEIPT,
  KIND_CODING_SESSION_LEASE,
  KIND_CODING_SESSION_METADATA,
  KIND_CODING_SESSION_AUTHORITY_TRANSITION,
  KIND_CODING_SESSION_TEAM_TRANSACTION,
  KIND_CODING_SESSION_TRANSCRIPT,
} from "@/shared/constants/kinds";
import { waitForAnimations } from "../helpers/animations";
import { installMockBridge } from "../helpers/bridge";
import {
  assertConversationAndMissionLenses,
  assertMissionRestartRecovery,
  assertNarrowMissionSurfaceHierarchy,
  buildGovernedMissionApprovalPhases,
} from "./helpers/codingSessionMissionLensAssertions";

const CHANNEL_NAME = "engineering";
const CHANNEL_ID = "1c7e1c02-87bb-5e88-b2da-5a7a9432d0c9";
const SESSION_REF = "5b7e1c2a-90d4-4b0e-a1f3-7c2d8e6f4a10";
const BUILDER_SECRET = generateSecretKey();
const BUILDER_PROVIDER = getPublicKey(BUILDER_SECRET);
const VERIFIER_SECRET = generateSecretKey();
const VERIFIER_PROVIDER = getPublicKey(VERIFIER_SECRET);
const BUILDER_ACTOR_SECRET = generateSecretKey();
const BUILDER_ACTOR = getPublicKey(BUILDER_ACTOR_SECRET);
const VERIFIER_ACTOR_SECRET = generateSecretKey();
const VERIFIER_ACTOR = getPublicKey(VERIFIER_ACTOR_SECRET);
const FOUNDER_SECRET = generateSecretKey();
const FOUNDER = getPublicKey(FOUNDER_SECRET);
const RELAY_SECRET = generateSecretKey();
const RELAY = getPublicKey(RELAY_SECRET);
const BUILDER_TARGET: CodingSessionCommandTarget = {
  driver: "claude-agent-acp",
  instanceId: "builder-instance",
  sessionId: "11111111-2222-3333-4444-555555555555",
  generation: 1,
};
const VERIFIER_TARGET: CodingSessionCommandTarget = {
  driver: "codex-acp",
  instanceId: "verifier-instance",
  sessionId: "66666666-7777-8888-9999-000000000000",
  generation: 1,
};
const SCREENSHOTS = "test-results/singularity-lens";
const GENESIS_CREATED_AT = 1_800_000_000;
const OBSERVED_FILE =
  "desktop/src/features/coding-sessions/ui/CodingSessionUmbrellaWorkspace.tsx";

function governedMissionEvents(): {
  events: RelayEvent[];
  foldResponse: Record<string, unknown>;
} {
  const genesisInput = buildCodingSessionGenesisEvent({
    channelId: CHANNEL_ID,
    sessionRef: SESSION_REF,
  });
  const genesis = finalizeEvent(
    {
      kind: genesisInput.kind,
      tags: genesisInput.tags,
      content: genesisInput.content,
      created_at: GENESIS_CREATED_AT,
    },
    FOUNDER_SECRET,
  ) as unknown as RelayEvent;
  const lifecycle: RelayEvent[] = [genesis];
  for (const [index, seat] of (
    [
      {
        actor: BUILDER_ACTOR,
        role: "builder",
        provider: BUILDER_PROVIDER,
        providerInstanceRef: "claude-primary",
        secret: BUILDER_SECRET,
        target: BUILDER_TARGET,
      },
      {
        actor: VERIFIER_ACTOR,
        role: "verifier",
        provider: VERIFIER_PROVIDER,
        providerInstanceRef: "codex-primary",
        secret: VERIFIER_SECRET,
        target: VERIFIER_TARGET,
      },
    ] as const
  ).entries()) {
    const commandId = `mission-seat-${index + 1}`;
    const createInput = buildCodingSessionCreateEvent({
      channelId: CHANNEL_ID,
      commandId,
      projectRef: null,
      repoRef: null,
      sessionRef: SESSION_REF,
      genesisRef: genesis.id,
      actor: seat.actor,
      role: seat.role,
      providerInstanceRef: seat.providerInstanceRef,
      providerAuthorityPubkey: seat.provider,
      model: index === 0 ? "sonnet" : "gpt-5.6-sol",
      title: "Portable team loop",
      initialTurn: null,
    });
    const create = finalizeEvent(
      {
        kind: createInput.kind,
        tags: createInput.tags,
        content: createInput.content,
        created_at: GENESIS_CREATED_AT + index + 1,
      },
      FOUNDER_SECRET,
    ) as unknown as RelayEvent;
    const receipt = finalizeEvent(
      {
        kind: KIND_CODING_SESSION_LIFECYCLE_RECEIPT,
        tags: [
          ["h", CHANNEL_ID],
          ["cslr-v", CODING_SESSION_LIFECYCLE_RECEIPT_TAG_VERSION],
          ["csl-command", commandId],
          ["csl-key", lifecycleReceiptSemanticKey(commandId)],
        ],
        content: JSON.stringify({
          schema: CODING_SESSION_LIFECYCLE_RECEIPT_SCHEMA,
          commandId,
          status: "created",
          session: seat.target,
          error: null,
        }),
        created_at: GENESIS_CREATED_AT + index + 2,
      },
      seat.secret,
    ) as unknown as RelayEvent;
    lifecycle.push(create, receipt);
  }
  const goalInput = buildCodingSessionGoalEvent({
    channelId: CHANNEL_ID,
    content: "Ship a portable provider-neutral team loop.",
    sessionRef: SESSION_REF,
  });
  const goal = finalizeEvent(
    {
      kind: goalInput.kind,
      tags: goalInput.tags,
      content: goalInput.content,
      created_at: GENESIS_CREATED_AT + 5,
    },
    FOUNDER_SECRET,
  ) as unknown as RelayEvent;
  const grantPayload = {
    genesisRef: genesis.id,
    prevAccepted: null,
    seq: 1,
    type: "grant-seat",
    granteePubkey: BUILDER_ACTOR,
    role: "builder",
  };
  const grant = finalizeEvent(
    {
      kind: KIND_CODING_SESSION_AUTHORITY_TRANSITION,
      tags: [
        ["h", CHANNEL_ID],
        ["csat-v", "csat1-1"],
        ["csat-genesis", genesis.id],
      ],
      content: JSON.stringify(grantPayload),
      created_at: GENESIS_CREATED_AT + 6,
    },
    FOUNDER_SECRET,
  ) as unknown as RelayEvent;
  const grantReceipt = finalizeEvent(
    {
      kind: 40099,
      tags: [["h", CHANNEL_ID]],
      content: JSON.stringify({
        type: "coding_session_authority_transition_accepted",
        genesisRef: genesis.id,
        acceptedEventId: grant.id,
        seq: 1,
        transitionType: "grant-seat",
        granteePubkey: BUILDER_ACTOR,
        role: "builder",
      }),
      created_at: GENESIS_CREATED_AT + 7,
    },
    RELAY_SECRET,
  ) as unknown as RelayEvent;
  const transaction = (
    type: "assignment" | "report",
    body: Record<string, unknown>,
    secret: Uint8Array,
    createdAt: number,
  ) =>
    finalizeEvent(
      {
        kind: KIND_CODING_SESSION_TEAM_TRANSACTION,
        tags: [
          ["h", CHANNEL_ID],
          ["d", SESSION_REF],
          ["cstx-v", CODING_SESSION_TEAM_TRANSACTION_SCHEMA],
          ["cstx-genesis", genesis.id],
          ["cstx-type", type],
        ],
        content: JSON.stringify({
          schema: CODING_SESSION_TEAM_TRANSACTION_SCHEMA,
          sessionRef: SESSION_REF,
          genesisRef: genesis.id,
          type,
          supersedes: null,
          deliveryCommandId: null,
          body,
        }),
        created_at: createdAt,
      },
      secret,
    ) as unknown as RelayEvent;
  const assignment = transaction(
    "assignment",
    {
      assigneeActor: BUILDER_ACTOR,
      assigneeRole: "builder",
      objective: "Mount the signed Mission inspector.",
      brief: "Preserve Conversation and show only canonical Mission facts.",
      branch: "portable-team-loop",
      baseSha: "1".repeat(40),
      fileOwnership: [OBSERVED_FILE],
      acceptanceSteps: ["Run the real mock-bridge smoke test"],
    },
    FOUNDER_SECRET,
    GENESIS_CREATED_AT + 8,
  );
  const report = transaction(
    "report",
    {
      assignmentRef: assignment.id,
      summary: "Mission inspector mounted with signed evidence.",
      branch: "portable-team-loop",
      baseSha: "1".repeat(40),
      headSha: "2".repeat(40),
      files: [OBSERVED_FILE],
      tests: [
        {
          name: "Mission inspector smoke",
          command: "pnpm playwright test coding-session-mission-lens.spec.ts",
          outcome: "passed",
          evidence: "Wide panel and narrow drawer rendered.",
        },
      ],
      redBeforeGreen: null,
      deviations: [],
      residuals: [],
      anomalies: [],
    },
    BUILDER_ACTOR_SECRET,
    GENESIS_CREATED_AT + 9,
  );
  const inputEventIds = [assignment.id, report.id].sort();
  return {
    events: [...lifecycle, goal, grant, grantReceipt, assignment, report],
    foldResponse: {
      schema: "buzz-coding-session-team-fold-adapter/v1",
      implementation: "buzz-core",
      inputEventIds,
      context: {
        channelRef: CHANNEL_ID,
        sessionRef: SESSION_REF,
        genesisRef: genesis.id,
        founderPubkey: FOUNDER,
        authorityHeadEventId: grant.id,
        authorityHeadSeq: 1,
      },
      includedEventIds: inputEventIds,
      excluded: [],
      conflicts: [],
      assignments: [
        {
          assignmentEventId: assignment.id,
          governedReportEventId: report.id,
          dispositionEventId: null,
          acknowledgementEventId: null,
          settled: false,
        },
      ],
      canonicalTerminal: null,
    },
  };
}

const GOVERNED_MISSION = governedMissionEvents();

const UNGOVERNED_MISSION = {
  events: GOVERNED_MISSION.events.filter(
    (event) => event.kind !== KIND_CODING_SESSION_TEAM_TRANSACTION,
  ),
  foldResponse: {
    ...GOVERNED_MISSION.foldResponse,
    inputEventIds: [],
    includedEventIds: [],
    assignments: [],
    canonicalTerminal: null,
  },
};

function governedMissionWithTerminal(
  state: "completed" | "blocked",
): typeof GOVERNED_MISSION {
  const events = [...GOVERNED_MISSION.events];
  const assignment = events.find((event) => {
    if (event.kind !== KIND_CODING_SESSION_TEAM_TRANSACTION) return false;
    return JSON.parse(event.content).type === "assignment";
  });
  const report = events.find((event) => {
    if (event.kind !== KIND_CODING_SESSION_TEAM_TRANSACTION) return false;
    return JSON.parse(event.content).type === "report";
  });
  if (!assignment || !report) throw new Error("governed fixture is incomplete");
  const genesisRef = GOVERNED_MISSION.foldResponse.context.genesisRef as string;
  const transaction = (
    type:
      | "verdict"
      | "acknowledgement"
      | "mission.completed"
      | "mission.blocked",
    body: Record<string, unknown>,
    secret: Uint8Array,
    createdAt: number,
  ) =>
    finalizeEvent(
      {
        kind: KIND_CODING_SESSION_TEAM_TRANSACTION,
        tags: [
          ["h", CHANNEL_ID],
          ["d", SESSION_REF],
          ["cstx-v", CODING_SESSION_TEAM_TRANSACTION_SCHEMA],
          ["cstx-genesis", genesisRef],
          ["cstx-type", type],
        ],
        content: JSON.stringify({
          schema: CODING_SESSION_TEAM_TRANSACTION_SCHEMA,
          sessionRef: SESSION_REF,
          genesisRef,
          type,
          supersedes: null,
          deliveryCommandId: null,
          body,
        }),
        created_at: createdAt,
      },
      secret,
    ) as unknown as RelayEvent;

  let dispositionEventId: string | null = null;
  let acknowledgementEventId: string | null = null;
  let terminal: RelayEvent;
  if (state === "completed") {
    const verdict = transaction(
      "verdict",
      {
        subtype: "disposition",
        assignmentRef: assignment.id,
        reportRef: report.id,
        refutationRef: null,
        decision: "approve-with-notes",
        summary: "The signed report is approved.",
        findings: [],
        requiredAction: "Publish the signed follow-up note.",
      },
      FOUNDER_SECRET,
      GENESIS_CREATED_AT + 10,
    );
    const acknowledgement = transaction(
      "acknowledgement",
      {
        acknowledgedEventRef: verdict.id,
        status: "received",
        note: "Approval received.",
      },
      BUILDER_ACTOR_SECRET,
      GENESIS_CREATED_AT + 11,
    );
    terminal = transaction(
      "mission.completed",
      {
        assignmentRefs: [assignment.id],
        landedShas: ["3".repeat(40)],
        summary: "Portable Mission evidence is complete.",
        followUps: [],
      },
      FOUNDER_SECRET,
      GENESIS_CREATED_AT + 12,
    );
    dispositionEventId = verdict.id;
    acknowledgementEventId = acknowledgement.id;
    events.push(verdict, acknowledgement, terminal);
  } else {
    terminal = transaction(
      "mission.blocked",
      {
        assignmentRefs: [assignment.id],
        summary: "Mission cannot proceed.",
        blockers: ["Relay authority is unavailable."],
        heldOn: null,
        requiredAction: "Restore the relay signing authority.",
      },
      FOUNDER_SECRET,
      GENESIS_CREATED_AT + 10,
    );
    events.push(terminal);
  }
  const transactions = events.filter(
    (event) => event.kind === KIND_CODING_SESSION_TEAM_TRANSACTION,
  );
  const inputEventIds = transactions.map((event) => event.id).sort();
  return {
    events,
    foldResponse: {
      ...GOVERNED_MISSION.foldResponse,
      inputEventIds,
      includedEventIds: inputEventIds,
      assignments: [
        {
          assignmentEventId: assignment.id,
          governedReportEventId: report.id,
          dispositionEventId,
          acknowledgementEventId,
          settled: state === "completed",
        },
      ],
      canonicalTerminal: { eventId: terminal.id, type: `mission.${state}` },
    },
  };
}

function signedMetadata(input: {
  actor: string;
  model: string;
  role: string;
  runtime: string;
  secret: Uint8Array;
  status:
    | "running"
    | "completed"
    | "waiting_for_input"
    | "failed"
    | "disconnected";
  target: CodingSessionCommandTarget;
  title: string;
  createdAt: number;
}): RelayEvent {
  const targetKey = buildCodingSessionTargetKey(input.target);
  return finalizeEvent(
    {
      kind: KIND_CODING_SESSION_METADATA,
      created_at: input.createdAt,
      tags: [
        ["h", CHANNEL_ID],
        ["csm-v", CODING_SESSION_METADATA_TAG_VERSION],
        ["cs-target", targetKey],
        ["csm-key", codingSessionMetadataSemanticKey(input.target)],
      ],
      content: JSON.stringify({
        schema: BUZZ_CODING_SESSION_METADATA_SCHEMA,
        session: input.target,
        projectRef: null,
        repoRef: null,
        title: input.title,
        agentRef: input.actor,
        provider: input.runtime,
        runtime: input.runtime,
        model: input.model,
        status: input.status,
        branch: null,
        capabilities: {
          threadTurnStart: true,
          threadTurnInterrupt: true,
          threadSteer: true,
          context: false,
          diff: true,
          plan: true,
        },
        sessionRef: SESSION_REF,
        role: input.role,
      }),
    },
    input.secret,
  ) as unknown as RelayEvent;
}

function signedTranscript(input: {
  createdAt: number;
  eventSeq: number;
  item: unknown;
  secret: Uint8Array;
  target: CodingSessionCommandTarget;
  turnId: string;
}): RelayEvent {
  return finalizeEvent(
    {
      kind: KIND_CODING_SESSION_TRANSCRIPT,
      created_at: input.createdAt,
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
        timestamp: input.createdAt * 1_000,
        turnId: input.turnId,
        item: input.item,
      }),
    },
    input.secret,
  ) as unknown as RelayEvent;
}

function signedTurnStarted(createdAt: number): RelayEvent {
  const commandId = "mission-builder-turn";
  const status = "turn_started";
  return finalizeEvent(
    {
      kind: KIND_CODING_SESSION_LIFECYCLE_RECEIPT,
      created_at: createdAt,
      tags: [
        ["h", CHANNEL_ID],
        ["cslr-v", CODING_SESSION_LIFECYCLE_RECEIPT_TAG_VERSION],
        ["csl-command", commandId],
        ["csl-key", codingSessionReceiptSemanticKey(commandId, status)],
      ],
      content: JSON.stringify({
        schema: CODING_SESSION_LIFECYCLE_RECEIPT_SCHEMA,
        commandId,
        status,
        session: BUILDER_TARGET,
        error: null,
        turnId: "builder-turn",
      }),
    },
    BUILDER_SECRET,
  ) as unknown as RelayEvent;
}

function signedLiveLease(createdAt: number): RelayEvent {
  const commandId = "mission-seat-1";
  return finalizeEvent(
    {
      kind: KIND_CODING_SESSION_LEASE,
      created_at: createdAt,
      tags: [
        ["h", CHANNEL_ID],
        ["cslease-v", "cslease1-1"],
        ["cs-target", buildCodingSessionTargetKey(BUILDER_TARGET)],
        ["csl-command", commandId],
        ["cslease-seq", "1"],
      ],
      content: JSON.stringify({
        schema: "buzz-coding-session-lease/v1",
        target: BUILDER_TARGET,
        state: "live",
        leaseSequence: 1,
      }),
    },
    BUILDER_SECRET,
  ) as unknown as RelayEvent;
}

function missionEvents(
  governed: typeof GOVERNED_MISSION = GOVERNED_MISSION,
  builderStatus:
    | "running"
    | "waiting_for_input"
    | "failed"
    | "disconnected" = "running",
): RelayEvent[] {
  const now = Math.floor(Date.now() / 1_000);
  return [
    ...governed.events,
    signedMetadata({
      actor: BUILDER_ACTOR,
      model: "sonnet",
      role: "builder",
      runtime: "claude-agent-acp",
      secret: BUILDER_SECRET,
      status: builderStatus,
      target: BUILDER_TARGET,
      title: "Portable team loop",
      createdAt: now - 8,
    }),
    signedLiveLease(now - 2),
    signedTurnStarted(now - 5),
    signedTranscript({
      createdAt: now - 30,
      eventSeq: 1,
      item: { kind: "user_prompt", content: "Build the Mission lens." },
      secret: BUILDER_SECRET,
      target: BUILDER_TARGET,
      turnId: "builder-turn",
    }),
    signedTranscript({
      createdAt: now - 29,
      eventSeq: 2,
      item: {
        kind: "plan",
        entries: [
          { content: "Wire the participant roster", status: "completed" },
          { content: "Verify the signed live activity", status: "in_progress" },
        ],
      },
      secret: BUILDER_SECRET,
      target: BUILDER_TARGET,
      turnId: "builder-turn",
    }),
    signedTranscript({
      createdAt: now - 28,
      eventSeq: 3,
      item: {
        kind: "tool_call",
        tool: {
          toolName: "Read",
          toolId: "read-mission",
          input: { path: "CodingSessionUmbrellaWorkspace.tsx" },
        },
      },
      secret: BUILDER_SECRET,
      target: BUILDER_TARGET,
      turnId: "builder-turn",
    }),
    signedMetadata({
      actor: VERIFIER_ACTOR,
      model: "gpt-5.6-sol",
      role: "verifier",
      runtime: "codex-acp",
      secret: VERIFIER_SECRET,
      status: "completed",
      target: VERIFIER_TARGET,
      title: "Portable team loop",
      createdAt: now - 12,
    }),
    signedTranscript({
      createdAt: now - 11,
      eventSeq: 1,
      item: {
        kind: "tool_call",
        tool: {
          toolName: "Edit",
          toolKind: "edit",
          toolId: "edit-mission",
          input: { path: OBSERVED_FILE },
          edit: { paths: [OBSERVED_FILE] },
        },
      },
      secret: VERIFIER_SECRET,
      target: VERIFIER_TARGET,
      turnId: "verifier-turn",
    }),
    signedTranscript({
      createdAt: now - 10,
      eventSeq: 2,
      item: {
        kind: "tool_result",
        toolId: "edit-mission",
        toolName: "Edit",
        toolKind: "edit",
        input: { path: OBSERVED_FILE },
        edit: { paths: [OBSERVED_FILE] },
        content: "Mission workspace integration updated.",
        isError: false,
      },
      secret: VERIFIER_SECRET,
      target: VERIFIER_TARGET,
      turnId: "verifier-turn",
    }),
    signedTranscript({
      createdAt: now - 9,
      eventSeq: 3,
      item: { kind: "assistant_text", text: "The Mission hierarchy is sound." },
      secret: VERIFIER_SECRET,
      target: VERIFIER_TARGET,
      turnId: "verifier-turn",
    }),
    signedTranscript({
      createdAt: now - 8,
      eventSeq: 4,
      item: {
        kind: "result",
        subtype: "success",
        isError: false,
        durationMs: 2_000,
        result: "Verified",
      },
      secret: VERIFIER_SECRET,
      target: VERIFIER_TARGET,
      turnId: "verifier-turn",
    }),
  ];
}

async function seedAndOpen(
  page: Page,
  governed: typeof GOVERNED_MISSION = GOVERNED_MISSION,
  builderStatus:
    | "running"
    | "waiting_for_input"
    | "failed"
    | "disconnected" = "running",
) {
  await page.getByTestId(`channel-${CHANNEL_NAME}`).click();
  await page.evaluate(
    ({ channelName, events }) => {
      const seed = window.__BUZZ_E2E_SEED_MOCK_SIGNED_EVENT__;
      if (!seed) throw new Error("signed-event seeding hook is missing");
      for (const event of events) seed({ channelName, event });
    },
    {
      channelName: CHANNEL_NAME,
      events: missionEvents(governed, builderStatus),
    },
  );
  const trigger = page.getByTestId("channel-coding-sessions-trigger");
  await expect(trigger).toHaveAttribute("aria-label", "Coding sessions (1)", {
    timeout: 15_000,
  });
  await trigger.click();
  await page.getByTestId("channel-coding-session-open").first().click();
  await expect(
    page.getByTestId("coding-session-umbrella-workspace"),
  ).toBeVisible({ timeout: 15_000 });
}

async function openMockApp(
  page: Page,
  input: {
    foldResponse?: Record<string, unknown>;
    reducedMotion: "no-preference" | "reduce";
    theme: "buzz" | "buzz-dark";
  },
) {
  await page.emulateMedia({ reducedMotion: input.reducedMotion });
  await page.addInitScript(({ theme }) => {
    window.localStorage.setItem("buzz-theme", theme);
    window.localStorage.setItem("buzz:text-scale", "1.25");
  }, input);
  await installMockBridge(page, {
    codingSessionTeamFoldResponse:
      input.foldResponse ?? GOVERNED_MISSION.foldResponse,
    globalAgentConfig: {
      env_vars: {},
      provider: null,
      model: null,
      "allowed-bridge-pubkeys": [
        { pubkey: BUILDER_PROVIDER, label: "Builder provider" },
        { pubkey: VERIFIER_PROVIDER, label: "Verifier provider" },
      ],
    },
    searchProfiles: [
      { pubkey: BUILDER_ACTOR, displayName: "Bob" },
      { pubkey: VERIFIER_ACTOR, displayName: "Parallax" },
    ],
    relaySelf: RELAY,
  });
  await page.setViewportSize({ width: 1400, height: 900 });
  await page.goto("/");
}

test("Conversation and Mission are explicit persistent lenses over one signed session", async ({
  page,
}) => {
  await assertConversationAndMissionLenses(page, {
    observedFile: OBSERVED_FILE,
    openMockApp: (targetPage) =>
      openMockApp(targetPage, {
        reducedMotion: "no-preference",
        theme: "buzz",
      }),
    screenshots: SCREENSHOTS,
    seedAndOpen,
  });
});
test("Mission recovers a report after restart, then folds live verdict evidence", async ({
  page,
}) => {
  const phases = buildGovernedMissionApprovalPhases(
    governedMissionWithTerminal("completed"),
    KIND_CODING_SESSION_TEAM_TRANSACTION,
  );
  await assertMissionRestartRecovery(page, {
    baseEvents: GOVERNED_MISSION.events,
    channelName: CHANNEL_NAME,
    observedFile: OBSERVED_FILE,
    ...phases,
    openMockApp: (targetPage) =>
      openMockApp(targetPage, {
        reducedMotion: "no-preference",
        theme: "buzz",
      }),
    seedAndOpen,
    transactionKind: KIND_CODING_SESSION_TEAM_TRANSACTION,
  });
});
for (const scenario of [
  {
    name: "completed",
    governed: governedMissionWithTerminal("completed"),
    status: "running" as const,
    label: "Mission completed",
  },
  {
    name: "blocked",
    governed: governedMissionWithTerminal("blocked"),
    status: "running" as const,
    label: "Mission blocked",
  },
  {
    name: "waiting",
    governed: UNGOVERNED_MISSION,
    status: "waiting_for_input" as const,
    label: "Waiting on a person",
  },
  {
    name: "stalled",
    governed: UNGOVERNED_MISSION,
    status: "failed" as const,
    label: "Mission stalled",
  },
] as const) {
  test(`Mission renders signed ${scenario.name} state without reading silence`, async ({
    page,
  }) => {
    await openMockApp(page, {
      foldResponse: scenario.governed.foldResponse,
      reducedMotion: "no-preference",
      theme: "buzz",
    });
    await seedAndOpen(page, scenario.governed, scenario.status);
    await page.getByRole("button", { name: "Mission lens" }).click();
    const card = page.getByTestId("coding-session-mission-transaction-card");
    await expect(card).toContainText(scenario.label, { timeout: 15_000 });
    await expect(card).toContainText("Signed source");
    if (scenario.name === "blocked") {
      await expect(card).toContainText("Required action:");
    }
    if (scenario.name === "waiting") {
      await expect(card).toContainText("Reply to Bob · Builder");
    }
    await waitForAnimations(page);
    await card.screenshot({
      path: `${SCREENSHOTS}/mission-${scenario.name}.png`,
    });
  });
}

test("Mission remains accessible in dark, narrow, reduced-motion layout", async ({
  page,
}) => {
  await openMockApp(page, {
    reducedMotion: "reduce",
    theme: "buzz-dark",
  });
  await seedAndOpen(page);
  await expect(page.locator("html")).toHaveAttribute(
    "data-buzz-theme",
    "buzz-dark",
  );
  await expect
    .poll(() =>
      page.evaluate(
        () => window.matchMedia("(prefers-reduced-motion: reduce)").matches,
      ),
    )
    .toBe(true);

  await page.setViewportSize({ width: 480, height: 760 });
  const lens = page.getByRole("group", { name: "Session lens" });
  const conversation = lens.getByRole("button", {
    name: "Conversation lens",
  });
  const mission = lens.getByRole("button", { name: "Mission lens" });
  await expect(conversation).toHaveAttribute("aria-pressed", "true");
  await mission.focus();
  await expect(mission).toBeFocused();
  await page.keyboard.press("Enter");

  await assertNarrowMissionSurfaceHierarchy(page, SCREENSHOTS);

  const participants = page.getByRole("navigation", {
    name: "Session participants",
  });
  await expect(participants).toBeVisible();
  const overflow = await participants.evaluate((element) => ({
    clientWidth: element.clientWidth,
    overflowX: getComputedStyle(element).overflowX,
    scrollWidth: element.scrollWidth,
  }));
  expect(overflow.scrollWidth).toBeGreaterThan(overflow.clientWidth);
  expect(["auto", "scroll"]).toContain(overflow.overflowX);
  await expect
    .poll(() =>
      page.evaluate(() => document.documentElement.scrollWidth <= innerWidth),
    )
    .toBe(true);
  const live = page.getByTestId("coding-session-live-activity-bar");
  await expect(live).toBeVisible();
  await waitForAnimations(page);
  await live.screenshot({
    path: `${SCREENSHOTS}/mission-dark-narrow-reduced-activity.png`,
  });

  const bob = participants.getByRole("button", {
    name: /Focus Bob · Builder/,
  });
  await bob.focus();
  await expect(bob).toBeFocused();
  await page.keyboard.press("Enter");
  await expect(
    page.getByTestId("coding-session-focused-agent-notice"),
  ).toContainText("Bob");
  await waitForAnimations(page);
  await participants.screenshot({
    path: `${SCREENSHOTS}/mission-dark-narrow-reduced-participants.png`,
  });

  await conversation.focus();
  await page.keyboard.press("Enter");
  await expect(conversation).toHaveAttribute("aria-pressed", "true");
  await expect(participants).toHaveCount(0);
  await expect(
    page.getByTestId("coding-session-disposition-strip"),
  ).toBeVisible();
  await expect(
    page.getByTestId("coding-session-umbrella-timeline"),
  ).toContainText("The Mission hierarchy is sound.");
  await expect(
    page.getByTestId("coding-session-umbrella-composer"),
  ).toBeVisible();
});
