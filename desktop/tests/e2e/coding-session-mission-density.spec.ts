import { mkdirSync } from "node:fs";

import { expect, test, type Page } from "@playwright/test";
import {
  expectConversationColumnUnchanged,
  expectMissionColumnUncapped,
  expectStreamFillsTheRails,
  measureCodingSessionMissionGrid,
} from "./helpers/codingSessionMissionDensityAssertions";
import { finalizeEvent, getPublicKey } from "nostr-tools/pure";

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
const _LEAD_PROVIDER = getPublicKey(LEAD_SECRET);
const LEAD_ACTOR_SECRET = hexToBytes(
  "7777777777777777777777777777777777777777777777777777777777777777",
);
const _LEAD_ACTOR = getPublicKey(LEAD_ACTOR_SECRET);
const _LEAD_TARGET: CodingSessionCommandTarget = {
  driver: "claude-agent-acp",
  instanceId: "lead-instance",
  sessionId: "aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee",
  generation: 1,
};
const _SCREENSHOTS = "test-results/singularity-lens";
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
      // `hasExactFields` again, and the same trap as `notes`/`decisions`
      // above: the decoder grew `pendingCompletion` (ledger 183(a)/(b)) and no
      // e2e fixture grew with it, so every mocked team fold decoded as
      // `native coding-session team fold returned a malformed response` and
      // the whole Mission tab rendered its unknown state. The unit tests in
      // `desktop/src` carry the field and stayed green, because
      // `desktop/tsconfig.json` includes only `src` — nothing typechecks this
      // file. `null` is the real answer: no fixture here publishes a
      // `mission.completed`, late or otherwise.
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
const _ROUTE_MISSION = governedMissionEvents({
  anchorSeconds: GENESIS_CREATED_AT - 1_800,
  spacingSeconds: 180,
  withDecisionRequest: true,
});

