import type { CodingSessionMissionInspectorInput } from "./codingSessionMissionInspectorModel";
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
  const terminal = fold.canonicalTerminal
    ? byId.get(fold.canonicalTerminal.eventId)
    : null;
  const missionState: CodingSessionMissionInspectorInput["missionState"] =
    terminal?.payload.type === "mission.completed"
      ? {
          kind: "completed",
          sourceEventId: terminal.eventId,
          summary: (terminal.payload.body as CompletedBody).summary,
          landedShas: [...(terminal.payload.body as CompletedBody).landedShas],
          followUps: [...(terminal.payload.body as CompletedBody).followUps],
        }
      : terminal?.payload.type === "mission.blocked"
        ? {
            kind: "blocked",
            sourceEventId: terminal.eventId,
            summary: (terminal.payload.body as BlockedBody).summary,
            blockers: [...(terminal.payload.body as BlockedBody).blockers],
            requiredAction: (terminal.payload.body as BlockedBody)
              .requiredAction,
          }
        : {
            kind: "unknown",
            detail:
              "The native canonical fold has no terminal; Mission does not infer running or completion from silence.",
          };
  const ingressRejections = input.ingressRejections ?? [];
  return {
    goal: { kind: "absent" },
    acceptedPlan: { kind: "absent" },
    seatPlans: [],
    reports,
    observedChanges: { files: [], unreportedEditCount: 0 },
    observedFileSources: new Map(),
    participants: [],
    contextLoads: new Map(),
    missionState,
    usage: null,
    rejectedEventCount: fold.excluded.length + ingressRejections.length,
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
