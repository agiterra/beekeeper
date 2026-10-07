import { expect, test, type Page } from "@playwright/test";
import { finalizeEvent, getPublicKey, verifyEvent } from "nostr-tools/pure";

import {
  buildCodingSessionCommandEvent,
  buildCodingSessionTargetKey,
} from "@/features/coding-sessions/lib/codingSessionCommand";
import { codingSessionTeamWakeText } from "@/features/coding-sessions/lib/codingSessionTeamWake";
import { buildCodingSessionGenesisEvent } from "@/features/coding-sessions/lib/codingSessionGenesis";
import { buildCodingSessionGoalEvent } from "@/features/coding-sessions/lib/codingSessionGoal";
import { buildCodingSessionCreateEvent } from "@/features/coding-sessions/lib/codingSessionLifecycleCommand";
import {
  BEEKEEPER_CODING_SESSION_METADATA_SCHEMA,
  CODING_SESSION_LIFECYCLE_RECEIPT_SCHEMA,
  CODING_SESSION_LIFECYCLE_RECEIPT_TAG_VERSION,
  CODING_SESSION_METADATA_TAG_VERSION,
  codingSessionMetadataSemanticKey,
  codingSessionReceiptSemanticKey,
  lifecycleReceiptSemanticKey,
} from "@/features/coding-sessions/lib/codingSessionIngressPayloads";
import { CODING_SESSION_TEAM_TRANSACTION_SCHEMA } from "@/features/coding-sessions/lib/codingSessionTeamTransactionWire";
import {
  BEEKEEPER_CODING_SESSION_TRANSCRIPT_SCHEMA,
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
  assertMissionTransactionFlow,
  assertNarrowMissionSurfaceHierarchy,
  assertProviderQueuedDelivery,
  assertZeroSwitchObservation,
  buildGovernedMissionApprovalPhases,
  governedMissionWithoutBuilderGrant,
  signedProviderWakeCommand,
  signedTurnQueuedReceipt,
} from "./helpers/codingSessionMissionLensAssertions";

function hexToBytes(value: string): Uint8Array {
  const bytes = new Uint8Array(value.length / 2);
  for (let index = 0; index < bytes.length; index += 1) {
    bytes[index] = Number.parseInt(value.slice(index * 2, index * 2 + 2), 16);
  }
  return bytes;
}

function hex(bytes: Uint8Array): string {
  return [...bytes].map((byte) => byte.toString(16).padStart(2, "0")).join("");
}

const CHANNEL_NAME = "engineering";
const CHANNEL_ID = "1c7e1c02-87bb-5e88-b2da-5a7a9432d0c9";
const SESSION_REF = "5b7e1c2a-90d4-4b0e-a1f3-7c2d8e6f4a10";
// Fixture keys are pinned, not generated. Two things depend on it: the seat
// accent (`codingSessionAgentAccent` hashes the execution key, which carries
// the provider pubkey, so a fresh key repainted every block a different
// colour on every run) and the byte-identity harness this lane uses to prove
// the Conversation lens DOM did not move. Any 32-byte value below the curve
// order is a valid secret; these are deliberately unmistakable for real ones.
const BUILDER_SECRET = hexToBytes(
  "1111111111111111111111111111111111111111111111111111111111111111",
);
const BUILDER_PROVIDER = getPublicKey(BUILDER_SECRET);
const VERIFIER_SECRET = hexToBytes(
  "2222222222222222222222222222222222222222222222222222222222222222",
);
const VERIFIER_PROVIDER = getPublicKey(VERIFIER_SECRET);
const BUILDER_ACTOR_SECRET = hexToBytes(
  "3333333333333333333333333333333333333333333333333333333333333333",
);
const BUILDER_ACTOR = getPublicKey(BUILDER_ACTOR_SECRET);
const VERIFIER_ACTOR_SECRET = hexToBytes(
  "4444444444444444444444444444444444444444444444444444444444444444",
);
const VERIFIER_ACTOR = getPublicKey(VERIFIER_ACTOR_SECRET);
// The founder is the E2E bridge's own known identity, not a fresh key.
// Two gates need it to be: the team-wake delivery plan runs only for the
// founder's Desktop (`lib/codingSessionTeamWake.ts:317-321`), and the mock
// relay's channel membership is keyed to the identities the bridge knows —
// so a fresh founder key can be the founder OR a member, never both.
const FOUNDER_SECRET = hexToBytes(
  "3dbaebadb5dfd777ff25149ee230d907a15a9e1294b40b830661e65bb42f6c03",
);
const FOUNDER = getPublicKey(FOUNDER_SECRET);
const RELAY_SECRET = hexToBytes(
  "5555555555555555555555555555555555555555555555555555555555555555",
);
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
// A team wake only exists when a seat holds the `lead` role — the wake is
// addressed to the lead's generation and nothing else can consume it. The base
// fixture is a builder plus a verifier, so it can never produce one; the wake
// scenario adds this third seat rather than re-roling the verifier, whose
// refutation authority other assertions depend on.
const LEAD_SECRET = hexToBytes(
  "6666666666666666666666666666666666666666666666666666666666666666",
);
const LEAD_PROVIDER = getPublicKey(LEAD_SECRET);
const LEAD_ACTOR_SECRET = hexToBytes(
  "7777777777777777777777777777777777777777777777777777777777777777",
);
const LEAD_ACTOR = getPublicKey(LEAD_ACTOR_SECRET);
const LEAD_TARGET: CodingSessionCommandTarget = {
  driver: "claude-agent-acp",
  instanceId: "lead-instance",
  sessionId: "aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee",
  generation: 1,
};
const SCREENSHOTS = "test-results/singularity-lens";
// Anchored to the run's own clock, not to a fixed 2027 timestamp. The stream
// interleaves transaction rows with turn blocks BY TIME, and the runtime
// transcript this fixture seeds is dated `now` — so a fixed future genesis
// sorted every signed handoff below every turn, off the first screen, which is
// the opposite of the causality plane the rows exist to show.
const GENESIS_CREATED_AT = Math.floor(Date.now() / 1_000) - 40;
const OBSERVED_FILE =
  "desktop/src/features/coding-sessions/ui/CodingSessionUmbrellaWorkspace.tsx";

/**
 * @param options.anchorSeconds unix second the genesis is signed at.
 * @param options.spacingSeconds seconds between the fixture's steps. The
 *   default of 1 reproduces the original timings byte for byte; the Route
 *   fixture uses minutes so roads have length, silences compress, and the
 *   `route-wide.png` shot shows a map rather than a legend (REVIEW-A4 F11).
 */