const _UNGOVERNED_MISSION = {
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

function _governedMissionWithTerminal(
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
function _auditEvidenceTranscripts(now: number): RelayEvent[] {
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
    /** Window width for the grid arithmetic; the height never varies. */
    viewportWidth?: number;
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
  await page.setViewportSize({
    width: input.viewportWidth ?? 1400,
    height: 900,
  });
  await page.goto("/");
}
// ── Batch 3, lane L2 ────────────────────────────────────────────────────────

/**
 * A mission carrying one answered ruling, one open ruling, and the founder's
 * own wake for the answer.
 *
 * Live run 2 is the source for every shape here: an identifier-only wake in a
 * "You" bubble (finding 17), a founder-held request whose `blocks` is empty
 * (finding 16), and a second request the mission is actually waiting on.
 */
const _DECISION_MISSION = governedMissionEvents({
  withDecisionRequest: true,
  withDecisionAnswer: true,
});

/** One signed 44244 of the given `cstx-type` from a fixture's events. */
function _transactionOfType(
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
function _wakePromptTranscript(input: {
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

// ── Batch 3, lane L4 — the Mission grid, and the rails a viewer owns ────────

const SHOTS = "test-results/mission-density";

function shot(name: string) {
  mkdirSync(SHOTS, { recursive: true });
  return `${SHOTS}/${name}.png`;
}

async function openMission(page: Page, viewportWidth: number) {
  await openMockApp(page, {
    asFounder: true,
    reducedMotion: "reduce",
    theme: "buzz",
    viewportWidth,
  });
  await seedAndOpen(page);
  await page.getByTestId("coding-session-lens-mission").click();
  await expect(
    page.getByTestId("coding-session-mission-inspector"),
  ).toBeVisible({ timeout: 15_000 });
  await waitForAnimations(page);
}

/**
 * B2, the whole of it: at 1920 the stream box was 1026 px and the column 768,
 * so 258 px of a person's window went to two symmetric margins — and closing
 * the Inspector raised the cap by 384 rather than handing the space over.
 *
 * The arithmetic is the acceptance test, so it is computed here rather than
 * eyeballed: at each width the stream must equal the body less both rails, and
 * the reading box must be the whole stream.
 */
for (const width of [1100, 1280, 1600, 1920]) {
  test(`Mission gives the stream what the rails leave at ${width}`, async ({
    page,
  }) => {
    await openMission(page, width);
    const grid = await expectStreamFillsTheRails(page, `window ${width}`);
    await expectMissionColumnUncapped(page);
    // The rail folds on the stream's own floor and on nothing else, so a
    // drawn rail always leaves at least that much stream behind it.
    expect(
      grid.stream,
      `window ${width}: the stream floor holds`,
    ).toBeGreaterThanOrEqual(grid.route > 40 ? 420 : 0);
    // eslint-disable-next-line no-console -- the four numbers are the report.
    console.log(
      `GRID ${width}: body=${Math.round(grid.body)} route=${Math.round(grid.route)} inspector=${Math.round(grid.inspector)} stream=${Math.round(grid.stream)} column=${Math.round(grid.column)}`,
    );
    await page.screenshot({ path: shot(`grid-${width}`) });
  });
}

test("the route rail has the handle and the collapse the Inspector has", async ({
  page,
}) => {
  // B1: two panels on one screen, one of which the viewer owns and one of
  // which owns the viewer. The rail was a hard `w-56` behind a width gate
  // nothing a person did could influence.
  await openMission(page, 1920);
  const handle = page.getByTestId("coding-session-route-resize");
  await expect(handle).toHaveAttribute("role", "separator");
  await expect(handle).toHaveAttribute("aria-valuemin", "176");
  await expect(handle).toHaveAttribute("aria-valuemax", "480");
  const before = Number(await handle.getAttribute("aria-valuenow"));
  expect(before).toBe(224);

  // Keyboard-operable, and the rail is on the left, so ArrowRight widens it.
  await handle.focus();
  for (let step = 0; step < 16; step += 1) {
    await handle.press("ArrowRight");
  }
  const widened = Number(await handle.getAttribute("aria-valuenow"));
  expect(widened).toBe(480);
  const grid = await expectStreamFillsTheRails(page, "rail widened to 480");
  expect(Math.round(grid.route)).toBe(480);
  // B5: at 480 px a sign is no longer clipped to `hire · Bob · Buil…`.
  const sign = page.getByTestId("coding-session-route-sign").first();
  await expect(sign).toBeVisible();
  const clipped = await sign.evaluate(
    (node) => node.scrollWidth > node.clientWidth + 1,
  );
  expect(clipped, "a 480 px rail shows its signs whole").toBe(false);
  await waitForAnimations(page);
  await page.screenshot({ path: shot("route-wide-480") });

  // Persisted per viewer — and restored **into the layout**, which is the
  // claim. REVIEW-L4 F15: asserting the localStorage byte proved the write and
  // not the read; a stored width nothing lays out is a preference that does
  // nothing.
  await page.reload();
  // Mock relay storage is page-local; restore it without resetting viewer preferences.
  await seedAndOpen(page);
  await expect(
    page.getByTestId("coding-session-umbrella-workspace"),
  ).toBeVisible({ timeout: 15_000 });
  await page.getByTestId("coding-session-lens-mission").click();
  await expect(
    page.getByTestId("coding-session-mission-inspector"),
  ).toBeVisible({ timeout: 15_000 });
  await waitForAnimations(page);
  const restored = await measureCodingSessionMissionGrid(page);
  expect(
    Math.round(restored.route),
    "the rail comes back at the width the viewer dragged it to",
  ).toBe(480);
  await expect(page.getByTestId("coding-session-route-resize")).toHaveAttribute(
    "aria-valuenow",
    "480",
  );
});

test("collapsing the rail leaves the 40 px scrubber, and says so", async ({
  page,
}) => {
  await openMission(page, 1920);
  const toggle = page.getByTestId("coding-session-route-toggle");
  await expect(toggle).toHaveAttribute("aria-expanded", "true");
  await toggle.click();
  await expect(page.getByTestId("coding-session-route-rail")).toHaveCount(0);
  const scrubber = page.getByTestId("coding-session-route-scrubber");
  await expect(scrubber).toBeVisible();
  await expect(toggle).toHaveAttribute("aria-expanded", "false");
  const grid = await expectStreamFillsTheRails(page, "rail collapsed");
  // Collapsed is the existing scrubber; nothing new is drawn. Its `w-10` is
  // 2.5rem, not 40 px — this harness runs at a 1.25 text scale, and the rail
  // is measured against the root font size rather than a frozen pixel count,
  // which is exactly what the rem rule is for.
  const rem = await page.evaluate(() =>
    Number.parseFloat(
      window.getComputedStyle(document.documentElement).fontSize,
    ),
  );
  expect(Math.round(grid.route)).toBe(Math.round(2.5 * rem));
  expect(
    await page.evaluate(() =>
      window.localStorage.getItem(
        "buzz.desktop.coding-session-route-collapsed",
      ),
    ),
  ).toBe("1");
  await waitForAnimations(page);
  await page.screenshot({ path: shot("route-collapsed") });

  // And back, from the same control.
  await toggle.click();
  await expect(page.getByTestId("coding-session-route-rail")).toBeVisible();
});

test("A7: the header's six actions are one menu, and Stop all says what it stops", async ({
  page,
}) => {
  await openMission(page, 1920);
  // A6: `Stop all` no longer sits beside `Close`, same size, same variant.
  await expect(page.getByTestId("coding-session-stop-all")).toHaveCount(0);
  await expect(page.getByTestId("coding-session-close")).toHaveCount(0);
  const overflow = page.getByTestId("coding-session-overflow");
  await expect(overflow).toHaveCount(1);
  await overflow.click();
  await waitForAnimations(page);
  const items = page.locator('[data-testid^="coding-session-overflow-"]');
  const labels = await items.allTextContents();
  expect(labels.length).toBeGreaterThan(0);
  // The order A7 fixes, over whichever of the six this session offers —
  // preceded by the workspace item, which creates rather than ends and so
  // sits above the destructive run.
  const order = [
    "New session in this workspace",
    "Add provider…",
    "Stop all",
    "Close session",
    "Reopen session",
    "Export transcript",
    "Pop out",
  ];
  const seen = labels.map(
    (text) => order.find((label) => text.startsWith(label)) ?? text,
  );
  expect(seen).toEqual(order.filter((label) => seen.includes(label)));
  await page.screenshot({ path: shot("header-overflow") });
  await page.keyboard.press("Escape");
});

test("the Audit table gets the reading column, and cards when it cannot", async ({
  page,
}) => {
  // B7: eight declared columns rendered four inside a 300 px rail while 258 px
  // of the stream's margin sat empty.
  await openMission(page, 1920);
  await page.getByTestId("coding-session-surface-tab-mission-audit").click();
  // At the Inspector's default 360 px the eight declared columns cannot be
  // read, and the old answer was a sideways scroller nested inside the Audit
  // tab's vertical one. Now the same eight rows are one card per turn: every
  // value still shown, one axis of scroll.
  await expect(page.getByTestId("mission-audit-per-turn-cards")).toBeVisible();
  await expect(page.getByTestId("mission-audit-per-turn")).toHaveCount(0);
  const cards = page.getByTestId("mission-audit-turn-card");
  await expect(cards.first()).toContainText("Cache reads");
  await expect(cards.first()).toContainText("Context window");
  await waitForAnimations(page);
  await page.screenshot({ path: shot("audit-cards") });

  // And a reader who wants the table can now ask for it: the Inspector's own
  // separator widens the rail past the floor and the eight columns come back.
  const handle = page.getByTestId("coding-session-surface-resize");
  await handle.focus();
  for (let step = 0; step < 12; step += 1) {
    await handle.press("ArrowLeft");
  }
  await expect(page.getByTestId("mission-audit-per-turn")).toBeVisible();
  await waitForAnimations(page);
  await page.screenshot({ path: shot("audit-in-column") });
});

test("the Inspector carries the goal reader and the open holds at every width", async ({
  page,
}) => {
  await openMission(page, 1920);
  const inspector = page.getByTestId("coding-session-mission-inspector");
  await expect(inspector).toBeVisible();
  // A3: nothing on this surface claims the wire is silent about tests. What
  // is asserted here is that the sentence L1 falsified is gone from the
  // build; the four-state empty copy itself is proved in
  // `CodingSessionMissionInspector.test.mjs`.
  //
  // This comment used to say the fixture publishes a signed test report and
  // that no refusal is therefore on screen. Neither half was true: the
  // fixture configures no observation fold, so the gate rows carry the mock's
  // own refusal instead, and the two absences below were holding over a
  // different sentence than the one they were written for. Naming that
  // refusal here means the day it changes, this test says so rather than
  // going on passing for a reason nobody checked.
  const gateRows = inspector
    .locator("section", { hasText: "Structured tests" })
    .first();
  await expect(gateRows).toContainText(
    "mock session-observation fold response is not configured",
  );
  await expect(inspector).not.toContainText(
    "Nothing on the wire reports tests",
  );
  await expect(inspector).not.toContainText("Beekeeper will not count it");
  // A5: the hold is on the roster that survives every width, and it names the
  // waiter and the clock the rail head never did.
  await expect(inspector).toContainText("waits on");
  const hold = inspector.getByTestId("mission-open-hold").first();
  await expect(hold).toContainText("· since ");
  // F2: the flagship line names the party that owes the answer. The founder
  // holds no seat, so resolving the holder against the roster alone printed
  // `holder not resolved` over the person reading the screen.
  await expect(inspector).not.toContainText("holder not resolved");
  await expect(hold).toContainText("waits on you");
  // F8: the hold sat 237 px below the bottom of the Inspector's own box, so
  // the PNG named as A5's evidence showed everything except the holds. Scroll
  // it into frame and shoot the panel, not the page.
  await hold.scrollIntoViewIfNeeded();
  await waitForAnimations(page);
  await expect(hold).toBeInViewport();
  await inspector.screenshot({ path: shot("inspector-open-holds") });

  // A5's point: the roster that carries the hold is the one that survives the
  // width at which the rail folds away.
  await page.setViewportSize({ width: 1280, height: 900 });
  await waitForAnimations(page);
  await expect(inspector).toBeVisible();
  await expectStreamFillsTheRails(page, "1280 with the Inspector open");
});

test("Conversation keeps its centred column, byte for byte", async ({
  page,
}) => {
  // I8: every item in this lane is Mission-gated. This is the continuous
  // check; the masked outerHTML diff against the base SHA is the exhaustive
  // one, and it is empty.
  await openMockApp(page, {
    asFounder: true,
    reducedMotion: "reduce",
    theme: "buzz",
    viewportWidth: 1920,
  });
  await seedAndOpen(page);
  await expectConversationColumnUnchanged(page);
  const grid = await measureCodingSessionMissionGrid(page);
  expect(grid.route, "Conversation mounts no rail at all").toBe(0);
  // Conversation's reading box is a capped, centred column inside the stream —
  // exactly what it has always been.
  expect(grid.column).toBeLessThan(grid.columnAvailable);
});

/**
 * L4.7 / critique B4 — the composer stops owning a third of the window.
 *
 * Three numbers, all read off the laid-out page rather than off a class name:
 * the editor opens at one line in Mission and four in Conversation; the
 * stream's bottom reserve is the dock's *measured* height rather than the
 * constant `pb-48` that drifted out of register with it; and no dock pixel
 * lands on the last row's box, which is the failure `zero-switch-wide.png`
 * caught — the `Add provider` button sitting on a turn block because the
 * unreachable notice grew the dock and the reserve did not move.
 */
test("B4: the Mission editor opens at one line and the reserve is the dock", async ({
  page,
}) => {
  await openMission(page, 1920);
  const editor = page.getByLabel("Coding-session instruction");
  await expect(editor).toBeVisible();
  const editorMinHeight = await editor.evaluate(
    (node) => window.getComputedStyle(node).minHeight,
  );
  // `min-h-11` is 2.75rem, and the app's root font-size carries Cmd +/- zoom,
  // so the assertion is on rem-scaled px rather than a literal 44.
  const rootFontSize = await page.evaluate(() =>
    Number.parseFloat(
      window.getComputedStyle(document.documentElement).fontSize,
    ),
  );
  expect(
    Number.parseFloat(editorMinHeight),
    "Mission's editor opens at one line (min-h-11 = 2.75rem)",
  ).toBeCloseTo(2.75 * rootFontSize, 0);

  const dock = page.getByTestId("coding-session-composer-dock");
  await expect(dock).toBeVisible();
  const reserve = await page.evaluate(() => {
    const column = document.querySelector(
      "[data-coding-session-column-mission]",
    );
    const dockNode = document.querySelector(
      '[data-testid="coding-session-composer-dock"]',
    );
    if (!column || !dockNode) return null;
    return {
      paddingBottom: Number.parseFloat(
        window.getComputedStyle(column).paddingBottom,
      ),
      dockHeight: dockNode.getBoundingClientRect().height,
    };
  });
  expect(reserve, "both the column and the dock are laid out").not.toBeNull();
  // Plus the dock's `before:h-8` fade (2rem), which paints over the stream
  // just above the dock: a reserve that stopped at the dock's edge left the
  // last row under the fade.
  expect(
    Math.abs(
      (reserve?.paddingBottom ?? 0) -
        (reserve?.dockHeight ?? 0) -
        2 * rootFontSize,
    ),
    `the reserve is the dock's measured height plus its fade (padding ${reserve?.paddingBottom} vs dock ${reserve?.dockHeight})`,
  ).toBeLessThanOrEqual(2);
  // The reserve is a measurement, so it is never the old constant by accident.
  expect(reserve?.paddingBottom ?? 0).toBeGreaterThan(0);

  // And nothing the dock draws lands on the newest row. Measured at the foot
  // of the scroller, which is the only place the two can meet: the reserve is
  // padding *under* the last row, so an unscrolled stream proves nothing.
  await page.evaluate(() => {
    const scroller = document.querySelector(
      '[data-testid="coding-session-narrative-scroll"]',
    );
    if (scroller) scroller.scrollTop = scroller.scrollHeight;
  });
  await waitForAnimations(page);
  const overlap = await page.evaluate(() => {
    const dockNode = document
      .querySelector('[data-testid="coding-session-composer-dock"]')
      ?.getBoundingClientRect();
    const rows = document.querySelectorAll(
      '[data-testid="coding-session-umbrella-timeline"] > *',
    );
    const last = rows[rows.length - 1]?.getBoundingClientRect();
    if (!dockNode || !last) return null;
    return last.bottom - dockNode.top;
  });
  expect(
    overlap ?? 0,
    "the last row ends above the dock rather than under it",
  ).toBeLessThanOrEqual(2);
});

test("B4: Conversation opens at min-h-16, and its reserve is the dock too", async ({
  page,
}) => {
  await openMockApp(page, {
    asFounder: true,
    reducedMotion: "reduce",
    theme: "buzz",
    viewportWidth: 1920,
  });
  await seedAndOpen(page);
  const editor = page.getByLabel("Coding-session instruction");
  await expect(editor).toBeVisible();
  const classes = (await editor.getAttribute("class")) ?? "";
  expect(
    classes,
    "Conversation's editor opens compact, at about two lines",
  ).toMatch(/\bmin-h-16\b/);
  expect(classes).not.toMatch(/\bmin-h-11\b/);
  // Conversation used to keep the literal `pb-48` (`pb-[34rem]` while a seat
  // worked). That literal is what Andy's 2026-09-29 screenshots show: the
  // composer over the session's last row, and hundreds of px opening up when
  // a reply started. One number now, in both lenses.
  const column = page
    .getByTestId("coding-session-narrative-scroll")
    .locator("[data-coding-session-column]")
    .first();
  expect((await column.getAttribute("class")) ?? "").not.toMatch(
    /\bpb-(48|\[34rem\])/,
  );
  const foot = await measureStreamFoot(page);
  expect(
    Math.abs(foot.paddingBottom - foot.dockHeight - foot.fade),
    `the reserve is the dock plus its fade (padding ${foot.paddingBottom}, dock ${foot.dockHeight}, fade ${foot.fade})`,
  ).toBeLessThanOrEqual(2);
});

/**
 * Andy, 2026-09-29 (hive DM, three screenshots): in Conversation the composer
 * covered the bottom of the session, the `Reply to … / Send to…` chips sat on
 * the disconnected notice, and a lot of space opened when a reply started.
 *
 * All three were one wrong number — the literal reserve against a dock whose
 * height moves with the disconnected notice, the active-work strip and the
 * task rail. Read off the laid-out page, at the foot of the scroller: the last
 * row clears the dock *and* its fade, by the same amount in every state.
 */
for (const builderStatus of [
  "running",
  "waiting_for_input",
  "disconnected",
] as const) {
  test(`the stream's last row clears the dock by one constant gap (${builderStatus})`, async ({
    page,
  }) => {
    await openMockApp(page, {
      asFounder: true,
      reducedMotion: "reduce",
      theme: "buzz",
      viewportWidth: 1920,
    });
    await seedAndOpen(page, GOVERNED_MISSION, builderStatus);
    await waitForAnimations(page);
    const foot = await measureStreamFoot(page);
    expect(
      foot.distance,
      "a session opens at its latest content",
    ).toBeLessThanOrEqual(2);
    expect(
      Math.abs(foot.paddingBottom - foot.dockHeight - foot.fade),
      `the reserve is the dock plus its fade (padding ${foot.paddingBottom}, dock ${foot.dockHeight}, fade ${foot.fade})`,
    ).toBeLessThanOrEqual(2);
    expect(
      foot.lastRowBottom,
      `the last row ends above the dock's fade (row ${foot.lastRowBottom}, fade top ${foot.dockTop - foot.fade})`,
    ).toBeLessThanOrEqual(foot.dockTop - foot.fade + 2);
  });
}

/**
 * Andy's third screenshot: back on a session tab, the view stopped ~200 px
 * short of the bottom. The router restores a scroller's absolute `scrollTop`,
 * and on a remount the reserve under the stream is not yet what it was when
 * that number was taken. The workspace keeps distance-from-bottom instead.
 */
test("a return to the session keeps its distance from the bottom", async ({
  page,
}) => {
  await openMockApp(page, {
    asFounder: true,
    reducedMotion: "reduce",
    theme: "buzz",
    viewportWidth: 1920,
  });
  await seedAndOpen(page);
  await waitForAnimations(page);
  const scroller = page.getByTestId("coding-session-narrative-scroll");

  // At the latest: back and forward land at the latest again.
  await scroller.evaluate((node) => {
    node.scrollTop = node.scrollHeight;
  });
  await page.waitForTimeout(1_200);
  await page.goBack();
  await expect(
    page.getByTestId("coding-session-umbrella-workspace"),
  ).toHaveCount(0);
  await page.goForward();
  await expect(
    page.getByTestId("coding-session-umbrella-workspace"),
  ).toBeVisible();
  await waitForAnimations(page);
  await page.waitForTimeout(300);
  expect(
    (await measureStreamFoot(page)).distance,
    "back at the latest after a return",
  ).toBeLessThanOrEqual(2);

  // Scrolled up by the reader: the same distance from the bottom, not the
  // same scrollTop.
  // Near the scroller's top: the dock overlays everything below the stream.
  const box = await scroller.boundingBox();
  if (!box) throw new Error("narrative scroller is not laid out");
  await page.mouse.move(box.x + box.width / 2, box.y + 40);
  await page.mouse.wheel(0, -160);
  await expect
    .poll(async () => (await measureStreamFoot(page)).distance)
    .toBeGreaterThan(100);
  await page.waitForTimeout(400);
  const before = (await measureStreamFoot(page)).distance;
  await page.goBack();
  await expect(
    page.getByTestId("coding-session-umbrella-workspace"),
  ).toHaveCount(0);
  await page.goForward();
  await expect(
    page.getByTestId("coding-session-umbrella-workspace"),
  ).toBeVisible();
  await waitForAnimations(page);
  await page.waitForTimeout(300);
  const after = (await measureStreamFoot(page)).distance;
  expect(
    Math.abs(after - before),
    `distance from the bottom survives the return (${before} → ${after})`,
  ).toBeLessThanOrEqual(2);
});

/**
 * The stream's foot, read off the laid-out page — after two frames. Scroll
 * events and ResizeObserver callbacks are delivered at the rendering step, so
 * a read taken between React's commit and the next frame sees rows that have
 * grown under an anchor that has not been told yet: a state no reader ever
 * sees on screen. `waitForAnimations` awaits animations, never a frame.
 */
async function measureStreamFoot(page: Page) {
  await page.evaluate(
    () =>
      new Promise<void>((resolve) =>
        requestAnimationFrame(() => requestAnimationFrame(() => resolve())),
      ),
  );
  const foot = await page.evaluate(() => {
    const scroller = document.querySelector<HTMLElement>(
      '[data-testid="coding-session-narrative-scroll"]',
    );
    const column = scroller?.querySelector<HTMLElement>(
      "[data-coding-session-column]",
    );
    const dock = document.querySelector<HTMLElement>(
      '[data-testid="coding-session-composer-dock"]',
    );
    const rows = document.querySelectorAll(
      '[data-testid="coding-session-umbrella-timeline"] > *',
    );
    const last = rows[rows.length - 1];
    if (!scroller || !column || !dock || !last) return null;
    const rootFontSize = Number.parseFloat(
      window.getComputedStyle(document.documentElement).fontSize,
    );
    return {
      distance:
        scroller.scrollHeight - scroller.clientHeight - scroller.scrollTop,
      dockHeight: dock.getBoundingClientRect().height,
      dockTop: dock.getBoundingClientRect().top,
      // The dock's `before:h-8` fade, in rem-scaled px.
      fade: 2 * rootFontSize,
      lastRowBottom: last.getBoundingClientRect().bottom,
      paddingBottom: Number.parseFloat(
        window.getComputedStyle(column).paddingBottom,
      ),
    };
  });
  if (!foot) throw new Error("the stream's foot is not laid out");
  return foot;
}

/**
 * L4.9.1 / critique A7 — the unreachable notice names the seat it is about.
 *
 * The live screen said "this execution" while the strip directly above it
 * showed a *different* seat working; both sentences were true and the reader
 * had no way to tell which one the notice meant.
 */
test("A7: the unreachable notice names the seat, in Mission only", async ({
  page,
}) => {
  await openMission(page, 1920);
  const notice = page.getByTestId("coding-session-composer-unreachable");
  await expect(notice).toBeVisible();
  await expect(notice).toContainText("No provider is answering for Parallax");
  await expect(notice).not.toContainText("for this execution");

  // Conversation's sentence does not move: I8 freezes that DOM, and the
  // masked outerHTML diff against the base is the exhaustive proof.
  await page.getByTestId("coding-session-lens-conversation").click();
  await expect(
    page.getByTestId("coding-session-composer-unreachable"),
  ).toContainText("No provider is answering for this execution");
});

/**
 * L4.1 / critique A1 — the goal reader states its own condition.
 *
 * The three sentences and the `Set goal` gate are proved exhaustively as DOM
 * in `CodingSessionMissionInspector.test.mjs`; what needs a *running app* is
 * the one claim a unit test cannot make — that the reader really is unresolved
 * while the history read is in flight, and that the control which would
 * publish a second 44227 is not on screen while it is.
 *
 * The goal read is held open by wrapping the mock IPC channel before the app
 * boots, rather than by adding a knob to `e2eBridge.ts` — that file is not
 * this lane's, and a fixture that lives in the spec cannot leak into anyone
 * else's run.
 */
test("A1: a goal that has not been read yet says so, and offers no Set goal", async ({
  page,
}) => {
  await page.addInitScript(() => {
    let internals: Record<string, unknown> | undefined;
    // `mockIPC` creates the internals object first and assigns `invoke` onto
    // it afterwards, so wrapping the object as it lands would wrap a bare
    // `{}`. The hook has to be on the property: the bridge's own `invoke`
    // arrives through this setter and is called by the getter's wrapper.
    let real: ((...args: unknown[]) => Promise<unknown>) | undefined;
    const wrapped = (...args: unknown[]): Promise<unknown> => {
      // Kind 44227 is the mission goal. Everything else is untouched, so the
      // rest of the surface renders exactly as it always does.
      if (JSON.stringify(args ?? []).includes("44227")) {
        return new Promise(() => {});
      }
      return real === undefined
        ? Promise.reject(new Error("no mock invoke"))
        : real(...args);
    };
    const hook = (value: Record<string, unknown>) => {
      Object.defineProperty(value, "invoke", {
        configurable: true,
        get: () => wrapped,
        set: (fn: (...args: unknown[]) => Promise<unknown>) => {
          real = fn;
        },
      });
      return value;
    };
    Object.defineProperty(window, "__TAURI_INTERNALS__", {
      configurable: true,
      get: () => internals,
      set: (value: Record<string, unknown>) => {
        internals = hook(value);
      },
    });
  });
  await openMission(page, 1600);
  const inspector = page.getByTestId("coding-session-mission-inspector");
  await expect(inspector.getByTestId("mission-goal-unresolved")).toHaveText(
    "Goal not read yet.",
  );
  // A1's compounding half: the remedy the surface offered for a record it had
  // failed to read was to publish a second one.
  await expect(inspector).not.toContainText("No accepted mission goal");
  expect(
    await page.getByTestId("coding-session-goal-edit-inspector").count(),
  ).toBe(0);
  await waitForAnimations(page);
  await inspector.screenshot({ path: shot("inspector-goal-unresolved") });
});

/**
 * F3 — the Route control acts at every width, or it is a lie.
 *
 * Driven live at 1280 in the review: a button labelled "Expand route rail",
 * pressed twice, moved a localStorage byte from 1 to 0 and back and changed
 * nothing else — not the rail, not the scrubber, not its own `aria-expanded`.
 * 1280 is the width the grid folds at and the commonest laptop there is.
 */
test("F3: the Route toggle acts at 1280, and says what it displaced", async ({
  page,
}) => {
  await openMission(page, 1280);
  const toggle = page.getByTestId("coding-session-route-toggle");
  // The floor has folded the rail: the Inspector is inline at 360 and the
  // stream would drop under 420 with both.
  await expect(page.getByTestId("coding-session-route-scrubber")).toBeVisible();
  await expect(toggle).toHaveAttribute("aria-expanded", "false");
  await expect(toggle).toHaveAttribute("aria-pressed", "false");
  await expect(toggle).toHaveAttribute(
    "title",
    /closes the Inspector, which this width cannot hold beside it/,
  );
  await expect(
    page.getByTestId("coding-session-mission-inspector"),
  ).toBeVisible();

  await toggle.click();
  // Asking for the rail makes the room: the Inspector is the panel that gives
  // way, exactly as its own twin becomes a sheet rather than doing nothing.
  await expect(page.getByTestId("coding-session-route-rail")).toBeVisible();
  await expect(page.getByTestId("coding-session-route-scrubber")).toHaveCount(
    0,
  );
  await expect(toggle).toHaveAttribute("aria-expanded", "true");
  await expect(toggle).toHaveAttribute("aria-pressed", "true");
  await expect(
    page.getByTestId("coding-session-mission-inspector"),
  ).toHaveCount(0);
  await expectStreamFillsTheRails(
    page,
    "1280 after the rail displaced the Inspector",
  );

  // And back: collapsing never re-opens a panel the viewer closed.
  await toggle.click();
  await expect(page.getByTestId("coding-session-route-scrubber")).toBeVisible();
  await expect(toggle).toHaveAttribute("aria-expanded", "false");
});

/**
 * F7 — the control's `aria-controls` resolves to a real element.
 */
test("F7: the Route control points at whichever rail is mounted", async ({
  page,
}) => {
  await openMission(page, 1920);
  const toggle = page.getByTestId("coding-session-route-toggle");
  const id = await toggle.getAttribute("aria-controls");
  expect(id).toBe("coding-session-route-rail");
  const resolved = async () =>
    page.evaluate(
      (target) => document.querySelectorAll(`#${target}`).length,
      id ?? "",
    );
  expect(await resolved(), "the expanded rail carries the id").toBe(1);
  await toggle.click();
  await expect(page.getByTestId("coding-session-route-scrubber")).toBeVisible();
  expect(await resolved(), "and so does the scrubber it folds to").toBe(1);
});

/**
 * F14 — B5 at the width people actually run: 1920, rail at its 224 default.
 *
 * `route-wide-480.png` proved a sign fills a 480 px rail, which nobody's
 * default is. The gutter was reserving one lane more than the map draws.
 */
test("F14: no sign is clipped at the default rail width", async ({ page }) => {
  await openMission(page, 1920);
  const rail = page.getByTestId("coding-session-route-rail");
  await expect(rail).toBeVisible();
  const grid = await measureCodingSessionMissionGrid(page);
  expect(Math.round(grid.route), "the rail is at its default").toBe(224);
  const clipped = await page.evaluate(() =>
    Array.from(
      document.querySelectorAll('[data-testid="coding-session-route-sign"]'),
    )
      .filter((node) => node.scrollWidth > node.clientWidth + 1)
      .map((node) => node.textContent ?? ""),
  );
  expect(clipped, "every sign fits the default rail").toEqual([]);
});
