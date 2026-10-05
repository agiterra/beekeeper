import * as React from "react";

import { codingSessionObservationNotLive } from "@/features/coding-sessions/lib/codingSessionObservationLiveness";
import { formatCodingSessionSurfaceClock } from "@/features/coding-sessions/lib/codingSessionSurfaceBadgeModelCtx";

import type { CodingSessionContextLoad } from "@/features/coding-sessions/lib/codingSessionContextLoad";
import type {
  CodingSessionGoal,
  CodingSessionGoalReader,
} from "@/features/coding-sessions/lib/codingSessionGoal";
import type {
  CodingSessionMissionTransactionInput,
  CodingSessionSeatAuthority,
  CodingSessionTeamWakeDelivery,
} from "@/features/coding-sessions/lib/codingSessionMissionContracts";
import {
  deriveCodingSessionMissionInspectorModel,
  selectCodingSessionUmbrellaGoal,
  type CodingSessionMissionInspectorInput,
} from "@/features/coding-sessions/lib/codingSessionMissionInspectorModel";
import { mergeCodingSessionMissionWorkspaceInput } from "@/features/coding-sessions/lib/codingSessionMissionWorkspaceModel";
import { projectCodingSessionMissionState } from "@/features/coding-sessions/lib/codingSessionMissionStateProjection";
import type { CodingSessionMissionDecisionInput } from "@/features/coding-sessions/lib/codingSessionMissionDecisions";
import type { CodingSessionParticipantPresence } from "@/features/coding-sessions/lib/codingSessionStreamPresence";
import { deriveCodingSessionTaskModel } from "@/features/coding-sessions/lib/codingSessionTaskModel";
import type { CodingSessionObservedChanges } from "@/features/coding-sessions/lib/codingSessionTranscriptModel";
import type { CodingSessionUmbrellaRecord } from "@/features/coding-sessions/lib/codingSessionTypes";
import type { CodingSessionActorNameResolver } from "@/features/coding-sessions/lib/codingSessionUmbrellaModel";
import { CODING_SESSION_UNKNOWN_ACTOR } from "@/features/coding-sessions/lib/codingSessionTurnByline";
import { useStableArrayShallow } from "@/shared/hooks/useStableReference";
import { useCodingSessionMissionEvidence } from "@/features/coding-sessions/lib/useCodingSessionMissionEvidence";
import {
  buildCodingSessionWakeOperationIndex,
  readCachedCodingSessionWakeOperations,
  rememberCodingSessionWakeOperations,
  type CodingSessionWakeOperationIndex,
} from "@/features/coding-sessions/lib/codingSessionWakeReading";
import { deriveCodingSessionObservationView } from "@/features/coding-sessions/lib/codingSessionObservationView";
import {
  codingSessionObservationFoldUnchecked,
  codingSessionSurfaceEvidenceScope,
} from "./surfaces/useCodingSessionSurfaceTeamRead";
import type { CodingSessionRouteGateRow } from "@/features/coding-sessions/lib/codingSessionRouteModel";
import { useCodingSessionObservations } from "@/features/coding-sessions/hooks/useCodingSessionObservations";
import { useIdentityQuery } from "@/shared/api/hooks";
import { truncatePubkey } from "@/shared/lib/pubkey";
import { useCodingSessionMissionLand } from "@/features/coding-sessions/hooks/useCodingSessionMissionLand";
import { useCodingSessionSessionPolicy } from "@/features/coding-sessions/hooks/useCodingSessionSessionPolicy";
import { deriveCodingSessionSubagentPanel } from "@/features/coding-sessions/lib/codingSessionSubagents";
import { resolveCodingSessionGenerationCallSettlement } from "@/features/coding-sessions/lib/codingSessionTranscriptModelSettlement";
import { shouldAutoOpenAgentsSurface } from "./CodingSessionUmbrellaWorkspaceModel";
import { CodingSessionMissionAudit } from "./CodingSessionMissionAudit";
import { CodingSessionMissionContext } from "./CodingSessionMissionContext";
import { useProjectWork } from "../hooks/useProjectWork";
import { CodingSessionMissionInspector } from "./CodingSessionMissionInspector";
import {
  type CodingSessionSurfaceBaseCtx,
  type CodingSessionSurfaceMissionContent,
  type CodingSessionSurfaceObservations,
  codingSessionSurfaceDecisionRequestRows,
} from "./surfaces/codingSessionSurfaceContext";

