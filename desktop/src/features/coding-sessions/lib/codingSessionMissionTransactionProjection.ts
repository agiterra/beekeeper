import type {
  CodingSessionMissionCanonicalStep,
  CodingSessionMissionInspectorInput,
  CodingSessionMissionStateInput,
} from "./codingSessionMissionInspectorModel";
import type { CodingSessionMissionTransactionInput } from "./codingSessionMissionContracts";
import { CODING_SESSION_MISSION_TRANSACTION_ROW_LIMIT } from "./codingSessionMissionContracts";
import type {
  ImmutableCodingSessionTeamWireEvent,
  NativeCodingSessionTeamFold,
} from "./invokeCodingSessionTeamFold";
import { requireIssuedNativeCodingSessionTeamFold } from "./invokeCodingSessionTeamFold";
import type {
  CodingSessionTeamTest,
  CodingSessionTeamTransactionBody,
  VerifiedCodingSessionTeamTransaction,
} from "./codingSessionTeamTransactionWire";
import { decodeVerifiedCodingSessionTeamTransaction } from "./codingSessionTeamTransactionWire";

/**
 * The wire union's own `decision.answer` arm — **not** a second declaration of
 * it.
 *
 * REVIEW-L7 F4: this file used to hand-write a `DecisionAnswerBody` and cast to
 * it, so the wire type and the projection could drift silently. Extracting the
 * arm keeps one source of truth; `condition` arrives with it.
 */
type DecisionAnswerBody = Extract<
  CodingSessionTeamTransactionBody,
  { requestRef: string; choice: number | string }
>;

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
  assigneeActor: string;
  assigneeRole: string;
  /**
   * The revision the assignee starts from. Required on the wire for a
   * `verifier` or `runner`; optional for every other role, so this stays
   * optional here and an absent value is never replaced with a guess.
   */
  baseSha?: string | null;
  objective: string;
  brief: string;
  fileOwnership: string[];
  acceptanceSteps: string[];
};

type VerdictBody = {
  subtype: "refutation" | "disposition";
  assignmentRef: string;
  reportRef: string;
  decision: string;
  summary: string;
  requiredAction: string | null;
};

type AcknowledgementBody = {
  acknowledgedEventRef: string;
  note: string | null;
};

type NoteBody = {
  text: string;
  refs: string[];
};

type DecisionRequestBody = {
  question: string;
  options: string[];
  heldOn: string;
  recommendation: string | null;
};

/** A 64-hex actor key — the one `heldOn` spelling that names a person. */
const HEX64 = /^[0-9a-f]{64}$/;

/**
 * What a `decision.answer` row says: the option that was chosen, and the
 * answerer's own note where there is one.
 *
 * A numeric `choice` is an index into the request's signed options, so the
 * text comes from the request itself when the fold included it. When it did
 * not, the row names the index rather than inventing the words that went with
 * it — an answer whose question is out of view is still a fact worth showing.
 */
function decisionAnswerSummary(
  body: DecisionAnswerBody,
  optionsByRequestId: ReadonlyMap<string, readonly string[]>,
): string {
  const chosen =
    typeof body.choice === "string"
      ? body.choice
      : (optionsByRequestId.get(body.requestRef)?.[body.choice] ??
        `option ${body.choice + 1}`);
  return body.note === null ? chosen : `${chosen} — ${body.note}`;
}

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

/** Longest verbatim body summary a stream row carries before it is elided. */
const MAX_TRANSACTION_SUMMARY_CHARS = 280;

function boundedSummary(value: string): string {
  return value.length <= MAX_TRANSACTION_SUMMARY_CHARS
    ? value
    : `${value.slice(0, MAX_TRANSACTION_SUMMARY_CHARS - 1)}\u2026`;
}

/**
 * The stream's type for one signed record.
 *
 * `verdict` is the one wire type the surface splits, by its signed `subtype`;
 * every other wire type is a stream type verbatim. The return used to be a
 * cast, and a cast is how `note`, `decision.request` and `decision.answer`
 * reached the surface as types the surface's own tables had never heard of.
 * Without it, tsc names the gap the day the wire grows a verb.
 */
function transactionType(
  event: VerifiedCodingSessionTeamTransaction,
): CodingSessionMissionTransactionInput["type"] {
  if (event.payload.type === "verdict") {
    return (event.payload.body as VerdictBody).subtype;
  }
  return event.payload.type;
}

/**
 * Project every fold-included 44244 as one stream row.
 *
 * Counterparty and parent are read from signed body references and resolved
 * against the included set only — an unresolvable reference stays `null`
 * rather than becoming a guess. `createdAt` orders the rows for display and
 * nothing else; discovery, dedupe, and the wake grace never read it (I4).
 */