function governedMissionEvents(
  options: {
    anchorSeconds?: number;
    spacingSeconds?: number;
    /**
     * Add one signed `decision.request` — a B1c verb — to the mission.
     *
     * Off by default so the fixtures that count stream rows keep counting the
     * same ones. On for the Route fixture, because the rail is where a verb
     * the surface has no sign for used to take the whole Mission tab down
     * with `Element type is invalid`.
     */
    withDecisionRequest?: boolean;
    /**
     * Add the founder's signed answer to that request, plus a second request
     * the mission is still waiting on.
     *
     * Batch 3 L2: `decisions` and `waitingOnDecision` have been on the wire
     * since item 105 and nothing rendered them, so no fixture ever produced an
     * answered ruling or a mission held on a person. Off by default, for the
     * same reason `withDecisionRequest` is.
     */
    withDecisionAnswer?: boolean;
  } = {},
): {
  events: RelayEvent[];
  foldResponse: Record<string, unknown>;
} {
  const anchor = options.anchorSeconds ?? GENESIS_CREATED_AT;
  const spacing = options.spacingSeconds ?? 1;
  const stepAt = (offset: number) => anchor + offset * spacing;
  const genesisInput = buildCodingSessionGenesisEvent({
    channelId: CHANNEL_ID,
    sessionRef: SESSION_REF,
  });
  const genesis = finalizeEvent(
    {
      kind: genesisInput.kind,
      tags: genesisInput.tags,
      content: genesisInput.content,
      created_at: stepAt(0),
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
        created_at: stepAt(index + 1),
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
        created_at: stepAt(index + 2),
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
      created_at: stepAt(5),
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
      created_at: stepAt(6),
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
      created_at: stepAt(7),
    },
    RELAY_SECRET,
  ) as unknown as RelayEvent;
  const transaction = (
    type: "assignment" | "report" | "decision.request" | "decision.answer",
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
    stepAt(8),
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
    stepAt(9),
  );
  // One open ruling, asked by the builder and held on the founder. This is a
  // B1c verb the wire has carried since batch 2, and the surface must draw a
  // sign for it rather than throwing on a glyph it has no entry for.
  const decisionRequest = options.withDecisionRequest
    ? transaction(
        "decision.request",
        {
          question: "Land the inspector now, or hold for the verifier's gate?",
          options: ["Land the inspector now", "Hold for the gate"],
          heldOn: "founder",
          blocks: [],
          recommendation: null,
        },
        BUILDER_ACTOR_SECRET,
        stepAt(10),
      )
    : null;
  // The founder's ruling on that request, and a second one still open — the
  // two states §1g's queue has to tell apart. The second one `blocks` the open
  // assignment, so the row can say what it is holding up as well as who holds
  // it; the first blocks nothing, which is live run 2's own shape.
  const decisionAnswer =
    decisionRequest && options.withDecisionAnswer
      ? transaction(
          "decision.answer",
          {
            requestRef: decisionRequest.id,
            choice: 0,
            note: null,
          },
          FOUNDER_SECRET,
          stepAt(11),
        )
      : null;
  const openDecisionRequest =
    decisionRequest && options.withDecisionAnswer
      ? transaction(
          "decision.request",
          {
            question: "Ship the verifier's gate in this batch, or the next?",
            options: ["This batch", "The next batch"],
            heldOn: "founder",
            blocks: [assignment.id],
            recommendation: null,
          },
          BUILDER_ACTOR_SECRET,
          stepAt(12),
        )
      : null;
  const transactions = [
    assignment,
    report,
    ...(decisionRequest ? [decisionRequest] : []),
    ...(decisionAnswer ? [decisionAnswer] : []),
    ...(openDecisionRequest ? [openDecisionRequest] : []),
  ];
  const inputEventIds = transactions.map((event) => event.id).sort();
  return {
    events: [...lifecycle, goal, grant, grantReceipt, ...transactions],
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
        // L8.3: the adapter echoes the `verifierRequired` it was asked
        // with, and the decoder requires the key. This fixture reads no
        // policy, so it asks with `false` and is echoed `false`.
        verifierRequired: false,
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
          // `settledBy`/`awaiting` are the rest of the settlement the decoder
          // requires (`isSettlement`), and they are not free choices:
          // `buzz-core`'s `awaiting_link` answers exactly this chain — a
          // report is filed and no approving disposition rules it — with
          // `disposition`, owed by the `lead`, naming no actor
          // (`coding_session_team_transaction_fold_settlement.rs:450-457`).
          // `settledBy` is null exactly when `settled` is false, and
          // `awaiting` is null exactly when it is true.
          settledBy: null,
          awaiting: {
            link: "disposition",
            owedByRole: "lead",
            owedByActor: null,
          },
        },
      ],
      // Required by the adapter and the TS decoder as of this batch: reports
      // the Rust fold INCLUDED by assignee equality whose author holds no live
      // seat for the assignment's role. Empty here — this fixture's builder is
      // properly granted. Every other `foldResponse` in this spec and in the
      // assertions helper spreads this object, so it is the only place the
      // field has to be declared.
      unseatedReports: [],
      // Required by the adapter and the TS decoder as of batch 2 B1c, in the
      // same way and for the same reason: the decoder does `hasExactFields` on
      // the response's top level, so a mocked response missing any of these
      // three throws `native coding-session team fold returned a malformed
      // response` for every team session. `[]`/`null` are real answers — this
      // fixture's mission carries no note, no ruling, and waits on nobody.
      // Every other `foldResponse` in this spec and in the assertions helper
      // spreads this object, so this is the only place they have to be
      // declared.
      notes: [],
      decisions: decisionRequest
        ? [
            {
              requestId: decisionRequest.id,
              heldOn: "founder",
              blocks: [],
              answeredBy: decisionAnswer ? FOUNDER : null,
              answerId: decisionAnswer ? decisionAnswer.id : null,
            },
            ...(openDecisionRequest
              ? [
                  {
                    requestId: openDecisionRequest.id,
                    heldOn: "founder",
                    blocks: [assignment.id],
                    answeredBy: null,
                    answerId: null,
                  },
                ]
              : []),
          ]
        : [],
      waitingOnDecision: openDecisionRequest
        ? { requestId: openDecisionRequest.id, heldOn: "founder" }
        : decisionRequest && !decisionAnswer
          ? { requestId: decisionRequest.id, heldOn: "founder" }
          : null,
      // Required by the decoder's `hasExactFields` (ledger 183(a)/(b)); `null`
      // because this fixture publishes no `mission.completed`.
      pendingCompletion: null,
      canonicalTerminal: null,
    },
  };
}

const GOVERNED_MISSION = governedMissionEvents();

/**
 * The same governed mission, signed over half an hour instead of nine seconds.
 *
 * The Route rail is a map of *distance*: with every step one second apart the
 * roads collapse to dots and the gutter reads as a legend (REVIEW-A4 F11).
 * Three minutes a step gives the creates real separation, puts a compressed
 * silence between the last create and the assignment, and leaves the report
 * a few minutes behind Now — the shape the TeamRolesV1 run actually had.
 */