export type CodingSessionMissionSurfaceResult = {
  /** Inspector, Context and Audit for the registry's Mission surfaces. */
  missionContent: CodingSessionSurfaceMissionContent | null;
  /**
   * The view's one kind-44246 read, for every surface's `ctx`. Scoped by the
   * genesis alone, so Conversation reads it too and Mission reuses it.
   */
  observationRead: CodingSessionSurfaceObservations;
  /**
   * The Mission evidence (assignments, transactions) has been read for this
   * session. Read in both lenses whenever there is a genesis and a founder,
   * so `ctx.openRulings` can be built outside Mission; false while it loads,
   * when it failed, or when there is nothing to scope it by.
   */
  teamEvidenceRead: boolean;
  /**
   * The Mission fold's `decisions[]`, verbatim, for `ctx.decisions` (DB8).
   * `null` when no fold supplied them — unknown, not empty. Read it only
   * with {@link teamEvidenceRead}.
   */
  decisions: readonly CodingSessionMissionDecisionInput[] | null;
  /**
   * The signed `decision.request` rows that date {@link decisions}, for
   * `ctx.decisionRequests` (SV-27). Read it only with {@link teamEvidenceRead}.
   */
  decisionRequests: CodingSessionSurfaceBaseCtx["decisionRequests"];
  missionState: ReturnType<
    typeof deriveCodingSessionMissionInspectorModel
  >["missionState"];
  /** Signed 44244 transactions for the stream's causality plane. */
  transactions: readonly CodingSessionMissionTransactionInput[];
  /** Report ids the Rust fold listed under `unseatedReports`. */
  unseatedReportEventIds: readonly string[];
  /**
   * The fold's operations, keyed by event id, for the wake reading (§1f).
   *
   * Built from the same fold rows the stream renders and nothing else, so a
   * wake line can never say more than the signed record does.
   */
  wakeOperations: CodingSessionWakeOperationIndex;
  /**
   * Folded kind-44246 gate rows, for the Route rail's signs (L5.6).
   *
   * Empty while Mission is closed: this surface reads no observations then,
   * and the rail draws no sign for a fact nothing has folded.
   */
  observationGates: readonly CodingSessionRouteGateRow[];
};

// U-F8: shared frozen empties. Returning fresh `[]` literals made
// `transactions` change identity on every `inspectorInput` change, which
// propagates through the hook's result into every consumer's memo deps —
// exactly the render-stability trap AGENTS.md § "React render perf" names.
const NO_TRANSACTIONS: readonly CodingSessionMissionTransactionInput[] =
  Object.freeze([]);
const NO_UNSEATED_REPORT_IDS: readonly string[] = Object.freeze([]);

/**
 * Reads the stream's half of the Mission evidence projection.
 *
 * Both fields are optional on {@link CodingSessionMissionInspectorInput}
 * because an empty or errored projection supplies neither, and absent is not
 * the same fact as empty — so this normalises absence to the shared frozen
 * empties rather than letting `undefined` reach the stream.
 */
export function readCodingSessionMissionStreamEvidence(
  inspectorInput: CodingSessionMissionInspectorInput,
): {
  transactions: readonly CodingSessionMissionTransactionInput[];
  unseatedReportEventIds: readonly string[];
} {
  return {
    transactions: inspectorInput.transactions ?? NO_TRANSACTIONS,
    unseatedReportEventIds:
      inspectorInput.unseatedReportEventIds ?? NO_UNSEATED_REPORT_IDS,
  };
}

