import type { CodingSessionContextLoad } from "./codingSessionContextLoad";
import type { CodingSessionMissionTransactionInput } from "./codingSessionMissionContracts";
import type { CodingSessionParticipantPresence } from "./codingSessionStreamPresence";
import type {
  CodingSessionTaskModel,
  CodingSessionTaskStatus,
} from "./codingSessionTaskModel";
import type { CodingSessionObservedChanges } from "./codingSessionTranscriptModel";
import {
  addTruncation,
  cap,
  deriveAssignmentContextFacts,
  deriveContextFacts,
  deriveFiles,
} from "./codingSessionMissionInspectorBounds";
import type { CodingSessionGoalDisagreement } from "./codingSessionMissionGoal";
import type {
  CodingSessionMissionLandModel,
  CodingSessionMissionLandUnavailableReason,
} from "./codingSessionMissionLand";
import type { ImmutableCodingSessionTeamWireEvent } from "./invokeCodingSessionTeamFold";
import {
  deriveDecisions,
  type CodingSessionMissionDecisionAnswerInput,
  type CodingSessionMissionDecisionInput,
  type CodingSessionMissionDecisionRequestInput,
  type CodingSessionMissionDecisionModel,
  type CodingSessionMissionWaitingModel,
  type CodingSessionMissionWaitingOnDecisionInput,
} from "./codingSessionMissionDecisions";

export {
  addTruncation,
  cap,
  MISSION_INSPECTOR_LIMITS,
} from "./codingSessionMissionInspectorBounds";
export type {
  CodingSessionMissionContextFact,
  CodingSessionMissionFileAttribution,
  CodingSessionMissionFileModel,
  CodingSessionMissionInspectorSection,
  CodingSessionMissionTruncation,
} from "./codingSessionMissionInspectorBounds";
import type {
  CodingSessionMissionContextFact,
  CodingSessionMissionFileModel,
  CodingSessionMissionTruncation,
} from "./codingSessionMissionInspectorBounds";
import { MISSION_INSPECTOR_LIMITS } from "./codingSessionMissionInspectorBounds";

/** The fold's own included set and the signed events it was folded from. */
export type CodingSessionMissionLandEvidenceInput = {
  includedEventIds: readonly string[];
  wireEvents: readonly ImmutableCodingSessionTeamWireEvent[];
  /**
   * The mission's active seats, with their roles.
   *
   * Arm (C) of the require-verdict rule asks whether a **`verifier`** seat
   * cleared the report, so the push predicate needs roles, not just event
   * ids. Empty means the surface could not read them — the native answer then
   * says `seatsRead: false`, and its refusal is about a check that did not
   * run rather than a verifier that does not exist.
   */
  activeSeats: readonly { actorPubkey: string; role: string }[];
};

export type CodingSessionMissionGoalInput =
  | { kind: "absent" }
  /**
   * A 44227 exists for this channel and this surface refused it on identity.
   *
   * Critique A1: rendering this as `absent` makes "nobody set a goal" and "a
   * goal is here and we would not bind it" the same sentence, which is a claim
   * about the wire made from the outcome of a local join.
   */
  | {
      kind: "rejected";
      disagreements: readonly CodingSessionGoalDisagreement[];
    }
  | {
      kind: "available";
      sourceEventId: string;
      authorLabel: string;
      text: string;
    }
  | { kind: "conflict"; eventIds: readonly string[] };

export type CodingSessionAcceptedPlanInput =
  | { kind: "absent" }
  | {
      kind: "available";
      steps: readonly CodingSessionAcceptedPlanStepInput[];
    }
  | { kind: "conflict"; eventIds: readonly string[] };

export type CodingSessionAcceptedPlanStepInput = {
  text: string;
  sourceEventId: string;
  authorLabel: string;
  sourceCreatedAt: number;
  sourceIndex: number;
};

export type CodingSessionSeatPlanInput = {
  executionKey: string;
  ownerLabel: string;
  model: CodingSessionTaskModel | null;
};

