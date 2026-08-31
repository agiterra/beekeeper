import type {
  CodingSessionMissionCanonicalStep,
  CodingSessionMissionInspectorInput,
  CodingSessionMissionStateInput,
} from "./codingSessionMissionInspectorModel";
import type {
  ImmutableCodingSessionTeamWireEvent,
  NativeCodingSessionTeamFold,
} from "./invokeCodingSessionTeamFold";
import { requireIssuedNativeCodingSessionTeamFold } from "./invokeCodingSessionTeamFold";
import type {
  CodingSessionTeamTest,
  VerifiedCodingSessionTeamTransaction,
} from "./codingSessionTeamTransactionWire";
import { decodeVerifiedCodingSessionTeamTransaction } from "./codingSessionTeamTransactionWire";

type ReportBody = {
  assignmentRef: string;
  summary: string;
  branch: string | null;
  baseSha: string | null;
  headSha: string | null;
  files: string[];
  tests: CodingSessionTeamTest[];
};

type CompletedBody = {
  summary: string;
  landedShas: string[];
  followUps: string[];
};

type BlockedBody = {
  summary: string;
  blockers: string[];
  requiredAction: string;
};

type AssignmentBody = {
  assigneeRole: string;
  objective: string;
  brief: string;
  fileOwnership: string[];
  acceptanceSteps: string[];
};

type VerdictBody = {
  subtype: "refutation" | "disposition";
  assignmentRef: string;
  decision: string;
  summary: string;
  requiredAction: string | null;
};

type AcknowledgementBody = {
  acknowledgedEventRef: string;
  note: string | null;
};

function decodeFrozenWireEvent(input: {
  event: ImmutableCodingSessionTeamWireEvent;
  channelRef: string;
  sessionRef: string;
  genesisRef: string;
}): VerifiedCodingSessionTeamTransaction {
  const decoded = decodeVerifiedCodingSessionTeamTransaction({
    event: {
      id: input.event.id,
      pubkey: input.event.pubkey,
      created_at: input.event.created_at,
      kind: input.event.kind,
      tags: input.event.tags.map((tag) => [...tag]),
      content: input.event.content,
      sig: input.event.sig,
    },
    channelRef: input.channelRef,
    sessionRef: input.sessionRef,
    genesisRef: input.genesisRef,
  });
  if (!decoded.ok) {
    throw new Error(
      `native fold included an invalid signed wire event: ${decoded.error}`,
    );
  }
  return decoded.value;
}

function compareTransactions(
  left: VerifiedCodingSessionTeamTransaction,
  right: VerifiedCodingSessionTeamTransaction,
): number {
  return (
    left.createdAt - right.createdAt ||
    left.eventId.localeCompare(right.eventId)
  );
}

function canonicalStep(
  event: VerifiedCodingSessionTeamTransaction,
): CodingSessionMissionCanonicalStep {
  const body = event.payload.body;
  if (event.payload.type === "assignment") {
    return {
      type: "assignment",
      sourceEventId: event.eventId,
      authorPubkey: event.authorPubkey,
      createdAt: event.createdAt,
      summary: (body as AssignmentBody).objective,
    };
  }
  if (event.payload.type === "report") {
    return {
      type: "report",
      sourceEventId: event.eventId,
      authorPubkey: event.authorPubkey,
      createdAt: event.createdAt,
      summary: (body as ReportBody).summary,
    };
  }
  if (event.payload.type === "verdict") {
    const verdict = body as VerdictBody;
    return {
      type: verdict.subtype,
      sourceEventId: event.eventId,
      authorPubkey: event.authorPubkey,
      createdAt: event.createdAt,
      summary: verdict.summary,
      decision: verdict.decision,
      requiredAction: verdict.requiredAction,
    };
  }
  const acknowledgement = body as AcknowledgementBody;
  return {
    type: "acknowledgement",
    sourceEventId: event.eventId,
    authorPubkey: event.authorPubkey,
    createdAt: event.createdAt,
    summary: acknowledgement.note ?? "Disposition received.",
  };
}