/** Build Mission's canonical inspector surface without subscribing in Conversation. */
export function useCodingSessionMissionSurface(input: {
  active: boolean;
  channelId: string;
  contextLoads: readonly {
    key: string;
    load: CodingSessionContextLoad | null;
  }[];
  /** Team-wake delivery evidence from Lane D's hook; the finalizer supplies it. */
  deliveries?: readonly CodingSessionTeamWakeDelivery[];
  focusedExecutionKey: string | null;
  goal: CodingSessionGoal | null;
  /** The founder's goal edit control, rendered inside Current goal. */
  goalEditor?: React.ReactNode;
  goalReader: CodingSessionGoalReader;
  isNarrow: boolean;
  observedChanges: CodingSessionObservedChanges;
  onFocusParticipant: (executionKey: string | null) => void;
  onOpenTrace: () => void;
  participants: readonly CodingSessionParticipantPresence[];
  resolveActorName: CodingSessionActorNameResolver;
  /** Seat authority per execution from Lane D's accepted-44228 projection. */
  seatAuthorities?: readonly CodingSessionSeatAuthority[];
  umbrella: CodingSessionUmbrellaRecord;
}): CodingSessionMissionSurfaceResult {
  const scope = React.useMemo(
    () =>
      input.active &&
      input.umbrella.sessionRef !== null &&
      input.umbrella.genesisRef !== null &&
      input.umbrella.founderPubkey !== null
        ? {
            channelRef: input.channelId,
            sessionRef: input.umbrella.sessionRef,
            genesisRef: input.umbrella.genesisRef,
            founderPubkey: input.umbrella.founderPubkey,
          }
        : null,
    [
      input.active,
      input.channelId,
      input.umbrella.founderPubkey,
      input.umbrella.genesisRef,
      input.umbrella.sessionRef,
    ],
  );
  // Item 107's owed reader. Scoped to the same channel/session/genesis/founder
  // the fold uses, and read only while Mission is open — a one-shot read with
  // a refresh, never a poll (I1). Read before `evidence` so its
  // `gates.verifierRequired` (codingSessionPolicy.ts:91) can reach the native
  // fold rather than folding with a permanent `false` (L8.3).
  const policy = useCodingSessionSessionPolicy(scope);
  // B0 (SV-38): the evidence itself is scoped by the genesis, not the lens,
  // so `ctx.openRulings` exists in Conversation too (B3's waiting tone, B5's
  // ruling marks). One read per view: Mission reuses this one.
  const evidenceScope = React.useMemo(
    () =>
      codingSessionSurfaceEvidenceScope(input.channelId, {
        founderPubkey: input.umbrella.founderPubkey,
        genesisRef: input.umbrella.genesisRef,
        sessionRef: input.umbrella.sessionRef,
      }),
    [
      input.channelId,
      input.umbrella.founderPubkey,
      input.umbrella.genesisRef,
      input.umbrella.sessionRef,
    ],
  );
  // L8.3: the real `gates.verifierRequired`, and — separately — whether a
  // 44245 reached this view at all. Unknown is not false: with no record the
  // fold reads `false` exactly as it did before, and the state panel says so.
  const policyRecordKnown = policy.fold?.selected != null;
  const verifierRequired =
    policy.fold?.selected?.record.gates?.verifierRequired ?? null;
  const evidence = useCodingSessionMissionEvidence(
    evidenceScope,
    undefined,
    verifierRequired,
  );
  const teamEvidenceRead =
    evidenceScope !== null &&
    !evidence.isLoading &&
    evidence.errorMessage === null;
  // The assignments a 44246 `assignmentRef` may resolve against, taken from
  // the Mission fold this surface already holds. Not re-fetched: the only
  // thing the observation fold does with a pointer is disclose the ones that
  // resolve to nothing, and a second relay read would buy one word of
  // disclosure at the price of a round trip.
  const knownAssignmentRefs = useStableArrayShallow(
    (evidence.inspectorInput.assignments ?? []).map(
      (assignment) => assignment.sourceEventId,
    ),
  );
  // B0: scoped by the genesis, not by the lens, so the badges and Landing
  // read the same one fold in Conversation that Mission's Audit renders.
  const observationScope = React.useMemo(
    () =>
      input.umbrella.sessionRef === null || input.umbrella.genesisRef === null
        ? null
        : {
            channelRef: input.channelId,
            sessionRef: input.umbrella.sessionRef,
            genesisRef: input.umbrella.genesisRef,
          },
    [input.channelId, input.umbrella.genesisRef, input.umbrella.sessionRef],
  );
  // REVIEW-L5 F2. Every execution's `signerPubkey` is "the fact-stream signer
  // (provider authority) behind this execution" (`codingSessionTypes.ts`), so
  // this is exactly the set whose `observed` claim this session honours. A
  // signer outside it is folded down to `declared` and disclosed rather than
  // rendered as a watched measurement.
  const providerPubkeys = useStableArrayShallow(
    input.umbrella.executions
      .map((execution) => execution.signerPubkey)
      .filter((pubkey) => pubkey.length > 0),
  );
  const observations = useCodingSessionObservations(
    observationScope,
    knownAssignmentRefs,
    providerPubkeys,
  );
  // The viewer's own key. §1l's Answer control is disabled for anyone who is
  // not the party a ruling is held on, and §1f's `you` depends on it too;
  // without it every row would read as somebody else's and every control
  // would be enabled for everyone.
  const viewerPubkey = useIdentityQuery().data?.pubkey ?? null;
  // L8.2: the repository this session's creates actually named. One address
  // or none — executions that disagree name no single repository, and the
  // control then says the session names none rather than picking one (F4).
  const repoRef = React.useMemo(() => {
    const named = new Set(
      input.umbrella.executions
        .map((execution) => execution.activeGeneration.repoRef?.trim() ?? "")
        .filter((ref) => ref.length > 0),
    );
    return named.size === 1 ? [...named][0] : null;
  }, [input.umbrella.executions]);
  // LANE-L20 item 2: the project this session's creates named, one address or
  // none — the same one-or-none rule `repoRef` above already uses, since
  // executions that disagree name no single project either.
  const projectRef = React.useMemo(() => {
    const named = new Set(
      input.umbrella.executions
        .map((execution) => execution.activeGeneration.projectRef?.trim() ?? "")
        .filter((ref) => ref.length > 0),
    );
    return named.size === 1 ? [...named][0] : null;
  }, [input.umbrella.executions]);
  // L8.2: the push path's own rule, asked once per fold, over the repository's
  // own kind:30617 when one is named and readable.
  // Frozen shapes, memoised on content, so the Land effect is not re-run on
  // every render by a fresh array (the `React.memo` lesson in AGENTS.md).
  const landObservedGates = React.useMemo(
    () =>
      (observations.result?.fold.gates ?? []).map((row) => ({
        authorPubkey: row.authorPubkey,
        source: row.source,
        gate: row.gate,
        outcome: row.outcome,
        headSha: row.headSha,
        dirty: row.dirty,
      })),
    [observations.result],
  );
  const landGatePolicy = React.useMemo(
    () =>
      policyRecordKnown
        ? {
            verifierRequired,
            requiredGates:
              policy.fold?.selected?.record.gates?.requiredGates ?? null,
          }
        : null,
    [
      policy.fold?.selected?.record.gates?.requiredGates,
      policyRecordKnown,
      verifierRequired,
    ],
  );
  // NIP-PW: the session's work coverage, folded natively. The scope is the
  // facts this surface already holds — its channel, session, project and
  // founder, plus the accepted-44228 seats it renders elsewhere. The agents
  // checkout is **not** passed: the native side resolves this host's own
  // record for the project, so a seat's worktree cannot become the source of
  // a contract. `null` scope means the question cannot be asked, and the
  // panel says unknown rather than "nothing remains".
  const workScope = React.useMemo(() => {
    if (
      !input.active ||
      input.umbrella.sessionRef === null ||
      input.umbrella.genesisRef === null ||
      input.umbrella.founderPubkey === null ||
      projectRef === null
    ) {
      return null;
    }
    return {
      channelRef: input.channelId,
      sessionRef: input.umbrella.sessionRef,
      genesisRef: input.umbrella.genesisRef,
      projectRef,
      founderPubkey: input.umbrella.founderPubkey,
      activeSeats: (input.seatAuthorities ?? [])
        .filter((authority) => authority.kind === "granted")
        .flatMap((authority) =>
          authority.actorPubkey && authority.role
            ? [{ actorPubkey: authority.actorPubkey, role: authority.role }]
            : [],
        ),
      activeGrants: [],
      repositoryIds:
        repoRef === null ? [] : [repoRef.slice(repoRef.lastIndexOf(":") + 1)],
    };
  }, [
    input.active,
    input.channelId,
    input.seatAuthorities,
    input.umbrella.founderPubkey,
    input.umbrella.genesisRef,
    input.umbrella.sessionRef,
    projectRef,
    repoRef,
  ]);
  const workCoverage = useProjectWork(workScope);
  const { land, unavailableReason: landUnavailableReason } =
    useCodingSessionMissionLand({
      founderPubkey: input.umbrella.founderPubkey,
      genesisRef: input.umbrella.genesisRef,
      landEvidence: evidence.inspectorInput.landEvidence,
      // Arm (B): the same folded gate rows the Audit tab renders, and the
      // same founder-signed policy the state panel reads. Passed rather than
      // re-derived, so the rule the screen asks is answered over exactly what
      // the screen shows.
      observedGates: landObservedGates,
      gatePolicy: landGatePolicy,
      repoRef,
      // LANE-L20 item 2: the read fallback that infers a repository from the
      // session's project when its own creates named none.
      projectRef,
      resolveWho: (pubkey) =>
        pubkey.trim().toLowerCase() ===
        (input.umbrella.founderPubkey ?? "").trim().toLowerCase()
          ? "the founder"
          : (input.resolveActorName(pubkey) ?? truncatePubkey(pubkey)),
      sessionRef: input.umbrella.sessionRef,
      viewerPubkey,
    });
  // The same case-folded selection the workspace made, applied again here so
  // this surface trusts a goal for the reasons it can check rather than on
  // three exact-equality comparisons that finding 23 showed can each miss.
  const goalSelection = selectCodingSessionUmbrellaGoal({
    channelId: input.channelId,
    founderPubkey: input.umbrella.founderPubkey,
    goals: input.goal === null ? [] : [input.goal],
    sessionRef: input.umbrella.sessionRef,
  });
  const trustedGoal =
    goalSelection.kind === "available" ? goalSelection.goal : null;
  // F6: `disagreements` is rebuilt by every call and is a `useMemo` dependency
  // below, so a fresh array meant `inspectorInput` — and with it the entire
  // Inspector model — re-derived on **every** render of every Mission surface.
  // The content-equality cache is the repo's own answer to exactly this trap
  // (AGENTS.md § "React render perf").
  const goalDisagreements = useStableArrayShallow(goalSelection.disagreements);
  const contextLoads = React.useMemo(
    () => new Map(input.contextLoads.map((entry) => [entry.key, entry.load])),
    [input.contextLoads],
  );
  const seatPlans = React.useMemo(() => {
    const labels = new Map(
      input.participants.map((participant) => [
        participant.executionKey,
        participant.label,
      ]),
    );
    return input.umbrella.executions.map((execution) => ({
      executionKey: execution.executionKey,
      ownerLabel:
        labels.get(execution.executionKey) ?? CODING_SESSION_UNKNOWN_ACTOR,
      model: deriveCodingSessionTaskModel(
        execution.activeGeneration.transcript,
      ),
    }));
  }, [input.participants, input.umbrella.executions]);
  const missionState = React.useMemo(() => {
    const labels = new Map(
      input.participants.map((participant) => [
        participant.executionKey,
        participant.label,
      ]),
    );
    return projectCodingSessionMissionState({
      canonical: evidence.inspectorInput.missionState,
      seats: input.umbrella.executions.map((execution) => ({
        label:
          labels.get(execution.executionKey) ?? CODING_SESSION_UNKNOWN_ACTOR,
        record: execution.activeGeneration,
      })),
    });
  }, [
    evidence.inspectorInput.missionState,
    input.participants,
    input.umbrella.executions,
  ]);
  // Only this surface knows whether the lead is mid-turn, and that is the one
  // qualifier the fold's waiting state needs: a mission whose lead is working
  // is waiting *and* running, and the rail says both (finding 16's ruling).
  const leadHasOpenTurn = React.useMemo(
    () =>
      input.participants.some(
        (participant) =>
          participant.role?.trim().toLowerCase() === "lead" &&
          participant.status.kind === "working",
      ),
    [input.participants],
  );
  const inspectorInput = React.useMemo(
    () => ({
      ...mergeCodingSessionMissionWorkspaceInput({
        evidence: { ...evidence.inspectorInput, missionState },
        goal: trustedGoal,
        goalAuthorLabel:
          (trustedGoal && input.resolveActorName(trustedGoal.founderPubkey)) ??
          "Founder",
        observedChanges: input.observedChanges,
        participants: input.participants,
        contextLoads,
        seatPlans,
      }),
      // Critique A1: a 44227 this surface refused on identity is not silence,
      // and the card must not answer it with `Set goal`. Overrides the merged
      // `goal` only in that one case; every other path is untouched.
      ...(goalSelection.kind === "rejected"
        ? {
            goal: {
              kind: "rejected" as const,
              disagreements: goalDisagreements,
            },
          }
        : {}),
      currentUserPubkey: viewerPubkey,
      founderPubkey: input.umbrella.founderPubkey,
      land,
      // LANE-L20 item 3 (finding 37): why `land` is null, when it is.
      landUnavailableReason,
      leadHasOpenTurn,
      policyRecordKnown,
      resolveActorLabel: input.resolveActorName,
    }),
    [
      evidence.inspectorInput,
      land,
      landUnavailableReason,
      policyRecordKnown,
      viewerPubkey,
      contextLoads,
      input.observedChanges,
      input.participants,
      input.resolveActorName,
      input.umbrella.founderPubkey,
      leadHasOpenTurn,
      seatPlans,
      missionState,
      goalSelection.kind,
      goalDisagreements,
      trustedGoal,
    ],
  );
  const model = React.useMemo(
    () => deriveCodingSessionMissionInspectorModel(inspectorInput),
    [inspectorInput],
  );
  const pending = React.useMemo(
    () => readCodingSessionMissionStreamEvidence(evidence.inspectorInput),
    [evidence.inspectorInput],
  );
  const foldedWakeOperations = React.useMemo(
    () =>
      buildCodingSessionWakeOperationIndex({
        assignments: evidence.inspectorInput.assignments ?? [],
        transactions: pending.transactions,
      }),
    [evidence.inspectorInput.assignments, pending.transactions],
  );
  // L5.4. Mission folds and remembers; Conversation reads what Mission already
  // folded for this exact session, or nothing. (B0 now reads the evidence in
  // both lenses for `ctx.openRulings`; the wake index keeps its L5.4 rule.)
  const wakeScope = React.useMemo(
    () => ({
      channelRef: input.channelId,
      sessionRef: input.umbrella.sessionRef,
      genesisRef: input.umbrella.genesisRef,
      founderPubkey: input.umbrella.founderPubkey,
    }),
    [
      input.channelId,
      input.umbrella.founderPubkey,
      input.umbrella.genesisRef,
      input.umbrella.sessionRef,
    ],
  );
  React.useEffect(() => {
    if (!input.active) return;
    rememberCodingSessionWakeOperations(wakeScope, foldedWakeOperations);
  }, [foldedWakeOperations, input.active, wakeScope]);
  const cachedWakeOperations = readCachedCodingSessionWakeOperations(wakeScope);
  const wakeOperations = input.active
    ? foldedWakeOperations
    : cachedWakeOperations;
  // The Audit tab reads the umbrella's own signed transcripts — every
  // generation, not just the live one, because a seat that was restarted spent
  // its earlier turns' tokens all the same.
  //
  // Only the *input* is assembled here. The fold itself lives inside
  // `CodingSessionMissionAudit`, which the surface host mounts only while the
  // Audit tab is selected, so a mission nobody is auditing pays nothing for it
  // (REVIEW-A3 F5). A single-generation seat hands over its own transcript
  // array by reference, so the component's memo does not re-fold when nothing
  // it reads has moved.
  const auditSeats = React.useMemo(() => {
    const labels = new Map(
      input.participants.map((participant) => [
        participant.executionKey,
        participant.label,
      ]),
    );
    return input.umbrella.executions.map((execution) => ({
      executionKey: execution.executionKey,
      seat: labels.get(execution.executionKey) ?? CODING_SESSION_UNKNOWN_ACTOR,
      transcript:
        execution.priorGenerations.length === 0
          ? execution.activeGeneration.transcript
          : [...execution.priorGenerations, execution.activeGeneration].flatMap(
              (record) => record.transcript,
            ),
    }));
  }, [input.participants, input.umbrella.executions]);
  const observationView = React.useMemo(
    () =>
      deriveCodingSessionObservationView({
        fold: observations.result?.fold ?? null,
        resolveLabel: (pubkey) => input.resolveActorName(pubkey),
        knownDecisionRefs: (evidence.inspectorInput.decisions ?? []).map(
          (decision) => decision.requestId,
        ),
      }),
    [
      evidence.inspectorInput.decisions,
      input.resolveActorName,
      observations.result,
    ],
  );
  // The Route rail places a sign at a signed `created_at`. The fold reads no
  // clock and returns none, so the time is joined here from the events this
  // read fetched — transport data, never fold semantics (see the type's doc).
  const observationGates = React.useMemo<
    readonly CodingSessionRouteGateRow[]
  >(() => {
    const signedAt = observations.result?.signedAt ?? null;
    if (signedAt === null) return [];
    return observationView.gates.map((row) => ({
      key: row.key,
      authorPubkey: row.authorPubkey,
      gate: row.gate,
      outcome: row.outcome,
      at: signedAt.get(row.sourceEventId) ?? null,
      sourceEventId: row.sourceEventId.length > 0 ? row.sourceEventId : null,
      commitShortSha: row.commitShortSha,
      dirty: row.dirty,
    }));
  }, [observationView.gates, observations.result]);
  // The Audit's open gate runs say "not live — read at HH:MM" while the
  // 44246 subscription is not up (SV-41): the read is then a snapshot.
  const observationsNotLive = React.useMemo(
    () =>
      codingSessionObservationNotLive(
        observations.live,
        observations.readAtMs,
        formatCodingSessionSurfaceClock,
      ),
    [observations.live, observations.readAtMs],
  );
  const missionContent =
    React.useMemo<CodingSessionSurfaceMissionContent | null>(
      () =>
        input.active
          ? {
              inspector: (
                <CodingSessionMissionInspector
                  deliveries={input.deliveries}
                  errorMessage={evidence.errorMessage}
                  focusedExecutionKey={input.focusedExecutionKey}
                  goalEditor={input.goalEditor}
                  goalReader={input.goalReader}
                  loading={evidence.isLoading}
                  model={model}
                  onFocusParticipant={input.onFocusParticipant}
                  onOpenFileTrace={input.onOpenTrace}
                  onRefresh={() => {
                    evidence.refresh();
                    observations.refresh();
                  }}
                  observationsLoading={observations.isLoading}
                  observationsError={observations.errorMessage}
                  gateRows={observationView.gates}
                  seatAuthorities={input.seatAuthorities}
                  unseatedReportEventIds={pending.unseatedReportEventIds}
                  settlements={evidence.inspectorInput.settlements}
                  workCoverage={workCoverage.data ?? null}
                  workCoverageScope={
                    workScope === null
                      ? undefined
                      : {
                          channelRef: workScope.channelRef,
                          sessionRef: workScope.sessionRef,
                        }
                  }
                  workCoverageLoading={
                    workCoverage.isPending && workScope !== null
                  }
                  workCoverageError={
                    workCoverage.error
                      ? workCoverage.error instanceof Error
                        ? workCoverage.error.message
                        : String(workCoverage.error)
                      : null
                  }
                  pendingCompletion={
                    evidence.inspectorInput.pendingCompletion ?? null
                  }
                  variant={input.isNarrow ? "drawer" : "panel"}
                />
              ),
              context: (
                <CodingSessionMissionContext
                  errorMessage={evidence.errorMessage}
                  founderPubkey={input.umbrella.founderPubkey}
                  loading={evidence.isLoading || policy.isLoading}
                  model={model}
                  onRefresh={() => {
                    evidence.refresh();
                    policy.refresh();
                  }}
                  policyErrorMessage={policy.errorMessage}
                  policyFold={policy.fold}
                  resolveActorLabel={input.resolveActorName}
                  variant={input.isNarrow ? "drawer" : "panel"}
                />
              ),
              audit: (
                <CodingSessionMissionAudit
                  errorMessage={evidence.errorMessage}
                  loading={evidence.isLoading}
                  observations={observationView}
                  observationsError={observations.errorMessage}
                  observationsLoading={observations.isLoading}
                  observationsNotLive={observationsNotLive}
                  onRefresh={() => {
                    evidence.refresh();
                    observations.refresh();
                  }}
                  seats={auditSeats}
                  variant={input.isNarrow ? "drawer" : "panel"}
                />
              ),
            }
          : null,
      [
        auditSeats,
        evidence.errorMessage,
        evidence.inspectorInput.pendingCompletion,
        evidence.inspectorInput.settlements,
        evidence.isLoading,
        workCoverage.data,
        workCoverage.error,
        workCoverage.isPending,
        workScope,
        evidence.refresh,
        input.active,
        input.deliveries,
        input.focusedExecutionKey,
        input.goalEditor,
        input.goalReader,
        input.isNarrow,
        input.onFocusParticipant,
        input.onOpenTrace,
        input.resolveActorName,
        input.seatAuthorities,
        input.umbrella.founderPubkey,
        model,
        observationView,
        observations.errorMessage,
        observations.isLoading,
        observations.refresh,
        observationsNotLive,
        pending.unseatedReportEventIds,
        policy.errorMessage,
        policy.fold,
        policy.isLoading,
        policy.refresh,
      ],
    );
  // `ctx`'s view: the Audit's own, except that until the assignments have
  // been read a pointer is "not checked", never "resolves to nothing".
  const ctxObservationView = React.useMemo(
    () =>
      teamEvidenceRead
        ? observationView
        : deriveCodingSessionObservationView({
            fold: codingSessionObservationFoldUnchecked(
              observations.result?.fold ?? null,
            ),
            resolveLabel: (pubkey) => input.resolveActorName(pubkey),
            knownDecisionRefs: [],
          }),
    [
      input.resolveActorName,
      observationView,
      observations.result,
      teamEvidenceRead,
    ],
  );
  const observationRead = React.useMemo<CodingSessionSurfaceObservations>(
    () =>
      observationScope === null
        ? {
            state: "not-read",
            reason: "This session has no genesis, so it has no observations.",
          }
        : {
            state: "read",
            isLoading: observations.isLoading,
            errorMessage: observations.errorMessage,
            result: observations.result,
            view: ctxObservationView,
            assignmentsChecked: teamEvidenceRead,
            readAtMs: observations.readAtMs,
            live: observations.live,
            refresh: observations.refresh,
          },
    [
      ctxObservationView,
      observationScope,
      teamEvidenceRead,
      observations.errorMessage,
      observations.isLoading,
      observations.live,
      observations.readAtMs,
      observations.refresh,
      observations.result,
    ],
  );
  const requestInputs = evidence.inspectorInput.decisionRequests;
  const decisionRequests = React.useMemo(
    () => codingSessionSurfaceDecisionRequestRows(requestInputs),
    [requestInputs],
  );
  return React.useMemo(
    () => ({
      missionContent,
      observationRead,
      teamEvidenceRead,
      decisions: evidence.inspectorInput.decisions ?? null,
      decisionRequests,
      missionState: model.missionState,
      transactions: pending.transactions,
      unseatedReportEventIds: pending.unseatedReportEventIds,
      wakeOperations,
      observationGates,
    }),
    [
      evidence.inspectorInput.decisions,
      decisionRequests,
      missionContent,
      model.missionState,
      observationGates,
      observationRead,
      teamEvidenceRead,
      pending.transactions,
      pending.unseatedReportEventIds,
      wakeOperations,
    ],
  );
}