export type CodingSessionStructuredTestInput = {
  name: string;
  command: string;
  outcome: "passed" | "failed" | "not-run";
  evidence: string | null;
};

export type CodingSessionMissionReportInput = {
  sourceEventId: string;
  /** Exact signed author and timestamp retained for coordination joins. */
  authorPubkey?: string;
  sourceCreatedAt?: number;
  /** Existing operation wake correlation, when the writer supplied one. */
  deliveryCommandId?: string | null;
  authorLabel: string;
  summary: string;
  assignmentRef: string;
  branch: string | null;
  baseSha: string | null;
  headSha: string | null;
  files: readonly string[];
  tests: readonly CodingSessionStructuredTestInput[];
};

export type CodingSessionMissionAssignmentInput = {
  sourceEventId: string;
  authorLabel: string;
  assigneeRole: string;
  objective: string;
  brief: string;
  fileOwnership: readonly string[];
};

export type CodingSessionMissionCanonicalStep = {
  type:
    | "assignment"
    | "report"
    | "refutation"
    | "disposition"
    | "acknowledgement";
  sourceEventId: string;
  authorPubkey: string;
  createdAt: number;
  summary: string;
  decision?: string | null;
  requiredAction?: string | null;
};

export type CodingSessionMissionStateInput =
  | { kind: "unknown"; detail: string | null }
  | {
      kind: "running";
      sourceEventId: string;
      phase: "assigned" | "reported" | "ruled" | "acknowledged";
      detail: string;
      canonicalChain: readonly CodingSessionMissionCanonicalStep[];
    }
  | {
      kind: "acknowledgement-required";
      sourceEventId: string;
      assignmentRef: string;
      requiredAction: string;
      heldOn: string | null;
      canonicalChain: readonly CodingSessionMissionCanonicalStep[];
    }
  | {
      kind: "waiting-on-person";
      sourceEventId: string;
      requiredAction: string;
      heldOn: string | null;
      canonicalChain?: readonly CodingSessionMissionCanonicalStep[];
    }
  | { kind: "stalled"; sourceEventId: string; detail: string }
  | {
      kind: "blocked";
      sourceEventId: string;
      summary: string;
      blockers: readonly string[];
      requiredAction: string;
      canonicalChain: readonly CodingSessionMissionCanonicalStep[];
    }
  | {
      kind: "completed";
      sourceEventId: string;
      summary: string;
      landedShas: readonly string[];
      followUps: readonly string[];
      canonicalChain: readonly CodingSessionMissionCanonicalStep[];
    }
  | { kind: "conflict"; eventIds: readonly string[] };

export type CodingSessionMissionUsageInput = {
  sourceEventId: string;
  inputTokens: number | null;
  outputTokens: number | null;
  totalTokens: number | null;
  toolCalls: number | null;
  costUsd: number | null;
};

export type CodingSessionMissionDisclosureInput = {
  code: string;
  summary: string;
  eventIds: readonly string[];
};

/**
 * One active assignment's settlement, carried verbatim from the native fold
 * (ledger 183(c)/(g)).
 *
 * `awaiting` is the whole point: three nulls said *that* a chain was
 * incomplete and never *where*, and the kettle lead read a missing
 * acknowledgement as a missing refutation and recalled a verifier that owed
 * nothing. Absent (`undefined`) means no fold ran — unknown, never "settled".
 */
export type CodingSessionMissionSettlementInput = {
  assignmentEventId: string;
  settled: boolean;
  awaiting: {
    link: "report" | "disposition" | "acknowledgement";
    owedByRole: string;
    /** `null` for a disposition: several parties may rule (183(c)). */
    owedByActor: string | null;
  } | null;
};

/** A signed completion whose prerequisites are merely late (ledger 183(a)). */
export type CodingSessionMissionPendingCompletionInput = {
  eventId: string;
  code: string;
  /** The fold's own sentence, verbatim. */
  reason: string;
  unsettledAssignmentEventIds: readonly string[];
};

