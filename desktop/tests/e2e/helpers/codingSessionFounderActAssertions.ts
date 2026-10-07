import { readFileSync } from "node:fs";

import { expect, type Page } from "@playwright/test";
import { finalizeEvent, getPublicKey } from "nostr-tools/pure";

import { buildCodingSessionCreateEvent } from "@/features/coding-sessions/lib/codingSessionLifecycleCommand";
import { buildCodingSessionGenesisEvent } from "@/features/coding-sessions/lib/codingSessionGenesis";
import { buildCodingSessionGoalEvent } from "@/features/coding-sessions/lib/codingSessionGoal";
import { buildCodingSessionTargetKey } from "@/features/coding-sessions/lib/codingSessionCommand";
import {
  BEEKEEPER_CODING_SESSION_METADATA_SCHEMA,
  CODING_SESSION_LIFECYCLE_RECEIPT_SCHEMA,
  CODING_SESSION_LIFECYCLE_RECEIPT_TAG_VERSION,
  CODING_SESSION_METADATA_TAG_VERSION,
  codingSessionMetadataSemanticKey,
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
  KIND_CODING_SESSION_AUTHORITY_TRANSITION,
  KIND_CODING_SESSION_LIFECYCLE_RECEIPT,
  KIND_CODING_SESSION_LEASE,
  KIND_CODING_SESSION_METADATA,
  KIND_CODING_SESSION_TEAM_TRANSACTION,
  KIND_CODING_SESSION_TRANSCRIPT,
  KIND_REPO_ANNOUNCEMENT,
} from "@/shared/constants/kinds";
import { installMockBridge } from "../../helpers/bridge";

/**
 * A signed mission whose whole point is the founder's two acts.
 *
 * Deliberately its **own** fixture rather than an import from
 * `coding-session-mission-lens.spec.ts`: importing a Playwright spec file
 * registers its tests a second time, and exporting from it would put L2's
 * fixture on this lane's critical path. What is shared is only the identities
 * the mock bridge already knows — the founder secret below is the bridge's own,
 * because the mock relay keys channel membership to the identities it knows and
 * a fresh key can be the founder or a member, never both.
 */

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

export const CHANNEL_NAME = "engineering";
export const CHANNEL_ID = "1c7e1c02-87bb-5e88-b2da-5a7a9432d0c9";
export const SESSION_REF = "5b7e1c2a-90d4-4b0e-a1f3-7c2d8e6f4a10";

/** The E2E bridge's own known identity — founder and channel member both. */
export const FOUNDER_SECRET = hexToBytes(
  "3dbaebadb5dfd777ff25149ee230d907a15a9e1294b40b830661e65bb42f6c03",
);
export const FOUNDER = getPublicKey(FOUNDER_SECRET);
const BUILDER_SECRET = hexToBytes(
  "1111111111111111111111111111111111111111111111111111111111111111",
);
const BUILDER_PROVIDER = getPublicKey(BUILDER_SECRET);
const BUILDER_ACTOR_SECRET = hexToBytes(
  "3333333333333333333333333333333333333333333333333333333333333333",
);
export const BUILDER_ACTOR = getPublicKey(BUILDER_ACTOR_SECRET);
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
// A second seat, because one seat is a *session* and two are a **team**: the
// umbrella workspace — and with it the Mission lens this lane's controls live
// in — only mounts for an umbrella. A one-seat fixture opens the single-session
// column instead, which is a different surface with no Inspector at all.
const VERIFIER_SECRET = hexToBytes(
  "2222222222222222222222222222222222222222222222222222222222222222",
);
const VERIFIER_PROVIDER = getPublicKey(VERIFIER_SECRET);
const VERIFIER_ACTOR_SECRET = hexToBytes(
  "4444444444444444444444444444444444444444444444444444444444444444",
);
const VERIFIER_ACTOR = getPublicKey(VERIFIER_ACTOR_SECRET);
const VERIFIER_TARGET: CodingSessionCommandTarget = {
  driver: "codex-acp",
  instanceId: "verifier-instance",
  sessionId: "66666666-7777-8888-9999-000000000000",
  generation: 1,
};