/** Every Task/Agent spawn any execution made, across its generations. */
export function useCodingSessionUmbrellaSubagents(
  umbrella: CodingSessionUmbrellaRecord,
): ReturnType<typeof deriveCodingSessionSubagentPanel> {
  return React.useMemo(() => {
    const generations = umbrella.executions.flatMap((execution) =>
      [...execution.priorGenerations, execution.activeGeneration].map(
        (record) => ({ execution, record }),
      ),
    );
    // Each spawn reads through the settlement Mission's block rules give its
    // turn, so the panel never spins over a call the stream calls stopped.
    const settlementOf = generations.map(({ execution, record }) =>
      resolveCodingSessionGenerationCallSettlement({
        activeGeneration: execution.activeGeneration,
        generationId: record.generationId,
        transcript: record.transcript,
      }),
    );
    return deriveCodingSessionSubagentPanel(
      generations.map(({ record }) => record.transcript),
      (call, index) => settlementOf[index]?.(call) ?? "unknown",
    );
  }, [umbrella.executions]);
}

/**
 * Open Mission's three surfaces (Inspector active) once per mount in Mission,
 * and Agents once for a wide multi-execution Conversation (§3's two kept
 * behaviours), and close Mission's surfaces whenever the lens leaves it.
 *
 * Both opens are proactive: once the person has made a panel choice for this
 * session (persisted with the panels), neither overrides it on a reload.
 */