export type CodingSessionMissionInspectorInput = {
  goal: CodingSessionMissionGoalInput;
  acceptedPlan: CodingSessionAcceptedPlanInput;
  assignments?: readonly CodingSessionMissionAssignmentInput[];
  seatPlans: readonly CodingSessionSeatPlanInput[];
  reports: readonly CodingSessionMissionReportInput[];
  /** Fold-included 44244 rows for the causality stream, chronological. */
  transactions?: readonly CodingSessionMissionTransactionInput[];
  /** How many older rows the projection dropped, disclosed rather than hidden. */
  transactionsTruncated?: number;
  /**
   * Reports the Rust fold listed under `unseatedReports`. Absent means no fold
   * ran, which is `unknown` — never "every report is seated".
   */
  unseatedReportEventIds?: readonly string[];
  /** The fold's `assignments[]`; absent means no fold ran. */
  settlements?: readonly CodingSessionMissionSettlementInput[];
  /** The fold's `pendingCompletion`; `null` is a folded "nothing is held". */
  pendingCompletion?: CodingSessionMissionPendingCompletionInput | null;
  observedChanges: CodingSessionObservedChanges;
  /** Trusted projected source items for observed paths, when ingress retains them. */
  observedFileSources?: ReadonlyMap<string, readonly string[]>;
  participants: readonly CodingSessionParticipantPresence[];
  contextLoads: ReadonlyMap<string, CodingSessionContextLoad | null>;
  missionState: CodingSessionMissionStateInput;
  /**
   * The fold's `decisions[]`. **Absent means no fold ran** — which is
   * `unknown`, not "nothing was asked" — so the queue can say which.
   */
  decisions?: readonly CodingSessionMissionDecisionInput[];
  /** The fold's `waitingOnDecision`; null is a folded "nothing is waiting". */
  waitingOnDecision?: CodingSessionMissionWaitingOnDecisionInput | null;
  /**
   * The signed `decision.request` bodies behind those rows.
   *
   * The queue needs the request's own declared options to offer buttons, and a
   * stream summary has never carried them.
   */
  decisionRequests?: readonly CodingSessionMissionDecisionRequestInput[];
  /** The signed `decision.answer` bodies, for an answered row's own words. */
  decisionAnswers?: readonly CodingSessionMissionDecisionAnswerInput[];
  /**
   * The umbrella this surface is showing, so the Answer control can publish
   * without a second source of truth about which session it is in.
   */
  channelRef?: string | null;
  sessionRef?: string | null;
  genesisRef?: string | null;
  /**
   * The signed events and the fold's included set, for the push path's rule.
   *
   * Absent when no fold ran. Nothing in TypeScript reads inside these — they
   * are handed to `coding_session_land` and the rule answers.
   */
  landEvidence?: CodingSessionMissionLandEvidenceInput;
  /**
   * What the push path's rule said about landing this mission, or null when
   * this surface did not ask. Computed by `useCodingSessionMissionLand`.
   */
  land?: CodingSessionMissionLandModel | null;
  /**
   * Why `land` is null, when it is — finding 37. Undefined only for a caller
   * that never asked `useCodingSessionMissionLand` at all (a test double, or
   * a surface with no Mission open); once the hook ran, it is always one of
   * the two named reasons.
   */
  landUnavailableReason?: CodingSessionMissionLandUnavailableReason | null;
  /**
   * Whether a kind:44245 record reached this view at all.
   *
   * Absent or false is disclosed rather than folded into "no requirement":
   * unknown is not the same fact as a policy that set none (§1l).
   */
  policyRecordKnown?: boolean;
  /**
   * Whether the lead seat has an open turn right now.
   *
   * The one input to the waiting state this surface owns rather than reads:
   * liveness is a fact about executions, which the fold has never seen.
   */
  leadHasOpenTurn?: boolean;
  /** Actor pubkey → display name, from the surface's own resolver. */
  resolveActorLabel?: (pubkey: string) => string | null;
  /**
   * The viewer's own key, so a ruling held on them reads `you` rather than
   * eight characters of their own pubkey (F12).
   */
  currentUserPubkey?: string | null;
  /**
   * The umbrella's founder key, so an answer signed by it reads `the founder`
   * — the same words the fold's own `heldOn: "founder"` produces. Without it
   * the founder's answer would read as eight hex characters of their key,
   * which is true and useless.
   */
  founderPubkey?: string | null;
  usage: CodingSessionMissionUsageInput | null;
  rejectedEventCount: number | null;
  rejectionsTruncated: boolean;
  rejectedReasons: readonly CodingSessionMissionDisclosureInput[];
  conflicts: readonly CodingSessionMissionDisclosureInput[];
};