function projectTransactions(
  included: readonly VerifiedCodingSessionTeamTransaction[],
  unseatedReportEventIds: ReadonlySet<string>,
): { rows: CodingSessionMissionTransactionInput[]; truncated: number } {
  const authorById = new Map(
    included.map((event) => [event.eventId, event.authorPubkey] as const),
  );
  const optionsByRequestId = new Map(
    included
      .filter((event) => event.payload.type === "decision.request")
      .map(
        (event) =>
          [
            event.eventId,
            (event.payload.body as DecisionRequestBody).options,
          ] as const,
      ),
  );
  const ordered = [...included].sort(compareTransactions);
  const truncated = Math.max(
    0,
    ordered.length - CODING_SESSION_MISSION_TRANSACTION_ROW_LIMIT,
  );
  const rows = ordered
    .slice(-CODING_SESSION_MISSION_TRANSACTION_ROW_LIMIT)
    .map((event) => {
      const body = event.payload.body as Record<string, unknown>;
      const type = transactionType(event);
      const report =
        event.payload.type === "report" ? (body as ReportBody) : null;
      const verdict =
        event.payload.type === "verdict" ? (body as VerdictBody) : null;
      const acknowledgement =
        event.payload.type === "acknowledgement"
          ? (body as AcknowledgementBody)
          : null;
      const assignment =
        event.payload.type === "assignment" ? (body as AssignmentBody) : null;
      const note = event.payload.type === "note" ? (body as NoteBody) : null;
      const decisionRequest =
        event.payload.type === "decision.request"
          ? (body as DecisionRequestBody)
          : null;
      // The wire union's own arm, `condition` included (REVIEW-L7 F4).
      const decisionAnswer =
        event.payload.type === "decision.answer"
          ? (body as DecisionAnswerBody)
          : null;
      // A note's `refs` and a request's `blocks` are pointers, not parents —
      // the wire says so — so neither becomes a parent here. An answer's
      // `requestRef` is causal and does.
      const parentEventId =
        report?.assignmentRef ??
        verdict?.reportRef ??
        acknowledgement?.acknowledgedEventRef ??
        decisionAnswer?.requestRef ??
        null;
      const counterpartyPubkey = assignment
        ? assignment.assigneeActor
        : // A ruling held on a named actor has that actor as its counterparty;
          // one held on `founder` names no key, so it has none.
          decisionRequest && HEX64.test(decisionRequest.heldOn)
          ? decisionRequest.heldOn
          : parentEventId
            ? (authorById.get(parentEventId) ?? null)
            : null;
      return {
        sourceEventId: event.eventId,
        type,
        authorPubkey: event.authorPubkey,
        createdAt: event.createdAt,
        counterpartyPubkey,
        parentEventId,
        summary: boundedSummary(
          assignment
            ? assignment.objective
            : report
              ? report.summary
              : verdict
                ? verdict.summary
                : acknowledgement
                  ? (acknowledgement.note ?? "Disposition received.")
                  : note
                    ? note.text
                    : decisionRequest
                      ? decisionRequest.question
                      : decisionAnswer
                        ? decisionAnswerSummary(
                            decisionAnswer,
                            optionsByRequestId,
                          )
                        : ((body.summary as string | undefined) ?? ""),
        ),
        decision: verdict ? verdict.decision : null,
        requiredAction: verdict
          ? verdict.requiredAction
          : ((body.requiredAction as string | undefined) ?? null),
        fileCount: report ? report.files.length : null,
        testCount: report ? report.tests.length : null,
        unseated: unseatedReportEventIds.has(event.eventId),
        // Carried verbatim off the signed answer, and only off an answer: the
        // decision queue reads the class a ruling covers from here (finding
        // 21). Never parsed — nothing derives state from it.
        condition: decisionAnswer ? decisionAnswer.condition : null,
        // Carried verbatim off the signed assignment body, for the one
        // consumer that needs them: the host that puts a verifier's or a
        // runner's revision into its own seat's tree.
        assigneeRole: assignment ? assignment.assigneeRole : null,
        baseSha: assignment ? (assignment.baseSha ?? null) : null,
      } satisfies CodingSessionMissionTransactionInput;
    });
  return { rows, truncated };
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
  /** The mission's active seats — arm (C) of the push rule needs the roles. */
  activeSeats?: readonly { actorPubkey: string; role: string }[];
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
  const unseatedReportEventIds = fold.unseatedReports.map(
    (report) => report.eventId,
  );
  const transactions = projectTransactions(
    included,
    new Set(unseatedReportEventIds),
  );
  // The signed decision bodies, carried alongside the stream rows. A stream row
  // holds a *summary*; an Answer control needs the request's own declared
  // options and an answered row needs the answer's own choice and condition,
  // and neither has ever been in a summary.
  const decisionRequests = included
    .filter((event) => event.payload.type === "decision.request")
    .map((event) => {
      const body = event.payload.body as DecisionRequestBody;
      return {
        requestId: event.eventId,
        question: body.question,
        options: [...body.options],
        recommendation: body.recommendation ?? null,
        createdAt: event.createdAt,
      };
    });
  const decisionAnswers = included
    .filter((event) => event.payload.type === "decision.answer")
    .map((event) => {
      const body = event.payload.body as DecisionAnswerBody;
      return {
        answerId: event.eventId,
        requestRef: body.requestRef,
        choice: body.choice,
        note: body.note,
        condition: body.condition ?? null,
      };
    });
  return {
    goal: { kind: "absent" },
    // What the push path's own rule needs, carried on the projection because
    // the native predicate reads *signed events*, not a stream row: TypeScript
    // hands them straight back to Rust and inspects none of them (I6).
    landEvidence: {
      includedEventIds: [...fold.includedEventIds],
      wireEvents,
      activeSeats: (input.activeSeats ?? []).map((seat) => ({ ...seat })),
    },
    channelRef: fold.context.channelRef,
    sessionRef: fold.context.sessionRef,
    genesisRef: fold.context.genesisRef,
    decisionRequests,
    decisionAnswers,
    acceptedPlan,
    assignments,
    seatPlans: [],
    reports,
    transactions: transactions.rows,
    transactionsTruncated: transactions.truncated,
    unseatedReportEventIds,
    // Carried through verbatim. Item 105 put both on the wire and nothing
    // rendered them for a batch; the fold decides what is waiting and what was
    // asked, and this layer copies the answer rather than forming one.
    decisions: fold.decisions.map((decision) => ({
      requestId: decision.requestId,
      heldOn: decision.heldOn,
      blocks: [...decision.blocks],
      answeredBy: decision.answeredBy,
      answerId: decision.answerId,
    })),
    waitingOnDecision:
      fold.waitingOnDecision === null ? null : { ...fold.waitingOnDecision },
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