/** The commit the mission's report names, and what §1l shortens it to. */
export const HEAD_SHA = "07c470be07c470be07c470be07c470be07c470be";

/**
 * LANE-L20 (finding 38): the repository a mission's creates can name.
 *
 * `FOUNDER` is both the umbrella's founder and this announcement's signer, so
 * a create that names it exercises the real write→read path end to end: the
 * seeded create's `repoRef` is read by `readCodingSessionRepository` against
 * this very announcement (via `__BEEKEEPER_E2E_EXTRA_PROJECT_EVENTS__`), and the
 * resolved owner pubkey reaches the (mocked) `coding_session_land` request.
 */
export const REPO_DTAG = "beekeeper";
export const REPO_REF = `${KIND_REPO_ANNOUNCEMENT}:${FOUNDER}:${REPO_DTAG}`;

/** The kind:30617 announcement `REPO_REF` names, seeded via extra events. */
export function repoAnnouncementEvent(): {
  id: string;
  kind: number;
  pubkey: string;
  created_at: number;
  content: string;
  tags: string[][];
} {
  const event = finalizeEvent(
    {
      kind: KIND_REPO_ANNOUNCEMENT,
      created_at: stepAt(-1),
      tags: [
        ["d", REPO_DTAG],
        ["clone", `https://relay.test/git/${FOUNDER}/${REPO_DTAG}`],
        ["name", "Beekeeper"],
      ],
      content: "",
    },
    FOUNDER_SECRET,
  );
  return {
    id: event.id,
    kind: event.kind,
    pubkey: event.pubkey,
    created_at: event.created_at,
    content: event.content,
    tags: event.tags,
  };
}

const ANCHOR = Math.floor(Date.now() / 1_000) - 60;
const stepAt = (offset: number) => ANCHOR + offset;

/** One signed kind:44244, in NIP-CSTX's exact five-tag envelope. */
function transaction(input: {
  genesisId: string;
  type: string;
  body: Record<string, unknown>;
  secret: Uint8Array;
  createdAt: number;
}): RelayEvent {
  return finalizeEvent(
    {
      kind: KIND_CODING_SESSION_TEAM_TRANSACTION,
      created_at: input.createdAt,
      tags: [
        ["h", CHANNEL_ID],
        ["d", SESSION_REF],
        ["cstx-v", CODING_SESSION_TEAM_TRANSACTION_SCHEMA],
        ["cstx-genesis", input.genesisId],
        ["cstx-type", input.type],
      ],
      content: JSON.stringify({
        schema: CODING_SESSION_TEAM_TRANSACTION_SCHEMA,
        sessionRef: SESSION_REF,
        genesisRef: input.genesisId,
        type: input.type,
        supersedes: null,
        deliveryCommandId: null,
        body: input.body,
      }),
    },
    input.secret,
  ) as unknown as RelayEvent;
}