export type CodingSessionMissionGoalModel =
  | { kind: "absent" }
  | {
      kind: "rejected";
      disagreements: readonly CodingSessionGoalDisagreement[];
    }
  | {
      kind: "available";
      sourceEventId: string;
      authorLabel: string;
      text: string;
    }
  | { kind: "conflict"; eventIds: string[] };

export type CodingSessionMissionPlanStep = {
  id: string;
  text: string;
  /** null means the producer published no progress state; it is not pending. */
  status: CodingSessionTaskStatus | null;
  sourceEventId: string | null;
  authorLabel: string | null;
  /** Signed event timestamp for accepted criteria; null for seat-local plans. */
  sourceCreatedAt: number | null;
  /** Zero-based position inside the signed assignment; null for seat-local plans. */
  sourceIndex: number | null;
};

export type CodingSessionMissionPlanModel = {
  kind: "absent" | "available" | "conflict";
  label: string;
  sourceEventIds: string[];
  authorLabel: string | null;
  steps: CodingSessionMissionPlanStep[];
};

export type CodingSessionMissionSeatPlanModel = {
  executionKey: string;
  ownerLabel: string;
  sourceEventId: string | null;
  explanation: string | null;
  state: CodingSessionTaskModel["state"] | "absent";
  completedCount: number;
  steps: CodingSessionMissionPlanStep[];
};

export type CodingSessionMissionTestModel = CodingSessionStructuredTestInput & {
  id: string;
  sourceEventId: string;
  authorLabel: string;
};

export type CodingSessionMissionInspectorModel = {
  goal: CodingSessionMissionGoalModel;
  acceptedPlan: CodingSessionMissionPlanModel;
  seatPlans: CodingSessionMissionSeatPlanModel[];
  reports: Array<{
    sourceEventId: string;
    authorLabel: string;
    summary: string;
    /**
     * Whether the fold found a live seat for this report's author and role.
     * `unknown` when no fold ran; the fold decides, never this layer (I6).
     */
    seatAuthority: "granted" | "unseated" | "unknown";
  }>;
  /** Passed through unchanged from the projection; the stream owns rendering. */
  transactions: readonly CodingSessionMissionTransactionInput[];
  transactionsTruncated: number;
  tests: CodingSessionMissionTestModel[];
  changes: {
    state: "none" | "named" | "unnamed" | "mixed";
    namedEditCount: number;
    unreportedEditCount: number;
    additions: number | null;
    deletions: number | null;
  };
  files: CodingSessionMissionFileModel[];
  participants: Array<
    CodingSessionParticipantPresence & {
      contextLoad: CodingSessionContextLoad | null;
    }
  >;
  contextFacts: CodingSessionMissionContextFact[];
  missionState: CodingSessionMissionStateInput;
  /** The decision queue, bounded; ordered open-first, newest asked first. */
  decisions: CodingSessionMissionDecisionModel[];
  /** Rows the bound dropped — disclosed by the queue, never silent (I10). */
  decisionsTruncated: number;
  /** The sentence for those rows, naming what was actually dropped (F13). */
  decisionsTruncatedNotice: string | null;
  /** False when no fold supplied `decisions`: unknown, not empty (I9). */
  decisionsKnown: boolean;
  /** The waiting-on-a-person fact, or null when nothing is waiting. */
  waiting: CodingSessionMissionWaitingModel | null;
  /** What the push path's rule said about landing this mission, or null. */
  land: CodingSessionMissionLandModel | null;
  /** Why `land` is null, when it is — finding 37. */
  landUnavailableReason: CodingSessionMissionLandUnavailableReason | null;
  /**
   * Whether the Rust fold excluded a `mission.completed` with L7's
   * `completion_not_verified`.
   *
   * Read from the fold's own exclusion code, never decided here (I6).
   */
  completionNotVerified: boolean;
  /** Whether a kind:44245 record reached this view. Unknown ≠ false (§1l). */
  policyRecordKnown: boolean;
  usage: CodingSessionMissionUsageInput | null;
  integrity: {
    rejectedEventCount: number | null;
    rejectionsTruncated: boolean;
    rejectedReasons: CodingSessionMissionDisclosureInput[];
    conflicts: CodingSessionMissionDisclosureInput[];
  };
  truncations: CodingSessionMissionTruncation[];
};