const ROUTE_MISSION = governedMissionEvents({
  anchorSeconds: GENESIS_CREATED_AT - 1_800,
  spacingSeconds: 180,
  withDecisionRequest: true,
});

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
          // The `completed` branch above publishes an `approve-with-notes`
          // disposition that asks for a follow-up note, plus the builder's
          // acknowledgement of it, so the chain settles by that receipt and
          // never by `approving_disposition_without_ask` — the ask is what
          // rules that rule out. The `blocked` branch publishes neither, so
          // the fold is still awaiting the lead's ruling on a filed report
          // (`coding_session_team_transaction_fold_settlement.rs:408-463`).
          settledBy: state === "completed" ? "acknowledgement" : null,
          awaiting:
            state === "completed"
              ? null
              : {
                  link: "disposition",
                  owedByRole: "lead",
                  owedByActor: null,
                },
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
        schema: BEEKEEPER_CODING_SESSION_METADATA_SCHEMA,
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
        schema: BEEKEEPER_CODING_SESSION_TRANSCRIPT_SCHEMA,
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
  /**
   * Extra signed transcript events, built against the same run clock. Kept as
   * a callback rather than a fixed list because the fixture anchors every
   * timestamp to `Date.now()` and a caller has no way to see that clock.
   */
  extraTranscripts?: (now: number) => RelayEvent[],
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
    ...(extraTranscripts?.(now) ?? []),
  ];
}

/**
 * The three shapes the Audit tab exists to name, as signed builder items: one
 * skill file handed over twice, and `bee sessions operation get` run twice
 * back to back with a byte-identical answer — a room download and a retry loop
 * in the same pair, exactly as the 2026-09-01 run produced them.
 */
function auditEvidenceTranscripts(now: number): RelayEvent[] {
  const body = "S".repeat(512);
  const call = (
    eventSeq: number,
    createdAt: number,
    toolId: string,
    toolName: string,
    input: Record<string, string>,
  ) =>
    signedTranscript({
      createdAt,
      eventSeq,
      item: { kind: "tool_call", tool: { toolName, toolId, input } },
      secret: BUILDER_SECRET,
      target: BUILDER_TARGET,
      turnId: "builder-turn",
    });
  const answer = (
    eventSeq: number,
    createdAt: number,
    toolId: string,
    toolName: string,
    input: Record<string, string>,
    content: string,
  ) =>
    signedTranscript({
      createdAt,
      eventSeq,
      item: {
        kind: "tool_result",
        toolId,
        toolName,
        input,
        content,
        isError: false,
      },
      secret: BUILDER_SECRET,
      target: BUILDER_TARGET,
      turnId: "builder-turn",
    });
  const skill = { path: "skills/lead/SKILL.md" };
  const operation = { command: "bee sessions operation get --id 9f2c" };
  return [
    call(4, now - 27, "skill-1", "Read", skill),
    answer(5, now - 27, "skill-1", "Read", skill, body),
    call(6, now - 26, "skill-2", "Read", skill),
    answer(7, now - 26, "skill-2", "Read", skill, body),
    call(8, now - 25, "op-1", "Bash", operation),
    answer(9, now - 25, "op-1", "Bash", operation, "operation 9f2c"),
    call(10, now - 24, "op-2", "Bash", operation),
    answer(11, now - 24, "op-2", "Bash", operation, "operation 9f2c"),
    // Three in a row: `RETRY_LOOP_MIN` is 3, so two is repetition and only
    // this makes a loop — the same threshold `bee sessions audit` uses.
    call(12, now - 23, "op-3", "Bash", operation),
    answer(13, now - 23, "op-3", "Bash", operation, "operation 9f2c"),
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
  extraTranscripts?: (now: number) => RelayEvent[],
) {
  await page.getByTestId(`channel-${CHANNEL_NAME}`).click();
  await page.evaluate(
    ({ channelName, events }) => {
      const seed = window.__BEEKEEPER_E2E_SEED_MOCK_SIGNED_EVENT__;
      if (!seed) throw new Error("signed-event seeding hook is missing");
      for (const event of events) seed({ channelName, event });
    },
    {
      channelName: CHANNEL_NAME,
      events: missionEvents(governed, builderStatus, extraTranscripts),
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
    /**
     * Run as the umbrella's founder rather than as the default mock viewer.
     *
     * Team-wake delivery is deliberately founder-only: only the founder's
     * Desktop covers for a provider wake, so `deriveCodingSessionTeamWakePlan`
     * returns an empty plan for anyone else
     * (`lib/codingSessionTeamWake.ts:317-321`). A delivery scenario viewed as
     * a stranger therefore has nothing to disclose — correctly, but it cannot
     * test disclosure.
     */
    asFounder?: boolean;
    /** Canonical fold answer for kind:44245, when the scenario needs one. */
    policyFoldResponse?: Record<string, unknown>;
  },
) {
  await page.emulateMedia({ reducedMotion: input.reducedMotion });
  await page.addInitScript(
    ({ theme, founderIdentity }) => {
      window.localStorage.setItem("buzz-theme", theme);
      window.localStorage.setItem("buzz:text-scale", "1.25");
      if (founderIdentity) {
        window.localStorage.setItem(
          "buzz:e2e-identity-override.v1",
          JSON.stringify(founderIdentity),
        );
      }
    },
    {
      theme: input.theme,
      founderIdentity: input.asFounder
        ? {
            privateKey: hex(FOUNDER_SECRET),
            pubkey: FOUNDER,
            username: "tyler",
          }
        : null,
    },
  );
  await installMockBridge(page, {
    codingSessionPolicyFoldResponse: input.policyFoldResponse,
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
  test.setTimeout(90_000); // Multi-surface walkthrough plus a full app restart.
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
  test.setTimeout(90_000); // Three app lifecycles; individual content assertions stay bounded.
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
    // U-E2: the pinned card is gone; the state plane is the Inspector's.
    await expect(
      page.getByTestId("coding-session-mission-transaction-card"),
    ).toHaveCount(0);
    const inspector = page.getByTestId("coding-session-mission-inspector");
    await expect(inspector).toBeVisible({ timeout: 15_000 });
    const missionState = inspector.locator(
      "section:has([data-testid='mission-state-summary'])",
    );
    await expect(missionState).toContainText(scenario.label, {
      timeout: 15_000,
    });
    await expect(missionState).toContainText("Signed source");
    await expect(
      inspector.getByTestId("mission-state-phase-indicator"),
    ).toBeVisible();
    if (scenario.name === "blocked") {
      await expect(missionState).toContainText("Required action:");
    }
    if (scenario.name === "waiting") {
      await expect(missionState).toContainText("Reply to Bob · Builder");
    }
    await waitForAnimations(page);
    await missionState.screenshot({
      path: `${SCREENSHOTS}/mission-${scenario.name}.png`,
    });
  });
}

test("Mission stream stays within its own width at 900px", async ({ page }) => {
  // U-E3: the narrow layout scrolls row 2 and the live strip, never the page.
  await openMockApp(page, { reducedMotion: "no-preference", theme: "buzz" });
  await seedAndOpen(page);
  await page.getByRole("button", { name: "Mission lens" }).click();
  await page.setViewportSize({ width: 900, height: 900 });
  // Below 960 the Inspector is a Sheet over the workspace; U-E3 is about the
  // stream underneath it, so close the drawer before measuring.
  // Wave B (SV-20/24): each surface tab has its own "Close <tab>" button, so
  // the Sheet's own Close is matched exactly.
  const drawerClose = page
    .getByRole("button", { name: "Close", exact: true })
    .last();
  if (await drawerClose.isVisible().catch(() => false)) {
    await drawerClose.click();
  }
  await expect(
    page.getByTestId("coding-session-mission-inspector"),
  ).toHaveCount(0);
  const workspace = page.getByTestId("coding-session-umbrella-workspace");
  await expect(workspace).toBeVisible();
  const timeline = page.getByTestId("coding-session-umbrella-timeline");
  await expect(timeline).toBeVisible({ timeout: 15_000 });
  await expect
    .poll(() =>
      workspace.evaluate(
        (element) => element.scrollWidth <= element.clientWidth,
      ),
    )
    .toBe(true);
  await expect
    .poll(() =>
      page.evaluate(() => document.documentElement.scrollWidth <= innerWidth),
    )
    .toBe(true);
  const participants = page.getByRole("navigation", {
    name: "Session participants",
  });
  await expect(participants).toBeVisible();
  expect(["auto", "scroll"]).toContain(
    await participants.evaluate(
      (element) => getComputedStyle(element).overflowX,
    ),
  );
  await waitForAnimations(page);
  await page.getByTestId("coding-session-narrative-scroll").screenshot({
    path: `${SCREENSHOTS}/stream-flow-narrow.png`,
  });
});

// U-E5 / U-E6 — FINALIZER: enable these by deleting `.skip` once
// `CodingSessionUmbrellaWorkspace.tsx` forwards `missionTransactions`,
// `missionDeliveries`, `resolveMissionActor` and `missionFounderPubkey` to
// `CodingSessionUmbrellaTimelineView`, and `seatAuthorities` / `deliveries` to
// `useCodingSessionMissionSurface` (see REPORT-U.md § Finalizer wiring).
// Everything they need is already exported from the helper module; this lane
// owns neither the workspace mount nor Lane D's evidence hook, so the rows
// cannot reach the DOM here and a green assertion would be a false claim.
/**
 * The governed fixture plus a provider-signed wake for the builder's report,
 * addressed to the **verifier** generation as the lead target, and its
 * `turn_queued` receipt. Signed with the verifier's provider authority, which
 * is what makes it provider evidence under §1b.
 */
function wakeCandidate(sourceEventId: string) {
  return {
    sourceEventId,
    sourceCreatedAtMs: (GENESIS_CREATED_AT + 9) * 1_000,
    sourceEventSeq: null,
    sourceTargetKey: buildCodingSessionTargetKey(BUILDER_TARGET),
    kind: "operation_ready" as const,
    operationType: "report" as const,
    seatRole: "builder",
    causedByCommandId: null,
    preferredCommandId: null,
  };
}

/** The `lead`-role seat the wake is addressed to: create, receipt, metadata. */
function leadSeatEvents(): RelayEvent[] {
  const genesis = GOVERNED_MISSION.events[0];
  const commandId = "mission-seat-lead";
  const createInput = buildCodingSessionCreateEvent({
    channelId: CHANNEL_ID,
    commandId,
    projectRef: null,
    repoRef: null,
    sessionRef: SESSION_REF,
    genesisRef: genesis.id,
    actor: LEAD_ACTOR,
    role: "lead",
    providerInstanceRef: "lead-primary",
    providerAuthorityPubkey: LEAD_PROVIDER,
    model: "opus",
    title: "Portable team loop",
    initialTurn: null,
  });
  const create = finalizeEvent(
    {
      kind: createInput.kind,
      tags: createInput.tags,
      content: createInput.content,
      created_at: GENESIS_CREATED_AT + 10,
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
        session: LEAD_TARGET,
        error: null,
      }),
      created_at: GENESIS_CREATED_AT + 11,
    },
    LEAD_SECRET,
  ) as unknown as RelayEvent;
  return [
    create,
    receipt,
    signedMetadata({
      actor: LEAD_ACTOR,
      model: "opus",
      role: "lead",
      runtime: "claude-agent-acp",
      secret: LEAD_SECRET,
      status: "running",
      target: LEAD_TARGET,
      title: "Portable team loop",
      createdAt: GENESIS_CREATED_AT + 12,
    }),
  ];
}