export function useCodingSessionMissionSurfaceActivation(input: {
  bodyWidthPx: number;
  closeMissionSurfaces: () => void;
  isMultiExecution: boolean;
  mission: boolean;
  openProactive: (ids: readonly string[], activate: string) => void;
  openMissionSurfaces: () => void;
}): void {
  const openedRef = React.useRef(false);
  const autoOpenedAgentsRef = React.useRef(false);
  React.useEffect(() => {
    if (
      !input.mission &&
      !autoOpenedAgentsRef.current &&
      shouldAutoOpenAgentsSurface({
        bodyWidthPx: input.bodyWidthPx,
        isMultiExecution: input.isMultiExecution,
      })
    ) {
      autoOpenedAgentsRef.current = true;
      input.openProactive(["agents"], "agents");
    }
  }, [
    input.bodyWidthPx,
    input.isMultiExecution,
    input.mission,
    input.openProactive,
  ]);
  React.useEffect(() => {
    if (!input.mission) {
      // However the lens left Mission, its surfaces go with it.
      if (openedRef.current) input.closeMissionSurfaces();
      openedRef.current = false;
      return;
    }
    if (!openedRef.current) {
      openedRef.current = true;
      input.openMissionSurfaces();
    }
  }, [input.closeMissionSurfaces, input.mission, input.openMissionSurfaces]);
}