function projectCanonicalNonterminalState(input: {
  included: readonly VerifiedCodingSessionTeamTransaction[];
  assignmentEventIds: ReadonlySet<string>;
}): CodingSessionMissionStateInput {
  const assignments = input.included.filter(
    (event) =>
      event.payload.type === "assignment" &&
      input.assignmentEventIds.has(event.eventId),
  );
  if (assignments.length === 0) {
    return {
      kind: "unknown",
      detail:
        "The native canonical fold has no terminal or active assignment; Mission does not infer running or completion from silence.",
    };
  }

  const relevant = input.included.filter((event) => {
    if (event.payload.type === "assignment") {
      return input.assignmentEventIds.has(event.eventId);
    }
    if (event.payload.type === "report" || event.payload.type === "verdict") {
      return input.assignmentEventIds.has(
        (event.payload.body as ReportBody | VerdictBody).assignmentRef,
      );
    }
    return false;
  });
  const relevantVerdictIds = new Set(
    relevant
      .filter((event) => event.payload.type === "verdict")
      .map((event) => event.eventId),
  );
  const acknowledgements = input.included.filter(
    (event) =>
      event.payload.type === "acknowledgement" &&
      relevantVerdictIds.has(
        (event.payload.body as AcknowledgementBody).acknowledgedEventRef,
      ),
  );
  const accepted = [...relevant, ...acknowledgements].sort(compareTransactions);
  const canonicalChain = accepted.map(canonicalStep);
  const acknowledgedVerdicts = new Set(
    acknowledgements.map(
      (event) =>
        (event.payload.body as AcknowledgementBody).acknowledgedEventRef,
    ),
  );
  const pendingApprovals = relevant
    .filter((event) => {
      if (event.payload.type !== "verdict") return false;
      const verdict = event.payload.body as VerdictBody;
      return (
        verdict.subtype === "disposition" &&
        ["approve", "approve-with-notes"].includes(verdict.decision) &&
        !acknowledgedVerdicts.has(event.eventId)
      );
    })
    .sort(compareTransactions);
  const pendingApproval = pendingApprovals.at(-1);
  if (pendingApproval) {
    const verdict = pendingApproval.payload.body as VerdictBody;
    const assignment = assignments.find(
      (event) => event.eventId === verdict.assignmentRef,
    );
    const role = assignment
      ? (assignment.payload.body as AssignmentBody).assigneeRole
      : null;
    return {
      kind: "acknowledgement-required",
      sourceEventId: pendingApproval.eventId,
      assignmentRef: verdict.assignmentRef,
      requiredAction: role
        ? `The assigned ${role} seat must acknowledge this approved disposition.`
        : "The assigned seat must acknowledge this approved disposition.",
      heldOn: role,
      canonicalChain,
    };
  }

  const requiredActionVerdict = relevant
    .filter(
      (event) =>
        event.payload.type === "verdict" &&
        Boolean((event.payload.body as VerdictBody).requiredAction),
    )
    .sort(compareTransactions)
    .at(-1);
  if (requiredActionVerdict) {
    const verdict = requiredActionVerdict.payload.body as VerdictBody;
    return {
      kind: "waiting-on-person",
      sourceEventId: requiredActionVerdict.eventId,
      requiredAction: verdict.requiredAction as string,
      heldOn: null,
      canonicalChain,
    };
  }

  const last = accepted.at(-1) as VerifiedCodingSessionTeamTransaction;
  const phase = acknowledgements.length
    ? "acknowledged"
    : relevant.some((event) => event.payload.type === "verdict")
      ? "ruled"
      : relevant.some((event) => event.payload.type === "report")
        ? "reported"
        : "assigned";
  const detail =
    phase === "acknowledged"
      ? "The accepted disposition has been acknowledged; no terminal mission record has been published."
      : phase === "ruled"
        ? "The canonical fold contains an accepted verdict; no terminal mission record has been published."
        : phase === "reported"
          ? "The canonical fold contains an accepted report awaiting disposition."
          : "The canonical fold contains active assignment work.";
  return {
    kind: "running",
    sourceEventId: last.eventId,
    phase,
    detail,
    canonicalChain,
  };
}

/**
 * Map the response returned and request-bound by the native wrapper. This
 * layer performs no transaction semantics: correction, causal exclusion,
 * settlement, conflict, and terminal truth come only from buzz-core's native
 * `fold_coding_session_team_transactions` implementation.
 */