/**
 * The governed mission with a lead seat, and nothing else added.
 *
 * A team of three is the shape item 9 is about, and the lead seat is what
 * makes it three. Splitting it out from the wake fixture below matters for
 * more than tidiness: the wake and its receipt are signed after every
 * transaction in the fixture, so a stream pinned to its live edge opens past
 * the causality rows — which is the right thing for a wake to do and the
 * wrong thing to ask "is the team state on screen with no clicks?" against.
 */
function governedMissionWithLeadSeat(): typeof GOVERNED_MISSION {
  return {
    events: [...GOVERNED_MISSION.events, ...leadSeatEvents()],
    foldResponse: GOVERNED_MISSION.foldResponse,
  };
}

function governedMissionWithProviderQueuedWake(): typeof GOVERNED_MISSION {
  const base = GOVERNED_MISSION;
  const report = base.events.find(
    (event) =>
      event.kind === KIND_CODING_SESSION_TEAM_TRANSACTION &&
      JSON.parse(event.content).type === "report",
  );
  if (!report) throw new Error("governed fixture is missing its report");
  const commandId = "team-wake-provider-1";
  const wake = signedProviderWakeCommand({
    buildCommandEvent: buildCodingSessionCommandEvent,
    channelId: CHANNEL_ID,
    commandId,
    createdAt: GENESIS_CREATED_AT + 13,
    finalize: (event) =>
      finalizeEvent(event, LEAD_SECRET) as unknown as RelayEvent,
    leadTarget: LEAD_TARGET,
    // Byte-identical to what both producers publish for this operation.
    pointerText: codingSessionTeamWakeText(wakeCandidate(report.id)),
  });
  const queued = signedTurnQueuedReceipt({
    channelId: CHANNEL_ID,
    commandId,
    createdAt: GENESIS_CREATED_AT + 14,
    finalize: (event) =>
      finalizeEvent(event, LEAD_SECRET) as unknown as RelayEvent,
    leadTarget: LEAD_TARGET,
    receiptKind: KIND_CODING_SESSION_LIFECYCLE_RECEIPT,
    receiptSchema: CODING_SESSION_LIFECYCLE_RECEIPT_SCHEMA,
    receiptTagVersion: CODING_SESSION_LIFECYCLE_RECEIPT_TAG_VERSION,
    semanticKey: codingSessionReceiptSemanticKey,
  });
  return {
    events: [...governedMissionWithLeadSeat().events, wake, queued],
    foldResponse: base.foldResponse,
  };
}

test("U-E1: the signed handoff reads as one flow in the stream", async ({
  page,
}) => {
  const governed = governedMissionWithTerminal("completed");
  await openMockApp(page, {
    foldResponse: governed.foldResponse,
    reducedMotion: "no-preference",
    theme: "buzz",
  });
  await seedAndOpen(page, governed);
  await page.getByRole("button", { name: "Mission lens" }).click();
  await assertMissionTransactionFlow(page, { screenshots: SCREENSHOTS });
});