/** Bound and copy trusted projections; every omission becomes visible metadata. */
export function deriveCodingSessionMissionInspectorModel(
  input: CodingSessionMissionInspectorInput,
): CodingSessionMissionInspectorModel {
  const truncations: CodingSessionMissionTruncation[] = [];
  const reports = cap(
    input.reports,
    MISSION_INSPECTOR_LIMITS.reports,
    "reports",
    "structured reports",
    truncations,
  );
  const assignments = cap(
    input.assignments ?? [],
    MISSION_INSPECTOR_LIMITS.assignments,
    "context",
    "accepted assignments",
    truncations,
  );
  if (reports.length < input.reports.length) {
    for (const section of ["tests", "files", "context"] as const) {
      addTruncation(
        section,
        reports.length,
        input.reports.length,
        "reports contributing to this section",
        truncations,
      );
    }
  }
  const observedFiles = cap(
    input.observedChanges.files,
    MISSION_INSPECTOR_LIMITS.files,
    "changes",
    "observed files",
    truncations,
  );
  if (observedFiles.length < input.observedChanges.files.length) {
    addTruncation(
      "files",
      observedFiles.length,
      input.observedChanges.files.length,
      "observed files",
      truncations,
    );
  }
  const seatPlans = cap(
    input.seatPlans,
    MISSION_INSPECTOR_LIMITS.seatPlans,
    "seat-plans",
    "seat plans",
    truncations,
  ).map((plan) => deriveSeatPlan(plan, truncations));
  const participants = cap(
    input.participants,
    MISSION_INSPECTOR_LIMITS.participants,
    "team",
    "participants",
    truncations,
  ).map((participant) => ({
    ...participant,
    status: { ...participant.status },
    contextLoad: copyContextLoad(
      input.contextLoads.get(participant.executionKey) ?? null,
    ),
  }));
  const boundedChanges = {
    files: observedFiles,
    unreportedEditCount: input.observedChanges.unreportedEditCount,
  };
  return {
    goal: deriveGoal(input.goal, truncations),
    acceptedPlan: deriveAcceptedPlan(input.acceptedPlan, truncations),
    seatPlans,
    reports: reports.map((report) => ({
      sourceEventId: report.sourceEventId,
      authorLabel: report.authorLabel,
      summary: report.summary,
      seatAuthority:
        input.unseatedReportEventIds === undefined
          ? ("unknown" as const)
          : input.unseatedReportEventIds.includes(report.sourceEventId)
            ? ("unseated" as const)
            : ("granted" as const),
    })),
    transactions: input.transactions ?? [],
    transactionsTruncated: input.transactionsTruncated ?? 0,
    tests: deriveTests(reports, truncations),
    changes: deriveChanges(boundedChanges),
    files: deriveFiles(
      boundedChanges,
      input.observedFileSources,
      reports,
      truncations,
    ),
    participants,
    contextFacts: cap(
      [
        ...assignments.flatMap((assignment) =>
          deriveAssignmentContextFacts(assignment, truncations),
        ),
        ...reports.flatMap(deriveContextFacts),
      ],
      MISSION_INSPECTOR_LIMITS.contextFacts,
      "context",
      "context facts",
      truncations,
    ),
    missionState: deriveMissionState(input.missionState, truncations),
    ...deriveDecisions(input),
    land: input.land ?? null,
    landUnavailableReason: input.landUnavailableReason ?? null,
    // L8.3: the fold's own exclusion code, read verbatim. This layer never
    // decides whether a completion was verified — L7's rule does, in Rust (I6).
    completionNotVerified: input.rejectedReasons.some(
      (reason) => reason.code === "completion_not_verified",
    ),
    policyRecordKnown: input.policyRecordKnown ?? false,
    usage: hasUsage(input.usage) ? { ...input.usage } : null,
    integrity: {
      rejectedEventCount:
        input.rejectedEventCount === null
          ? null
          : Math.max(0, input.rejectedEventCount),
      rejectionsTruncated: input.rejectionsTruncated,
      rejectedReasons: deriveDisclosures(
        input.rejectedReasons,
        "rejected reasons",
        truncations,
      ),
      conflicts: deriveDisclosures(input.conflicts, "conflicts", truncations),
    },
    truncations,
  };
}