function signedMetadata(input: {
  createdAt: number;
  actor: string;
  target: CodingSessionCommandTarget;
  secret: Uint8Array;
  role: string;
  runtime: string;
  model: string;
  status: string;
  /**
   * LANE-L20: the runtime's own 44223 echo of the create's `repoRef`
   * (`useCodingSessionCatalog.ts`'s `activeGeneration.repoRef` reads this
   * event, never the create directly) — null reproduces the pre-fix wire.
   */
  repoRef?: string | null;
}): RelayEvent {
  return finalizeEvent(
    {
      kind: KIND_CODING_SESSION_METADATA,
      created_at: input.createdAt,
      tags: [
        ["h", CHANNEL_ID],
        ["csm-v", CODING_SESSION_METADATA_TAG_VERSION],
        ["cs-target", buildCodingSessionTargetKey(input.target)],
        ["csm-key", codingSessionMetadataSemanticKey(input.target)],
      ],
      content: JSON.stringify({
        schema: BEEKEEPER_CODING_SESSION_METADATA_SCHEMA,
        session: input.target,
        projectRef: null,
        repoRef: input.repoRef ?? null,
        title: "The founder's two acts",
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

function signedLiveLease(createdAt: number): RelayEvent {
  return finalizeEvent(
    {
      kind: KIND_CODING_SESSION_LEASE,
      created_at: createdAt,
      tags: [
        ["h", CHANNEL_ID],
        ["cslease-v", "cslease1-1"],
        ["cs-target", buildCodingSessionTargetKey(BUILDER_TARGET)],
        ["csl-command", "founder-acts-seat-1"],
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

function signedTranscript(input: {
  createdAt: number;
  eventSeq: number;
  item: unknown;
}): RelayEvent {
  return finalizeEvent(
    {
      kind: KIND_CODING_SESSION_TRANSCRIPT,
      created_at: input.createdAt,
      tags: [
        ["h", CHANNEL_ID],
        ["cst-v", CODING_SESSION_TRANSCRIPT_TAG_VERSION],
        ["cs-target", buildCodingSessionTargetKey(BUILDER_TARGET)],
        ["cst-seq", String(input.eventSeq)],
        [
          "cst-key",
          codingSessionTranscriptSemanticKey(BUILDER_TARGET, input.eventSeq),
        ],
      ],
      content: JSON.stringify({
        schema: BEEKEEPER_CODING_SESSION_TRANSCRIPT_SCHEMA,
        session: BUILDER_TARGET,
        eventSeq: input.eventSeq,
        timestamp: input.createdAt * 1_000,
        turnId: "builder-turn",
        item: input.item,
      }),
    },
    BUILDER_SECRET,
  ) as unknown as RelayEvent;
}

/** What the mission looks like on the wire, and how the fold reads it. */
export type FounderActMission = {
  events: RelayEvent[];
  foldResponse: Record<string, unknown>;
  ids: {
    genesis: string;
    assignment: string;
    report: string;
    disposition: string;
    founderRequest: string;
    actorRequest: string;
    answeredRequest: string;
    answer: string;
  };
};

/**
 * One assignment, one report naming a commit, the founder's approving
 * disposition over it, and three rulings: one open and held on the founder
 * (three declared options), one open and held on the builder, one answered
 * with a condition.
 *
 * That is deliberately every state §1l has copy for, in one mission, so a
 * single seeded session drives all six screenshots.
 */
export function founderActMission(
  options: {
    withRefusedCompletion?: boolean;
    /**
     * LANE-L20 (finding 38): the `repoRef` every seat's create signs. Null
     * (the default) reproduces the pre-fix wire — every app-created session
     * named no repository at all.
     */
    repoRef?: string | null;
  } = {},
): FounderActMission {
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

  const seats = [
    {
      commandId: "founder-acts-seat-1",
      actor: BUILDER_ACTOR,
      role: "builder",
      providerInstanceRef: "claude-primary",
      provider: BUILDER_PROVIDER,
      model: "sonnet",
      secret: BUILDER_SECRET,
      target: BUILDER_TARGET,
    },
    {
      commandId: "founder-acts-seat-2",
      actor: VERIFIER_ACTOR,
      role: "verifier",
      providerInstanceRef: "codex-primary",
      provider: VERIFIER_PROVIDER,
      model: "gpt-5.6-sol",
      secret: VERIFIER_SECRET,
      target: VERIFIER_TARGET,
    },
  ] as const;
  const lifecycle: RelayEvent[] = [];
  for (const [index, seat] of seats.entries()) {
    const createInput = buildCodingSessionCreateEvent({
      channelId: CHANNEL_ID,
      commandId: seat.commandId,
      projectRef: null,
      repoRef: options.repoRef ?? null,
      sessionRef: SESSION_REF,
      genesisRef: genesis.id,
      actor: seat.actor,
      role: seat.role,
      providerInstanceRef: seat.providerInstanceRef,
      providerAuthorityPubkey: seat.provider,
      model: seat.model,
      title: "The founder's two acts",
      initialTurn: null,
    });
    lifecycle.push(
      finalizeEvent(
        {
          kind: createInput.kind,
          tags: createInput.tags,
          content: createInput.content,
          created_at: stepAt(1 + index * 2),
        },
        FOUNDER_SECRET,
      ) as unknown as RelayEvent,
      finalizeEvent(
        {
          kind: KIND_CODING_SESSION_LIFECYCLE_RECEIPT,
          created_at: stepAt(2 + index * 2),
          tags: [
            ["h", CHANNEL_ID],
            ["cslr-v", CODING_SESSION_LIFECYCLE_RECEIPT_TAG_VERSION],
            ["csl-command", seat.commandId],
            ["csl-key", lifecycleReceiptSemanticKey(seat.commandId)],
          ],
          content: JSON.stringify({
            schema: CODING_SESSION_LIFECYCLE_RECEIPT_SCHEMA,
            commandId: seat.commandId,
            status: "created",
            session: seat.target,
            error: null,
          }),
        },
        seat.secret,
      ) as unknown as RelayEvent,
    );
  }

  const goalInput = buildCodingSessionGoalEvent({
    channelId: CHANNEL_ID,
    content: "Let the founder answer and land from the app.",
    sessionRef: SESSION_REF,
  });
  const goal = finalizeEvent(
    {
      kind: goalInput.kind,
      tags: goalInput.tags,
      content: goalInput.content,
      created_at: stepAt(3),
    },
    FOUNDER_SECRET,
  ) as unknown as RelayEvent;

  const grant = finalizeEvent(
    {
      kind: KIND_CODING_SESSION_AUTHORITY_TRANSITION,
      created_at: stepAt(4),
      tags: [
        ["h", CHANNEL_ID],
        ["csat-v", "csat1-1"],
        ["csat-genesis", genesis.id],
      ],
      content: JSON.stringify({
        genesisRef: genesis.id,
        prevAccepted: null,
        seq: 1,
        type: "grant-seat",
        granteePubkey: BUILDER_ACTOR,
        role: "builder",
      }),
    },
    FOUNDER_SECRET,
  ) as unknown as RelayEvent;
  const grantReceipt = finalizeEvent(
    {
      kind: 40099,
      created_at: stepAt(5),
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
    },
    RELAY_SECRET,
  ) as unknown as RelayEvent;

  const assignment = transaction({
    genesisId: genesis.id,
    type: "assignment",
    body: {
      assigneeActor: BUILDER_ACTOR,
      assigneeRole: "builder",
      objective: "Give the founder the two controls.",
      brief: "Answer a ruling in the app; show the command that lands it.",
      branch: "lane/batch3-l8-founder",
      baseSha: "1".repeat(40),
      fileOwnership: ["desktop/src/features/coding-sessions"],
      acceptanceSteps: ["pnpm test", "playwright test --project=smoke"],
    },
    secret: FOUNDER_SECRET,
    createdAt: stepAt(6),
  });
  const report = transaction({
    genesisId: genesis.id,
    type: "report",
    body: {
      assignmentRef: assignment.id,
      summary: "Both controls implemented, with the rule read from Rust.",
      branch: "lane/batch3-l8-founder",
      baseSha: "1".repeat(40),
      headSha: HEAD_SHA,
      files: ["desktop/src/features/coding-sessions"],
      tests: [
        {
          name: "Founder acts smoke",
          command: "pnpm playwright test coding-session-founder-acts",
          outcome: "passed",
          evidence: "Six states rendered.",
        },
      ],
      redBeforeGreen: true,
      deviations: [],
      residuals: [],
      anomalies: [],
    },
    secret: BUILDER_ACTOR_SECRET,
    createdAt: stepAt(7),
  });
  const disposition = transaction({
    genesisId: genesis.id,
    type: "verdict",
    body: {
      subtype: "disposition",
      assignmentRef: assignment.id,
      reportRef: report.id,
      refutationRef: null,
      decision: "approve",
      summary: "Approved: the rule is read, never re-implemented.",
      findings: [],
      requiredAction: null,
    },
    secret: FOUNDER_SECRET,
    createdAt: stepAt(8),
  });

  const founderRequest = transaction({
    genesisId: genesis.id,
    type: "decision.request",
    body: {
      question: "Land the commit now, or hold for a second verifier?",
      options: [
        "Land it now",
        "Hold for a second verifier",
        "Hold until the relay carries the condition key",
      ],
      heldOn: "founder",
      blocks: [assignment.id],
      recommendation: "Land it now",
    },
    secret: BUILDER_ACTOR_SECRET,
    createdAt: stepAt(9),
  });
  const actorRequest = transaction({
    genesisId: genesis.id,
    type: "decision.request",
    body: {
      question: "Does the builder want the worktree kept after landing?",
      options: ["Keep it", "Remove it"],
      heldOn: BUILDER_ACTOR,
      blocks: [],
      recommendation: null,
    },
    secret: FOUNDER_SECRET,
    createdAt: stepAt(10),
  });
  const answeredRequest = transaction({
    genesisId: genesis.id,
    type: "decision.request",
    body: {
      question: "May the lane publish a fixture the adapter generates?",
      options: ["Yes", "No"],
      heldOn: "founder",
      blocks: [],
      recommendation: null,
    },
    secret: BUILDER_ACTOR_SECRET,
    createdAt: stepAt(11),
  });
  const answer = transaction({
    genesisId: genesis.id,
    type: "decision.answer",
    body: {
      requestRef: answeredRequest.id,
      choice: 0,
      note: null,
    },
    secret: FOUNDER_SECRET,
    createdAt: stepAt(12),
  });

  // A completion the fold refuses. It is on the wire and in the fold's input
  // set, and **not** in `includedEventIds` — which is exactly the shape L7's
  // `completion_not_verified` produces, and the only shape whose exclusion the
  // response may cite (the decoder binds every cited id to the input set).
  const completion = options.withRefusedCompletion
    ? transaction({
        genesisId: genesis.id,
        type: "mission.completed",
        body: {
          assignmentRefs: [assignment.id],
          landedShas: [HEAD_SHA],
          summary: "Both controls shipped.",
          followUps: [],
        },
        secret: FOUNDER_SECRET,
        createdAt: stepAt(12.5),
      })
    : null;
  const transactions = [
    assignment,
    report,
    disposition,
    founderRequest,
    actorRequest,
    answeredRequest,
    answer,
  ];
  const inputEventIds = [
    ...transactions.map((event) => event.id),
    ...(completion ? [completion.id] : []),
  ].sort();
  const includedEventIds = transactions.map((event) => event.id).sort();

  return {
    events: [
      genesis,
      ...lifecycle,
      goal,
      grant,
      grantReceipt,
      ...transactions,
      ...(completion ? [completion] : []),
      signedMetadata({
        createdAt: stepAt(13),
        actor: BUILDER_ACTOR,
        target: BUILDER_TARGET,
        secret: BUILDER_SECRET,
        role: "builder",
        runtime: "claude-agent-acp",
        model: "sonnet",
        status: "running",
        repoRef: options.repoRef ?? null,
      }),
      signedMetadata({
        createdAt: stepAt(13),
        actor: VERIFIER_ACTOR,
        target: VERIFIER_TARGET,
        secret: VERIFIER_SECRET,
        role: "verifier",
        runtime: "codex-acp",
        model: "gpt-5.6-sol",
        status: "completed",
        repoRef: options.repoRef ?? null,
      }),
      signedLiveLease(stepAt(14)),
      signedTranscript({
        createdAt: stepAt(15),
        eventSeq: 1,
        item: {
          kind: "user_prompt",
          content: "Give the founder the controls.",
        },
      }),
      signedTranscript({
        createdAt: stepAt(16),
        eventSeq: 2,
        item: { kind: "assistant_text", text: "Both controls are in place." },
      }),
    ],
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
      includedEventIds,
      excluded: completion
        ? [
            {
              eventId: completion.id,
              code: "completion_not_verified",
              reason:
                "mission.completed requires a verifier's ruling while the policy sets gates.verifierRequired: assignment " +
                `${assignment.id} settled on report ${report.id}, and no active verifier seat has ruled on that report`,
            },
          ]
        : [],
      conflicts: [],
      assignments: [
        {
          assignmentEventId: assignment.id,
          governedReportEventId: report.id,
          dispositionEventId: disposition.id,
          acknowledgementEventId: null,
          settled: true,
          // This chain is settled, and not by choice: the disposition above
          // is `decision: "approve"` with `requiredAction: null`, which is
          // exactly `approving_disposition_asks_nothing`, so `buzz-core`
          // settles it with no acknowledgement owed
          // (`coding_session_team_transaction_fold_settlement.rs:336-350`).
          // `settledBy` is non-null exactly when `settled` is true, and
          // `awaiting` is null exactly then — the decoder's `isSettlement`
          // requires both keys either way.
          settledBy: "approving_disposition_without_ask",
          awaiting: null,
        },
      ],
      unseatedReports: [],
      notes: [],
      decisions: [
        {
          requestId: founderRequest.id,
          heldOn: "founder",
          blocks: [assignment.id],
          answeredBy: null,
          answerId: null,
        },
        {
          requestId: actorRequest.id,
          heldOn: BUILDER_ACTOR,
          blocks: [],
          answeredBy: null,
          answerId: null,
        },
        {
          requestId: answeredRequest.id,
          heldOn: "founder",
          blocks: [],
          answeredBy: FOUNDER,
          answerId: answer.id,
        },
      ],
      waitingOnDecision: { requestId: founderRequest.id, heldOn: "founder" },
      // Required by the decoder's `hasExactFields` (ledger 183(a)/(b)); `null`
      // because this fixture publishes no `mission.completed`.
      pendingCompletion: null,
      canonicalTerminal: null,
    },
    ids: {
      genesis: genesis.id,
      assignment: assignment.id,
      report: report.id,
      disposition: disposition.id,
      founderRequest: founderRequest.id,
      actorRequest: actorRequest.id,
      answeredRequest: answeredRequest.id,
      answer: answer.id,
    },
  };
}

/** The `coding_session_land` answer for a mission whose commit is admitted. */
export function landReadyResponse(mission: FounderActMission) {
  return {
    schema: "buzz-coding-session-land-adapter/v1",
    implementation: "buzz-core",
    repositoryKnown: true,
    ruleGoverns: true,
    admitted: true,
    evidence: {
      // Arm (C): the founder's disposition settled it, the verifier seat
      // independently failed to refute the same report (L21), and the
      // mission's own provider watched every required gate pass on this exact
      // commit (L27 — the clearance alone no longer admits).
      arm: "verifier-verdict",
      sessionRef: SESSION_REF,
      dispositionEventId: mission.ids.disposition,
      dispositionAuthorPubkey: FOUNDER,
      refutationEventId: REFUTATION_EVENT_ID,
      verifierPubkey: VERIFIER,
      reportEventId: mission.ids.report,
      headSha: HEAD_SHA,
      // L27: arm (C) stands on these too, so an admitting arm-(C) answer never
      // carries an empty list. A mock that kept one would be exercising a
      // response the adapter cannot produce.
      observedGates: ["cargo fmt", "cargo clippy", "cargo test"],
      // Finding 89: which policy record the arm stood on, and how it
      // resolved. Arm (C) reads one for its gate list, so `absent` here would
      // be a different fixture, not a shorter one.
      policyResolution: "present",
      policyEventId: POLICY_EVENT_ID,
      policyNotEvaluated: null,
    },
    refusalReason: null,
    newestVerdict: {
      eventId: mission.ids.disposition,
      authorPubkey: FOUNDER,
      decision: "approve",
      reportEventId: mission.ids.report,
      headSha: HEAD_SHA,
    },
    command: `git push origin ${HEAD_SHA}:refs/heads/main`,
    ...landFounders(true),
  };
}

/**
 * The `coding_session_land` answer for a **founder's** push — arm (A).
 *
 * Deliberately over a mission whose newest ruling is `changes-requested`: the
 * point of the arm is that a founder lands with no verdict at all, and a
 * screen that printed "approved" over that ruling would be telling a
 * comfortable lie about a gate (L21).
 */
export function landFounderPushResponse(mission: FounderActMission) {
  return {
    schema: "buzz-coding-session-land-adapter/v1",
    implementation: "buzz-core",
    repositoryKnown: true,
    ruleGoverns: true,
    admitted: true,
    evidence: {
      arm: "founder",
      sessionRef: "",
      dispositionEventId: "",
      dispositionAuthorPubkey: "",
      refutationEventId: "",
      verifierPubkey: "",
      reportEventId: "",
      headSha: HEAD_SHA,
      observedGates: [],
      // Arm (A) reads no policy at all, and says which exception it took —
      // the 2026-09-05 audit's ask, so a founder's landing can never be read
      // as verifier-approved.
      policyResolution: "",
      policyEventId: null,
      policyNotEvaluated: "founder_exception",
    },
    refusalReason: null,
    newestVerdict: {
      eventId: mission.ids.disposition,
      authorPubkey: FOUNDER,
      decision: "changes-requested",
      reportEventId: mission.ids.report,
      headSha: HEAD_SHA,
    },
    command: `git push origin ${HEAD_SHA}:refs/heads/main`,
    ...landFounders(true),
  };
}

/**
 * §1j's refusal strings, **read from the adapter's own generated fixture**.
 *
 * Fix round 1 hand-copied the sentence here, and it went stale the moment L6
 * revised §1j — a Rust test, a JSON fixture, this helper and a shipped PNG all
 * asserting words the relay no longer prints (REVIEW-L8 F2). Reading the
 * fixture means the drift is impossible: the file is regenerated by
 * `the_typescript_decoder_fixture_is_this_adapter_s_real_output`, and a change
 * to §1j reaches this helper without anybody remembering to retype it.
 */
const LAND_FIXTURE = JSON.parse(
  readFileSync(
    new URL(
      "../../../src/features/coding-sessions/lib/codingSessionLandAdapterResponse.fixture.json",
      import.meta.url,
    ),
    "utf8",
  ),
) as Record<string, { refusalReason: string | null }>;

/** §1j's no-verdict string, as `buzz-core` formats it for this fixture. */
export const LAND_REFUSAL_NO_VERDICT = LAND_FIXTURE.refused
  .refusalReason as string;

/**
 * The repository's second founder — a NIP-34 `maintainers` co-owner who signs
 * nothing in this mission (finding 33).
 */
export const CO_FOUNDER =
  "3d3b7169a13a8311b480bdfce85b4a0c7ff9b185832cbc6e547db7bbcf96c05e";

/**
 * The verifier seat whose `not-refuted` refutation clears the report, and that
 * refutation's event id (L21, arm (C)).
 *
 * Fixture values, not derived from this mission's own events: the Land answer
 * these helpers build is a mock of the **adapter's output**, and the wording
 * that matters is pinned by the Rust-generated fixture the refusal strings are
 * read from. What a spec needs here is a well-formed answer whose verifier is
 * visibly not the report's author.
 */
export const VERIFIER =
  "7c1d5e9b2a4f6083bd15c7e4902a3f8615d0b47ce93a6f28104b5d7e83c9a061";
/** The kind 44245 record the admitting arm-(C) fixture stood on. */
export const POLICY_EVENT_ID =
  "9a9a9a9a9a9a9a9a9a9a9a9a9a9a9a9a9a9a9a9a9a9a9a9a9a9a9a9a9a9a9a9a";
export const REFUTATION_EVENT_ID =
  "441a97330c5e28b6f31d70a94e6c82b5137fa0de95c4813627ab0e9d5f31c8a4";

/**
 * The founder set every mock land answer carries, and the sentence
 * `buzz-core` composes for it.
 *
 * The sentence is the shape `RepositoryFounders::rules_sentence` emits with a
 * read roster; keeping it here rather than importing the crate is deliberate —
 * the fixture the Rust adapter generates is what pins the *wording*, and this
 * helper only needs a well-formed answer to render.
 */
export function landFounders(viewerIsFounder: boolean) {
  return {
    founders: [FOUNDER, CO_FOUNDER],
    foundersNote:
      `rules are set by any founder, as a signed rule record; the announcement's own rows ` +
      `stay with its signer ${FOUNDER}; ` +
      `founders of this repository are ${FOUNDER}, ${CO_FOUNDER} (2).`,
    viewerIsFounder,
    rulesSigner: FOUNDER,
    rosterRead: true,
    // The mission's seats reached the rule — arm (C) could be evaluated.
    seatsRead: true,
    // And so did its gate rows, which arm (C) has needed since L27. `false`
    // here would make every refusal in these fixtures carry the "this view
    // read no gate rows" disclosure.
    gateRowsRead: true,
    // Finding 91: the mission's repository binding reached the rule, so the
    // answer is not an assumption about the repository on screen. The
    // decoder's key set is exact — a fixture missing this key throws
    // "malformed response" before anything renders.
    boundRepositoriesRead: true,
  };
}

/** §1j's branch-scoping string, as `buzz-core` formats it for this fixture. */
export const LAND_REFUSAL_ANOTHER_REF = LAND_FIXTURE.approvedForAnotherRef
  .refusalReason as string;

/** The `coding_session_land` answer for a mission the rule refuses. */
export function landRefusedResponse(mission: FounderActMission) {
  return {
    schema: "buzz-coding-session-land-adapter/v1",
    implementation: "buzz-core",
    repositoryKnown: true,
    ruleGoverns: true,
    admitted: false,
    evidence: null,
    refusalReason: LAND_REFUSAL_NO_VERDICT,
    newestVerdict: {
      eventId: mission.ids.disposition,
      authorPubkey: FOUNDER,
      decision: "changes-requested",
      reportEventId: mission.ids.report,
      headSha: HEAD_SHA,
    },
    command: null,
    ...landFounders(true),
  };
}

/** The `coding_session_land` answer for a commit approved for another ref. */
export function landApprovedForAnotherRefResponse(mission: FounderActMission) {
  return {
    ...landRefusedResponse(mission),
    refusalReason: LAND_REFUSAL_ANOTHER_REF,
    newestVerdict: {
      eventId: mission.ids.disposition,
      authorPubkey: FOUNDER,
      decision: "approve",
      reportEventId: mission.ids.report,
      headSha: HEAD_SHA,
    },
  };
}

/** Open the mock app as the umbrella's founder. */
export async function openFounderActApp(
  page: Page,
  input: {
    foldResponse: Record<string, unknown>;
    landResponse?: Record<string, unknown>;
    supportsCondition?: boolean;
    rejectPublishedKinds?: { kind: number; message: string }[];
  },
) {
  await page.addInitScript(
    ({ founderIdentity }) => {
      window.localStorage.setItem("buzz-theme", "buzz");
      window.localStorage.setItem(
        "buzz:e2e-identity-override.v1",
        JSON.stringify(founderIdentity),
      );
    },
    {
      founderIdentity: {
        privateKey: hex(FOUNDER_SECRET),
        pubkey: FOUNDER,
        username: "tyler",
      },
    },
  );
  await installMockBridge(page, {
    codingSessionTeamFoldResponse: input.foldResponse,
    codingSessionLandResponse: input.landResponse,
    codingSessionTeamTransactionCapabilities:
      input.supportsCondition === undefined
        ? undefined
        : {
            schema: "buzz-coding-session-team-transaction-adapter/v1",
            implementation: "buzz-core",
            choiceMaxBytes: 2048,
            noteMaxBytes: 8192,
            conditionMaxBytes: 512,
            supportsDecisionAnswerCondition: input.supportsCondition,
          },
    rejectPublishedKinds: input.rejectPublishedKinds,
    searchProfiles: [{ pubkey: BUILDER_ACTOR, displayName: "Bob" }],
    relaySelf: RELAY,
  });
  await page.setViewportSize({ width: 1400, height: 900 });
  await page.goto("/");
}

/** Seed the mission, open the session, and switch to the Mission lens. */
export async function openMissionLens(page: Page, mission: FounderActMission) {
  await page.getByTestId(`channel-${CHANNEL_NAME}`).click();
  await page.evaluate(
    ({ channelName, events }) => {
      const seed = window.__BEEKEEPER_E2E_SEED_MOCK_SIGNED_EVENT__;
      if (!seed) throw new Error("signed-event seeding hook is missing");
      for (const event of events) seed({ channelName, event });
    },
    { channelName: CHANNEL_NAME, events: mission.events },
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
  await page.getByTestId("coding-session-lens-mission").click();
  const inspector = page.getByTestId("coding-session-mission-inspector");
  await expect(inspector).toBeVisible({ timeout: 15_000 });
  return inspector;
}