// The fixture itself runs unskipped: it is the half of U-E5 that does not need
// the finalizer's mount, and without it the two exported wake helpers would be
// dead code that nothing ever proves correct.
test("U-E5 fixture: the provider wake and its receipt are real signed events", () => {
  const governed = governedMissionWithProviderQueuedWake();
  const report = GOVERNED_MISSION.events.find(
    (event) =>
      event.kind === KIND_CODING_SESSION_TEAM_TRANSACTION &&
      JSON.parse(event.content).type === "report",
  );
  if (!report) throw new Error("governed fixture is missing its report");
  const added = governed.events.filter(
    (event) => !GOVERNED_MISSION.events.some((prior) => prior.id === event.id),
  );
  // Three lead-seat events (create, receipt, metadata) plus the wake and its
  // receipt. The lead seat is not decoration: a team wake is addressed to the
  // lead's generation, so a fixture with no `lead` role can never produce one.
  expect(added).toHaveLength(5);
  const wake = added[3];
  const queued = added[4];

  // The wake is a real 44220 addressed to the LEAD generation, signed by the
  // lead's provider authority — not the founder, and not the reporter.
  expect(verifyEvent(wake as never)).toBe(true);
  expect(wake.kind).toBe(44220);
  expect(wake.pubkey).toBe(LEAD_PROVIDER);
  expect(wake.pubkey).not.toBe(FOUNDER);
  expect(wake.pubkey).not.toBe(BUILDER_PROVIDER);
  expect(wake.tags.find((tag) => tag[0] === "cs-target")?.[1]).toBe(
    buildCodingSessionTargetKey(LEAD_TARGET),
  );
  const command = JSON.parse(wake.content);
  expect(command.schema).toBe("buzz-coding-session-command/v1");
  expect(command.action.type).toBe("thread.turn.start");
  // Byte-identical to the pointer both producers publish for this operation.
  expect(command.action.text).toBe(
    codingSessionTeamWakeText(wakeCandidate(report.id)),
  );
  expect(JSON.parse(command.action.text).operationId).toBe(report.id);

  // The receipt is a real 44224 `turn_queued` bound to that command id.
  expect(verifyEvent(queued as never)).toBe(true);
  expect(queued.kind).toBe(KIND_CODING_SESSION_LIFECYCLE_RECEIPT);
  expect(queued.pubkey).toBe(LEAD_PROVIDER);
  const receipt = JSON.parse(queued.content);
  expect(receipt.status).toBe("turn_queued");
  expect(receipt.commandId).toBe(command.commandId);
  expect(receipt.session).toEqual(LEAD_TARGET);
  expect(queued.tags.find((tag) => tag[0] === "csl-key")?.[1]).toBe(
    codingSessionReceiptSemanticKey(command.commandId, "turn_queued"),
  );
});

test("item 9: Mission - Live shows the whole team state with no clicks", async ({
  page,
}) => {
  // 1400x900 with the rail open, the shape Brian named. Everything after the
  // lens switch is reading, not driving.
  await page.setViewportSize({ width: 1400, height: 900 });
  const governed = governedMissionWithLeadSeat();
  await openMockApp(page, {
    asFounder: true,
    foldResponse: governed.foldResponse,
    reducedMotion: "no-preference",
    theme: "buzz",
  });
  await seedAndOpen(page, governed);
  await page.getByRole("button", { name: "Mission lens" }).click();
  await assertZeroSwitchObservation(page, {
    expectedSeatCount: 3,
    screenshots: SCREENSHOTS,
  });
});

test("U-E5: a queued provider wake is disclosed on the row, the chip and the rail", async ({
  page,
}) => {
  const governed = governedMissionWithProviderQueuedWake();
  await openMockApp(page, {
    asFounder: true,
    foldResponse: governed.foldResponse,
    reducedMotion: "no-preference",
    theme: "buzz",
  });
  await seedAndOpen(page, governed);
  await page.getByRole("button", { name: "Mission lens" }).click();
  await assertProviderQueuedDelivery(page, {
    reporterChipName: /Bob · Builder/,
    screenshots: SCREENSHOTS,
  });
});

test("U-E6: a created-but-ungranted builder is disclosed, not hidden", async ({
  page,
}) => {
  const governed = governedMissionWithoutBuilderGrant(
    governedMissionWithTerminal("completed"),
    KIND_CODING_SESSION_AUTHORITY_TRANSITION,
  );
  await openMockApp(page, {
    foldResponse: governed.foldResponse,
    reducedMotion: "no-preference",
    theme: "buzz",
  });
  await seedAndOpen(page, governed);
  await page.getByRole("button", { name: "Mission lens" }).click();
  // Both chips: the fixture removes the BUILDER's grant, and the verifier
  // never had one — only the builder is granted in the base fixture. Two
  // ungranted seats is the honest reading of that chain, and the assertion
  // said "one" only because it could not be run.
  const seatBadges = page
    .getByTestId("coding-session-participant-bar")
    .getByTestId("coding-session-seat-authority-badge");
  await expect(seatBadges).toHaveCount(2);
  for (let index = 0; index < 2; index += 1) {
    await expect(seatBadges.nth(index)).toContainText("ungranted");
    await expect(seatBadges.nth(index)).toHaveAttribute(
      "data-kind",
      "created-ungranted",
    );
  }
  await expect(
    page.getByTestId("coding-session-mission-inspector"),
  ).toContainText("bee sessions seat-repair");
  await assertMissionTransactionFlow(page, {
    screenshots: SCREENSHOTS,
    expectUnseated: true,
  });
});