function deriveGoal(
  input: CodingSessionMissionGoalInput,
  truncations: CodingSessionMissionTruncation[],
): CodingSessionMissionGoalModel {
  if (input.kind !== "conflict") return { ...input };
  return {
    kind: "conflict",
    eventIds: cap(
      input.eventIds,
      MISSION_INSPECTOR_LIMITS.goalSourceEventIds,
      "goal",
      "conflicting goal sources",
      truncations,
    ),
  };
}

function deriveAcceptedPlan(
  input: CodingSessionAcceptedPlanInput,
  truncations: CodingSessionMissionTruncation[],
): CodingSessionMissionPlanModel {
  if (input.kind === "absent") {
    return {
      kind: "absent",
      label: "No accepted plan published",
      sourceEventIds: [],
      authorLabel: null,
      steps: [],
    };
  }
  if (input.kind === "conflict") {
    return {
      kind: "conflict",
      label: "Accepted plan unavailable — conflicting signed records",
      sourceEventIds: cap(
        input.eventIds,
        MISSION_INSPECTOR_LIMITS.disclosureEventIds,
        "accepted-plan",
        "conflicting accepted-plan sources",
        truncations,
      ),
      authorLabel: null,
      steps: [],
    };
  }
  const sortedSteps = [...input.steps].sort(compareAcceptedPlanSteps);
  const allSourceEventIds = [
    ...new Set(sortedSteps.map((step) => step.sourceEventId)),
  ];
  const steps = cap(
    sortedSteps,
    MISSION_INSPECTOR_LIMITS.acceptedPlanSteps,
    "accepted-plan",
    "accepted-plan steps",
    truncations,
  ).map((step) => ({
    id: `${step.sourceEventId}:accepted:${step.sourceIndex}`,
    text: step.text,
    status: null,
    sourceEventId: step.sourceEventId,
    authorLabel: step.authorLabel,
    sourceCreatedAt: step.sourceCreatedAt,
    sourceIndex: step.sourceIndex,
  }));
  const sourceEventIds = [
    ...new Set(steps.map((step) => step.sourceEventId as string)),
  ];
  if (sourceEventIds.length < allSourceEventIds.length) {
    addTruncation(
      "accepted-plan",
      sourceEventIds.length,
      allSourceEventIds.length,
      "accepted-plan sources",
      truncations,
    );
  }
  const authorLabels = [
    ...new Set(steps.map((step) => step.authorLabel as string)),
  ];
  return {
    kind: "available",
    label: steps.length > 0 ? "Accepted plan" : "Accepted plan has no steps",
    sourceEventIds,
    authorLabel: authorLabels.join(", "),
    steps,
  };
}