export function projectNativeTeamFoldToMissionInspector(input: {
  nativeFold: NativeCodingSessionTeamFold;
  ingressRejections?: readonly string[];
}): CodingSessionMissionInspectorInput {
  const { fold, wireEvents } = requireIssuedNativeCodingSessionTeamFold(
    input.nativeFold,
  );
  const includedIds = new Set(fold.includedEventIds);
  const byId = new Map(
    wireEvents
      .filter((event) => includedIds.has(event.id))
      .map((event) => {
        const decoded = decodeFrozenWireEvent({
          event,
          channelRef: fold.context.channelRef,
          sessionRef: fold.context.sessionRef,
          genesisRef: fold.context.genesisRef,
        });
        return [decoded.eventId, decoded] as const;
      }),
  );
  const included = fold.includedEventIds
    .map((id) => byId.get(id))
    .filter((event): event is VerifiedCodingSessionTeamTransaction =>
      Boolean(event),
    );
  const reports = included
    .filter((event) => event.payload.type === "report")
    .map((event) => {
      const body = event.payload.body as ReportBody;
      return {
        sourceEventId: event.eventId,
        authorPubkey: event.authorPubkey,
        sourceCreatedAt: event.createdAt,
        deliveryCommandId: event.payload.deliveryCommandId,
        authorLabel: event.authorPubkey,
        summary: body.summary,
        assignmentRef: body.assignmentRef,
        branch: body.branch,
        baseSha: body.baseSha,
        headSha: body.headSha,
        files: [...body.files],
        tests: body.tests.map((test) => ({ ...test })),
      };
    });
  const assignmentEventIds = new Set(
    fold.assignments.map((assignment) => assignment.assignmentEventId),
  );
  const assignmentEvents = included
    .filter(
      (event) =>
        event.payload.type === "assignment" &&
        assignmentEventIds.has(event.eventId),
    )
    .sort(compareTransactions);
  const assignments = assignmentEvents.map((event) => {
    const body = event.payload.body as AssignmentBody;
    return {
      sourceEventId: event.eventId,
      authorLabel: event.authorPubkey,
      assigneeRole: body.assigneeRole,
      objective: body.objective,
      brief: body.brief,
      fileOwnership: [...body.fileOwnership],
    };
  });
  const acceptedPlan =
    assignmentEvents.length === 0
      ? ({ kind: "absent" } as const)
      : {
          kind: "available" as const,
          steps: assignmentEvents.flatMap((event) =>
            (event.payload.body as AssignmentBody).acceptanceSteps.map(
              (text, sourceIndex) => ({
                text,
                sourceEventId: event.eventId,
                authorLabel: event.authorPubkey,
                sourceCreatedAt: event.createdAt,
                sourceIndex,
              }),
            ),
          ),
        };
  const terminal = fold.canonicalTerminal
    ? byId.get(fold.canonicalTerminal.eventId)
    : null;
  const canonicalNonterminal = projectCanonicalNonterminalState({
    included,
    assignmentEventIds,
  });
  const canonicalChain =
    "canonicalChain" in canonicalNonterminal
      ? (canonicalNonterminal.canonicalChain ?? [])
      : [];
  const missionState: CodingSessionMissionInspectorInput["missionState"] =
    terminal?.payload.type === "mission.completed"
      ? {
          kind: "completed",
          sourceEventId: terminal.eventId,
          summary: (terminal.payload.body as CompletedBody).summary,
          landedShas: [...(terminal.payload.body as CompletedBody).landedShas],
          followUps: [...(terminal.payload.body as CompletedBody).followUps],
          canonicalChain,
        }
      : terminal?.payload.type === "mission.blocked"
        ? {
            kind: "blocked",
            sourceEventId: terminal.eventId,
            summary: (terminal.payload.body as BlockedBody).summary,
            blockers: [...(terminal.payload.body as BlockedBody).blockers],
            requiredAction: (terminal.payload.body as BlockedBody)
              .requiredAction,
            canonicalChain,
          }
        : canonicalNonterminal;
  const ingressRejections = input.ingressRejections ?? [];
  return {
    goal: { kind: "absent" },
    acceptedPlan,
    assignments,
    seatPlans: [],
    reports,
    observedChanges: { files: [], unreportedEditCount: 0 },
    observedFileSources: new Map(),
    participants: [],
    contextLoads: new Map(),
    missionState,
    usage: null,
    rejectedEventCount: fold.excluded.length + ingressRejections.length,
    rejectionsTruncated: false,
    rejectedReasons: [
      ...fold.excluded.map((item) => ({
        code: item.code,
        summary: item.reason,
        eventIds: [item.eventId],
      })),
      ...ingressRejections.map((summary, index) => ({
        code: `INGRESS_REJECTED_${index + 1}`,
        summary,
        eventIds: [],
      })),
    ],
    conflicts: fold.conflicts.map((conflict) => ({
      code: conflict.subject,
      summary: `Native canonical winner ${conflict.winnerEventId}`,
      eventIds: [...conflict.contenderEventIds],
    })),
  };
}