test("Mission remains accessible in dark, narrow, reduced-motion layout", async ({
  page,
}) => {
  await openMockApp(page, {
    reducedMotion: "reduce",
    theme: "buzz-dark",
  });
  await seedAndOpen(page);
  await expect(page.locator("html")).toHaveAttribute(
    "data-beekeeper-theme",
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

test("A3.5: the Audit tab renders this session's own accounting", async ({
  page,
}) => {
  await page.setViewportSize({ width: 1400, height: 900 });
  await openMockApp(page, { reducedMotion: "no-preference", theme: "buzz" });
  await seedAndOpen(
    page,
    GOVERNED_MISSION,
    "running",
    auditEvidenceTranscripts,
  );
  await page.getByRole("button", { name: "Mission lens" }).click();
  await page.getByTestId("coding-session-surface-tab-mission-audit").click();
  const audit = page.getByTestId("coding-session-mission-audit");
  await expect(audit).toBeVisible({ timeout: 15_000 });

  // Per turn: one entry per signed turn, and no invented zero where the driver
  // reported nothing.
  //
  // Edited by lane L4 (REVIEW-L4 F11), disclosed in REPORT-L4: below 400 px of
  // measured width the eight-column table renders as one card per turn — every
  // value still shown, one axis of scroll instead of a sideways scroller
  // nested in a vertical one (L4.6.4). The Inspector's default is 360 px, so
  // this assertion is on the *entries*, whichever shape they took. Moving the
  // Audit to the reading column, where the table is reachable by default, is
  // the recorded follow-on.
  const rows = audit.getByTestId("mission-audit-turn-row");
  const cards = audit.getByTestId("mission-audit-turn-card");
  const entries = (await rows.count()) > 0 ? rows : cards;
  await expect(entries).toHaveCount(2);
  await expect(entries.first()).toContainText("Bob · Builder");
  // The "not reported" carrier survives the degrade: a bare value where the
  // driver reported nothing would be the honesty loss, not the shape change.
  await expect(
    audit.getByTestId("mission-audit-not-reported").first(),
  ).toHaveAttribute("title", "not reported");

  // Totals: per seat, then Σ, with the partial-reporting disclosure.
  await expect(audit.getByTestId("mission-audit-totals-row")).toHaveCount(3);
  // Turn-granular (REVIEW-A3 F2): one disclosure per seat row plus one on Σ.
  const disclosures = audit.getByTestId("mission-audit-partial-disclosure");
  await expect(disclosures).toHaveCount(3);
  await expect(disclosures.last()).toHaveText("(0 of 2 turns reported usage)");

  // The three shapes of waste.
  await expect(audit.getByTestId("mission-audit-handed-twice")).toContainText(
    "skills/lead/SKILL.md",
  );
  // Named exactly as `bee sessions audit` names it: `sessions <verb>`.
  await expect(audit.getByTestId("mission-audit-room-downloads")).toContainText(
    "sessions operation",
  );
  const loops = audit.getByTestId("mission-audit-retry-loops");
  await expect(loops).toContainText("identical results");
  await expect(loops).toContainText("×3");

  // Tall enough that all five sections are inside the rail's own scroller;
  // the per-turn table still scrolls horizontally inside its container.
  await page.setViewportSize({ width: 1400, height: 1600 });
  await waitForAnimations(page);
  await audit.screenshot({ path: `${SCREENSHOTS}/audit-wide.png` });
});

test("A3.2/A3.3: the working block says its W1 word and Mission's header drops the aggregate", async ({
  page,
}) => {
  await page.setViewportSize({ width: 1400, height: 900 });
  await openMockApp(page, { reducedMotion: "no-preference", theme: "buzz" });
  await seedAndOpen(page);

  // Conversation keeps its own agent control (and its aggregate) untouched,
  // and never renders the W1 byline word.
  await expect(
    page.getByTestId("coding-session-agent-focus-trigger"),
  ).toContainText(/agents/i);
  await expect(
    page.getByTestId("coding-session-umbrella-byline-liveness"),
  ).toHaveCount(0);

  await page.getByRole("button", { name: "Mission lens" }).click();
  await expect(
    page.getByTestId("coding-session-mission-inspector"),
  ).toBeVisible({ timeout: 15_000 });
  // A3.3: SURFACES A3/B2 — Mission's badge states the lifecycle word alone.
  // It read `2 agents · 1 working` here, one row above the roster chips that
  // say the same thing per seat, and `IDLE` over a session mid-turn.
  const status = page.getByTestId("coding-session-status-badge");
  await expect(status).toBeVisible();
  await expect(status).not.toContainText(/agents/i);
  await expect(status).not.toHaveAttribute("title", /agents/i);

  // A3.2: exactly one block is working, and it is the one that says `live`.
  const blocks = page.getByTestId("coding-session-umbrella-turn-block");
  const liveness = page.getByTestId("coding-session-umbrella-byline-liveness");
  await expect(liveness).toHaveCount(1);
  await expect(liveness).toHaveText("live");
  const working = blocks.filter({ has: liveness });
  // REVIEW-A3 F7: the animation is on the block's own identity rail, never a
  // ring around the whole card.
  await expect(working).not.toHaveClass(/coding-session-agent-breathe/);
  const rail = working.getByTestId("coding-session-umbrella-turn-rail");
  await expect(rail).toHaveCount(1);
  await expect(rail).toHaveClass(/coding-session-agent-breathe/);
  const settled = blocks.filter({ hasNotText: "live" });
  await expect(settled.first()).not.toHaveClass(/coding-session-agent-breathe/);
  await expect(
    settled.first().getByTestId("coding-session-umbrella-turn-rail"),
  ).toHaveCount(0);
  await waitForAnimations(page);
  await working.screenshot({ path: `${SCREENSHOTS}/turn-block-live.png` });
});

test("A4: the Route rail maps the session, and folds to a scrubber below its width gates", async ({
  page,
}) => {
  await openMockApp(page, {
    foldResponse: ROUTE_MISSION.foldResponse,
    reducedMotion: "no-preference",
    theme: "buzz",
  });
  await seedAndOpen(page, ROUTE_MISSION);
  await page.getByRole("button", { name: "Mission lens" }).click();
  await expect(
    page.getByTestId("coding-session-mission-inspector"),
  ).toBeVisible({ timeout: 15_000 });
  await page.getByTestId("coding-session-surface-close").click();
  // The gate measures the **workspace body**, not the window: the app's own
  // chrome takes ~310 px, so 1400 leaves the body 1089 and the rail folds.
  // This is the first viewport whose body can give the rail 224 px without
  // narrowing the reading column — §9.2's two gates, both of them.
  // `openMockApp` sets 1400 itself, so this has to come after it.
  await page.setViewportSize({ width: 2000, height: 1000 });

  const rail = page.getByTestId("coding-session-route-rail");
  await expect(rail).toBeVisible();
  await expect(page.getByRole("navigation", { name: "Route" })).toHaveCount(1);
  await expect(
    page.getByRole("list", { name: "Route signs, oldest first" }),
  ).toHaveCount(1);
  await expect(page.getByRole("list", { name: "Route roads" })).toHaveCount(1);
  await expect(rail.getByTestId("coding-session-route-now")).toContainText(
    "Now · ",
  );
  // R8: the road heads name the seats with the same words the chips use.
  await expect(rail).toContainText("Bob · Builder");
  await expect(rail).toContainText("Parallax · Verifier");

  const signs = rail.getByTestId("coding-session-route-sign");
  await expect(signs.first()).toBeVisible();
  // F12: assert against kinds that ARE members of the closed set. The stream
  // renders one assignment, one report and one open ruling; the rail must show
  // exactly those three 44244 signs and no refutation, which is a member this
  // fixture never produces — so a stray sign from anywhere else fails here.
  const kinds = await signs.evaluateAll((nodes) =>
    nodes.map((node) => node.getAttribute("data-kind")),
  );
  expect(kinds.filter((kind) => kind === "assignment")).toHaveLength(1);
  expect(kinds.filter((kind) => kind === "report")).toHaveLength(1);
  // B1c: a verb the rail had no glyph for used to throw `Element type is
  // invalid` out of `RouteSignButton` and take the whole Mission tab with it.
  expect(kinds.filter((kind) => kind === "decision.request")).toHaveLength(1);
  expect(kinds.filter((kind) => kind === "refutation")).toHaveLength(0);
  expect(kinds.filter((kind) => kind === "hire")).toHaveLength(2);
  for (const kind of kinds) {
    expect([
      "assignment",
      "report",
      "refutation",
      "disposition",
      "acknowledgement",
      "mission.completed",
      "mission.blocked",
      "note",
      "decision.request",
      "decision.answer",
      "hire",
      "delivery",
      "seat-ungranted",
    ]).toContain(kind);
  }
  // The ruling says its own word on the rail and in the screen-reader list.
  await expect(signs.filter({ hasText: "ruling asked" }).first()).toBeVisible();
  await expect(
    page.getByRole("list", { name: "Route signs, oldest first" }),
  ).toContainText("Ruling asked");
  // §9.4: a gap longer than five minutes compresses to a dashed stretch that
  // carries its own duration.
  const stretches = rail.getByTestId("coding-session-route-stretch");
  await expect(stretches.first()).toBeVisible();
  await expect(stretches.first()).toHaveText(/^· \d+[hms]/);
  // R1/F1: the seat roads now start at their signed creates, so they have
  // length and their caps are filled rather than open.
  await expect(
    rail.getByTestId("coding-session-route-road").first(),
  ).toBeVisible();
  await expect(page.getByRole("list", { name: "Route roads" })).toContainText(
    "road starts since its create",
  );
  // F6: the whole rail is one tab stop.
  const stops = await rail.locator("[tabindex='0']").count();
  expect(stops).toBe(1);
  await waitForAnimations(page);
  await rail.screenshot({ path: `${SCREENSHOTS}/route-wide.png` });

  // §9.6: clicking a sign reveals its stream row — scroll plus the ring.
  const reportSign = signs.filter({ hasText: "report" }).first();
  await reportSign.click();
  await expect(page.locator("[data-highlighted='true']").first()).toBeVisible();
  await expect(reportSign).toHaveAttribute("aria-pressed", "true");

  // §9.6: `j` and `k` walk the signs, and they bind on the rail alone.
  await signs.first().focus();
  await expect(signs.first()).toBeFocused();
  await page.keyboard.press("j");
  await expect(signs.nth(1)).toBeFocused();
  await page.keyboard.press("k");
  await expect(signs.first()).toBeFocused();

  // Below the stream's 420 px floor the map folds to the 40 px scrubber,
  // which still names the attention signs in words rather than going quiet.
  //
  // Edited by lane L4 (REVIEW-L4 F11), disclosed in REPORT-L4: the old gate
  // folded the rail on a 1,280 px *body* and an 816 px reading reserve, an
  // opinion held against a viewer with no handle. L4.6 replaces it with the
  // stream's own floor, so a narrow window alone no longer folds anything —
  // this test closed the Inspector at the top, and with nothing beside it the
  // rail has all the room it needs at any width the app runs at. The fold
  // happens when both panels genuinely cannot fit, so the Inspector is
  // re-opened first: at a 1280 window the body is 969 and
  // 969 − 224 − 360 = 385, under the 420 px floor.
  // SV-20: the header's Inspector toggle left with the other surface
  // toggles; the right-panel toggle reopens the panel on its remembered tab.
  await page.getByTestId("coding-session-panel-toggle-right").click();
  await expect(
    page.getByTestId("coding-session-mission-inspector"),
  ).toBeVisible({ timeout: 15_000 });
  await page.setViewportSize({ width: 1280, height: 900 });
  const scrubber = page.getByTestId("coding-session-route-scrubber");
  await expect(scrubber).toBeVisible();
  await expect(rail).toHaveCount(0);
  await expect(scrubber).toHaveAttribute("aria-label", /^Route — /);
  await expect(scrubber).toHaveAttribute("aria-expanded", "false");
  await waitForAnimations(page);
  // The whole workspace, so the shot shows the folded rail in the layout it
  // folded for rather than a 40 px sliver with no context.
  await page.getByTestId("coding-session-umbrella-workspace").screenshot({
    path: `${SCREENSHOTS}/route-narrow.png`,
  });
});

// ── Batch 3, lane L2 ────────────────────────────────────────────────────────

/**
 * A mission carrying one answered ruling, one open ruling, and the founder's
 * own wake for the answer.
 *
 * Live run 2 is the source for every shape here: an identifier-only wake in a
 * "You" bubble (finding 17), a founder-held request whose `blocks` is empty
 * (finding 16), and a second request the mission is actually waiting on.
 */
const DECISION_MISSION = governedMissionEvents({
  withDecisionRequest: true,
  withDecisionAnswer: true,
});

/** One signed 44244 of the given `cstx-type` from a fixture's events. */
function transactionOfType(
  fixture: typeof GOVERNED_MISSION,
  type: string,
): RelayEvent {
  const found = fixture.events.filter(
    (event) =>
      event.kind === KIND_CODING_SESSION_TEAM_TRANSACTION &&
      event.tags.some((tag) => tag[0] === "cstx-type" && tag[1] === type),
  );
  if (found.length === 0) throw new Error(`fixture has no ${type}`);
  return found[0];
}

/**
 * The founder's wake for an operation, as a signed transcript prompt.
 *
 * Byte-identical to what the CLI put on the wire at 10:42 in live run 2: the
 * whole turn text is the pointer, and the operator stamp is the founder's own
 * key — which is why it rendered as raw JSON in a "You" bubble.
 */
function wakePromptTranscript(input: {
  createdAt: number;
  operationId: string;
  type: string;
}): RelayEvent {
  return signedTranscript({
    createdAt: input.createdAt,
    eventSeq: 4,
    item: {
      kind: "user_prompt",
      content: JSON.stringify({
        operationId: input.operationId,
        type: input.type,
      }),
      operatorPubkey: FOUNDER,
    },
    secret: BUILDER_SECRET,
    target: BUILDER_TARGET,
    turnId: "builder-turn",
  });
}

/** A folded policy: one record in force, one stranger's record refused. */
function policyFoldResponse(): Record<string, unknown> {
  return {
    schema: "buzz-coding-session-policy-fold-adapter/v1",
    implementation: "buzz-core",
    selected: {
      eventId: "a5956d50".repeat(8),
      authorPubkey: FOUNDER,
      authorIsFounder: true,
      createdAt: GENESIS_CREATED_AT,
      record: {
        sessionRef: SESSION_REF,
        genesisRef: "ce5d87ed".repeat(8),
        posture: "ship",
        budget: {
          turns: 40,
          tokensPerSeat: null,
          tokensPerSession: null,
          costUsdPerSession: 25,
          contextTier: null,
        },
        attention: "decisions",
        gates: null,
        bench: null,
        irreversible: null,
        stop: null,
        setsAnyPolicy: true,
      },
    },
    excluded: [
      {
        eventId: "c3".repeat(32),
        authorPubkey: VERIFIER_ACTOR,
        createdAt: GENESIS_CREATED_AT + 5,
        code: "unauthorized",
        reason:
          "signed by an identity that could not steer this umbrella when it was published",
      },
    ],
    // L5.5 / REVIEW-L2 F15: the native fold now also discloses the claimed
    // authority grants it refused because no verified kind-44228 supported
    // them. Required, not optional — an adapter that stopped disclosing it
    // would read exactly like one where nothing was refused — so this fixture
    // learns the key. **The only line L5 changed in this file.**
    refusedGrants: [],
    enforcement:
      "a published policy is a stated intention, not an enforced limit: only budget.turns is enforced (at the provider's turn gate); every other field is read and shown, never counted",
  };
}

test("L2: an identifier-only wake reads as one line in both lenses", async ({
  page,
}) => {
  const answer = transactionOfType(DECISION_MISSION, "decision.answer");
  const request = transactionOfType(DECISION_MISSION, "decision.request");
  await openMockApp(page, {
    asFounder: true,
    foldResponse: DECISION_MISSION.foldResponse,
    reducedMotion: "no-preference",
    theme: "buzz",
  });
  await seedAndOpen(page, DECISION_MISSION, "running", (now) => [
    wakePromptTranscript({
      createdAt: now - 27,
      operationId: answer.id,
      type: "decision.answer",
    }),
  ]);

  // Conversation first — the lens finding 17 was reported against. It holds no
  // fold, so §1f's unresolved line is the honest reading; what it is NOT is
  // the pointer's own JSON.
  const bubble = page.getByTestId("coding-session-user-message-wake");
  await expect(bubble).toHaveText(
    `You sent a wake for operation ${answer.id.slice(0, 8)} — this lens holds no session records; open Mission to read it.`,
  );
  await expect(page.getByText(`{"operationId"`)).toHaveCount(0);
  // The stream is long; without this the shot is of whatever the scroller
  // settled on rather than of the row the test is about.
  // REVIEW-L2 F14: this scenario deliberately writes **no** PNG.
  //
  // Three captures were tried — the whole workspace, the bubble's row, and the
  // line element itself, scrolled to centre and then to top. Every one came
  // back with a floating element over the subject: this fixture's stream keeps
  // a sticky provenance header at the top of the scroller and a provider
  // notice plus the composer dock at the bottom, and `locator.screenshot()`
  // captures the page region, not the element in isolation. A PNG whose
  // filename claims a line it does not contain is worse than no PNG, so the
  // evidence for this scenario is the assertion above and the masked DOM
  // dumps in `batch3/baseline-1dd98e876/`, which carry the rendered line
  // verbatim.

  // Mission holds the fold, so the same module resolves the operation and the
  // line names the **request**, never the answer's own id.
  await page.getByTestId("coding-session-lens-mission").click();
  await expect(
    page.getByTestId("coding-session-mission-inspector"),
  ).toBeVisible({ timeout: 15_000 });
  const missionBubble = page.getByTestId("coding-session-user-message-wake");
  await expect(missionBubble).toHaveText(
    `You answered decision ${request.id.slice(0, 8)}: Land the inspector now`,
  );
  // No PNG here either, for the reason given above.
});

test("L2: the rail says who is waiting and lists every ruling", async ({
  page,
}) => {
  const request = transactionOfType(DECISION_MISSION, "decision.request");
  const assignment = transactionOfType(DECISION_MISSION, "assignment");
  await openMockApp(page, {
    asFounder: true,
    foldResponse: DECISION_MISSION.foldResponse,
    reducedMotion: "no-preference",
    theme: "buzz",
  });
  await seedAndOpen(page, DECISION_MISSION);
  await page.getByTestId("coding-session-lens-mission").click();
  const inspector = page.getByTestId("coding-session-mission-inspector");
  await expect(inspector).toBeVisible({ timeout: 15_000 });

  // No lead seat holds an open turn in this fixture, so waiting IS the state.
  // `asFounder: true` makes the viewer's own identity the umbrella's founder
  // (see the option's own doc comment above), so once L8 wired the surface's
  // real `currentUserPubkey` through, `heldOnLabel` correctly reads every one
  // of these as first person rather than naming the founder in the third
  // person — the viewer IS the founder in this fixture.
  const state = page.getByTestId("mission-state-summary");
  await expect(state).toHaveAttribute("data-mission-waiting", "state-line");
  await expect(state).toContainText("Waiting on you");

  const rows = page.getByTestId("mission-decision-row");
  await expect(rows).toHaveCount(2);
  // Open first, then the answered one.
  await expect(rows.first()).toHaveAttribute("data-decision-state", "open");
  await expect(rows.first()).toContainText("Open · held on you");
  await expect(rows.first()).toContainText(
    `holds up 1 assignment: ${assignment.id.slice(0, 8)}`,
  );
  await expect(rows.last()).toHaveAttribute("data-decision-state", "answered");
  await expect(rows.last()).toContainText("Answered by you");
  // `blocks: []` is a real answer from the fold, never a blank.
  await expect(rows.last()).toContainText("holds up no assignment yet");
  await expect(rows.last()).toContainText(request.id.slice(0, 8));
  // The rail scrolls, so the shot is of the section itself — a panel-sized
  // capture put the queue below the fold and showed nothing it is about.
  const queueSection = page.locator(
    'section:has([data-testid="mission-decision-queue"])',
  );
  await queueSection.scrollIntoViewIfNeeded();
  await waitForAnimations(page);
  await queueSection.screenshot({
    path: `${SCREENSHOTS}/decision-queue.png`,
  });
});

test("L2: the Context tab renders the folded policy and what it refused", async ({
  page,
}) => {
  await openMockApp(page, {
    asFounder: true,
    foldResponse: DECISION_MISSION.foldResponse,
    policyFoldResponse: policyFoldResponse(),
    reducedMotion: "no-preference",
    theme: "buzz",
  });
  await seedAndOpen(page, DECISION_MISSION);
  await page.getByTestId("coding-session-lens-mission").click();
  await expect(
    page.getByTestId("coding-session-mission-inspector"),
  ).toBeVisible({ timeout: 15_000 });
  await page.getByTestId("coding-session-surface-tab-mission-context").click();
  const policy = page.getByTestId("mission-session-policy");
  await expect(policy).toBeVisible({ timeout: 15_000 });
  await expect(policy).toHaveAttribute("data-policy-state", "record");
  await expect(policy).toContainText("only budget.turns is enforced");
  // The one enforced field says so; a cost ceiling nothing counts does not,
  // and gets no bar of any kind.
  await expect(
    policy.locator('[data-policy-field="budget.turns"]'),
  ).toHaveAttribute("data-policy-enforced", "yes");
  await expect(
    policy.locator('[data-policy-field="budget.costUsdPerSession"]'),
  ).toHaveAttribute("data-policy-enforced", "no");
  await expect(
    page.getByTestId("mission-session-policy-refused"),
  ).toContainText("unauthorized");
  const policySection = page.locator(
    'section:has([data-testid="mission-session-policy"])',
  );
  await policySection.scrollIntoViewIfNeeded();
  await waitForAnimations(page);
  await policySection.screenshot({
    path: `${SCREENSHOTS}/context-session-policy.png`,
  });
});