function compareAcceptedPlanSteps(
  left: CodingSessionAcceptedPlanStepInput,
  right: CodingSessionAcceptedPlanStepInput,
): number {
  return (
    left.sourceCreatedAt - right.sourceCreatedAt ||
    left.sourceEventId.localeCompare(right.sourceEventId) ||
    left.sourceIndex - right.sourceIndex
  );
}

function deriveSeatPlan(
  input: CodingSessionSeatPlanInput,
  truncations: CodingSessionMissionTruncation[],
): CodingSessionMissionSeatPlanModel {
  const model = input.model;
  const tasks = model
    ? cap(
        model.tasks,
        MISSION_INSPECTOR_LIMITS.seatPlanSteps,
        "seat-plans",
        "seat plan steps",
        truncations,
      )
    : [];
  return {
    executionKey: input.executionKey,
    ownerLabel: input.ownerLabel,
    sourceEventId: model?.sourceItemId ?? null,
    explanation: model?.explanation ?? null,
    state: model?.state ?? "absent",
    completedCount: model?.completedCount ?? 0,
    steps: tasks.map((task) => ({
      id: task.id,
      text: task.text,
      status: task.status,
      sourceEventId: null,
      authorLabel: null,
      sourceCreatedAt: null,
      sourceIndex: null,
    })),
  };
}

function deriveTests(
  reports: readonly CodingSessionMissionReportInput[],
  truncations: CodingSessionMissionTruncation[],
): CodingSessionMissionTestModel[] {
  const tests: CodingSessionMissionTestModel[] = [];
  let totalAfterNestedCaps = 0;
  for (const report of reports) {
    const reportTests = cap(
      report.tests,
      MISSION_INSPECTOR_LIMITS.testsPerReport,
      "tests",
      "report tests",
      truncations,
    );
    totalAfterNestedCaps += reportTests.length;
    for (const [index, result] of reportTests.entries()) {
      if (tests.length >= MISSION_INSPECTOR_LIMITS.tests) continue;
      tests.push({
        ...result,
        id: `${report.sourceEventId}:test:${index}`,
        sourceEventId: report.sourceEventId,
        authorLabel: report.authorLabel,
      });
    }
  }
  if (tests.length < totalAfterNestedCaps) {
    addTruncation(
      "tests",
      tests.length,
      totalAfterNestedCaps,
      "tests after the section limit",
      truncations,
    );
  }
  return tests;
}

function deriveChanges(changes: CodingSessionObservedChanges) {
  const namedEditCount = changes.files.reduce(
    (total, file) => total + file.editCount,
    0,
  );
  const allCountsKnown = changes.files.every(
    (file) => file.additions !== null && file.deletions !== null,
  );
  return {
    state:
      namedEditCount === 0 && changes.unreportedEditCount === 0
        ? ("none" as const)
        : namedEditCount === 0
          ? ("unnamed" as const)
          : changes.unreportedEditCount > 0
            ? ("mixed" as const)
            : ("named" as const),
    namedEditCount,
    unreportedEditCount: changes.unreportedEditCount,
    additions:
      changes.files.length > 0 && allCountsKnown
        ? changes.files.reduce((sum, file) => sum + (file.additions ?? 0), 0)
        : null,
    deletions:
      changes.files.length > 0 && allCountsKnown
        ? changes.files.reduce((sum, file) => sum + (file.deletions ?? 0), 0)
        : null,
  };
}

function deriveMissionState(
  input: CodingSessionMissionStateInput,
  truncations: CodingSessionMissionTruncation[],
): CodingSessionMissionStateInput {
  if (input.kind === "running" || input.kind === "acknowledgement-required") {
    return {
      ...input,
      canonicalChain: cap(
        input.canonicalChain,
        MISSION_INSPECTOR_LIMITS.missionStateItems,
        "mission-state",
        "canonical team transaction steps",
        truncations,
      ).map((step) => ({ ...step })),
    };
  }
  if (input.kind === "waiting-on-person" && input.canonicalChain) {
    return {
      ...input,
      canonicalChain: cap(
        input.canonicalChain,
        MISSION_INSPECTOR_LIMITS.missionStateItems,
        "mission-state",
        "canonical team transaction steps",
        truncations,
      ).map((step) => ({ ...step })),
    };
  }
  if (input.kind === "blocked") {
    return {
      ...input,
      blockers: cap(
        input.blockers,
        MISSION_INSPECTOR_LIMITS.missionStateItems,
        "mission-state",
        "mission blockers",
        truncations,
      ),
      canonicalChain: cap(
        input.canonicalChain,
        MISSION_INSPECTOR_LIMITS.missionStateItems,
        "mission-state",
        "canonical team transaction steps",
        truncations,
      ).map((step) => ({ ...step })),
    };
  }
  if (input.kind === "completed") {
    return {
      ...input,
      landedShas: cap(
        input.landedShas,
        MISSION_INSPECTOR_LIMITS.missionStateItems,
        "mission-state",
        "landed SHAs",
        truncations,
      ),
      followUps: cap(
        input.followUps,
        MISSION_INSPECTOR_LIMITS.missionStateItems,
        "mission-state",
        "mission follow-ups",
        truncations,
      ),
      canonicalChain: cap(
        input.canonicalChain,
        MISSION_INSPECTOR_LIMITS.missionStateItems,
        "mission-state",
        "canonical team transaction steps",
        truncations,
      ).map((step) => ({ ...step })),
    };
  }
  if (input.kind === "conflict") {
    return {
      kind: "conflict",
      eventIds: cap(
        input.eventIds,
        MISSION_INSPECTOR_LIMITS.disclosureEventIds,
        "mission-state",
        "conflicting mission-state sources",
        truncations,
      ),
    };
  }
  return { ...input };
}

function deriveDisclosures(
  input: readonly CodingSessionMissionDisclosureInput[],
  label: string,
  truncations: CodingSessionMissionTruncation[],
): CodingSessionMissionDisclosureInput[] {
  return cap(
    input,
    MISSION_INSPECTOR_LIMITS.disclosures,
    "integrity",
    label,
    truncations,
  ).map((item) => ({
    code: item.code,
    summary: item.summary,
    eventIds: cap(
      item.eventIds,
      MISSION_INSPECTOR_LIMITS.disclosureEventIds,
      "integrity",
      "disclosure source events",
      truncations,
    ),
  }));
}

function copyContextLoad(
  value: CodingSessionContextLoad | null,
): CodingSessionContextLoad | null {
  return value ? { ...value } : null;
}

function hasUsage(
  usage: CodingSessionMissionUsageInput | null,
): usage is CodingSessionMissionUsageInput {
  return (
    usage !== null &&
    [
      usage.inputTokens,
      usage.outputTokens,
      usage.totalTokens,
      usage.toolCalls,
      usage.costUsd,
    ].some((value) => value !== null)
  );
}

export {
  codingSessionMissionAskedRelative,
  MAX_CODING_SESSION_MISSION_DECISION_ROWS,
} from "./codingSessionMissionDecisions";
export type {
  CodingSessionMissionDecisionInput,
  CodingSessionMissionDecisionModel,
  CodingSessionMissionWaitingModel,
  CodingSessionMissionWaitingOnDecisionInput,
} from "./codingSessionMissionDecisions";
export {
  codingSessionGoalRejectionSentence,
  codingSessionPrivateContextLine,
  isCodingSessionPrivateContextMarker,
  selectCodingSessionUmbrellaGoal,
} from "./codingSessionMissionGoal";
export type {
  CodingSessionGoalDisagreement,
  CodingSessionGoalLike,
  CodingSessionUmbrellaGoalSelection,
} from "./codingSessionMissionGoal";
