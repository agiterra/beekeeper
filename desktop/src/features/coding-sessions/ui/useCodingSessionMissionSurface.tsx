import * as React from "react";

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
import type { CodingSessionRouteGateRow } from "@/features/coding-sessions/lib/codingSessionRouteModel";
import { useCodingSessionObservations } from "@/features/coding-sessions/hooks/useCodingSessionObservations";
import type { CodingSessionReachabilityResolver } from "@/features/coding-sessions/hooks/useCodingSessionProviderReachability";
import { useIdentityQuery } from "@/shared/api/hooks";
import { truncatePubkey } from "@/shared/lib/pubkey";
import { useCodingSessionMissionLand } from "@/features/coding-sessions/hooks/useCodingSessionMissionLand";
import { useCodingSessionSessionPolicy } from "@/features/coding-sessions/hooks/useCodingSessionSessionPolicy";
import { CodingSessionChangesRail } from "./CodingSessionChangesRail";
import { CodingSessionExecutionRail } from "./CodingSessionExecutionRail";
import { shouldAutoOpenAgentsSurface } from "./CodingSessionUmbrellaWorkspaceModel";
import { CodingSessionMissionAudit } from "./CodingSessionMissionAudit";
import { CodingSessionMissionContext } from "./CodingSessionMissionContext";
import { CodingSessionMissionInspector } from "./CodingSessionMissionInspector";
import type { CodingSessionSurfaceDescriptor } from "./CodingSessionSurfaceHost";

export type CodingSessionMissionSurfaceResult = {
  surfaces: CodingSessionSurfaceDescriptor[];
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
  // L8.3: the real `gates.verifierRequired`, and — separately — whether a
  // 44245 reached this view at all. Unknown is not false: with no record the
  // fold reads `false` exactly as it did before, and the state panel says so.
  const policyRecordKnown = policy.fold?.selected != null;
  const verifierRequired =
    policy.fold?.selected?.record.gates?.verifierRequired ?? null;
  const evidence = useCodingSessionMissionEvidence(
    scope,
    undefined,
    verifierRequired,
  );
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
  const observationScope = React.useMemo(
    () =>
      scope === null
        ? null
        : {
            channelRef: scope.channelRef,
            sessionRef: scope.sessionRef,
            genesisRef: scope.genesisRef,
          },
    [scope],
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
  // L8.2: the push path's own rule, asked once per fold, over the repository's
  // own kind:30617 when one is named and readable.
  const land = useCodingSessionMissionLand({
    founderPubkey: input.umbrella.founderPubkey,
    genesisRef: input.umbrella.genesisRef,
    landEvidence: evidence.inspectorInput.landEvidence,
    repoRef,
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
      leadHasOpenTurn,
      policyRecordKnown,
      resolveActorLabel: input.resolveActorName,
    }),
    [
      evidence.inspectorInput,
      land,
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
  // L5.4. Mission folds and remembers; Conversation — which subscribes to no
  // fold and must not start — reads what Mission already folded for this exact
  // session, or nothing. No invoke, no fetch, no subscription either way.
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
    }));
  }, [observationView.gates, observations.result]);
  const surfaces = React.useMemo(
    () =>
      input.active
        ? ([
            {
              id: "mission-inspector",
              label: "Inspector",
              content: (
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
                  onRefresh={evidence.refresh}
                  gateRows={observationView.gates}
                  seatAuthorities={input.seatAuthorities}
                  unseatedReportEventIds={pending.unseatedReportEventIds}
                  variant={input.isNarrow ? "drawer" : "panel"}
                />
              ),
            },
            {
              id: "mission-context",
              label: "Context",
              content: (
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
            },
            {
              id: "mission-audit",
              label: "Audit",
              content: (
                <CodingSessionMissionAudit
                  errorMessage={evidence.errorMessage}
                  loading={evidence.isLoading}
                  observations={observationView}
                  observationsError={observations.errorMessage}
                  observationsLoading={observations.isLoading}
                  onRefresh={() => {
                    evidence.refresh();
                    observations.refresh();
                  }}
                  seats={auditSeats}
                  variant={input.isNarrow ? "drawer" : "panel"}
                />
              ),
            },
          ] satisfies CodingSessionSurfaceDescriptor[])
        : [],
    [
      auditSeats,
      evidence.errorMessage,
      evidence.isLoading,
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
      pending.unseatedReportEventIds,
      policy.errorMessage,
      policy.fold,
      policy.isLoading,
      policy.refresh,
    ],
  );
  return React.useMemo(
    () => ({
      surfaces,
      missionState: model.missionState,
      transactions: pending.transactions,
      unseatedReportEventIds: pending.unseatedReportEventIds,
      wakeOperations,
      observationGates,
    }),
    [
      model.missionState,
      observationGates,
      pending.transactions,
      pending.unseatedReportEventIds,
      surfaces,
      wakeOperations,
    ],
  );
}

/** Build the lens-specific host registry; surfaces never leak across lenses. */
export function useCodingSessionWorkspaceSurfaces(input: {
  actorNames: CodingSessionActorNameResolver;
  mission: boolean;
  missionSurfaces: readonly CodingSessionSurfaceDescriptor[];
  observedChanges: CodingSessionObservedChanges;
  resolveReachability: CodingSessionReachabilityResolver;
  umbrella: CodingSessionUmbrellaRecord;
}): CodingSessionSurfaceDescriptor[] {
  return React.useMemo(
    () =>
      input.mission
        ? [...input.missionSurfaces]
        : [
            {
              id: "agents",
              label: "Agents",
              count: input.umbrella.executions.length,
              content: (
                <CodingSessionExecutionRail
                  actorNames={input.actorNames}
                  resolveReachability={input.resolveReachability}
                  umbrella={input.umbrella}
                />
              ),
            },
            {
              id: "changes",
              label: "Observed changes",
              count: input.observedChanges.files.length,
              content: (
                <CodingSessionChangesRail
                  files={input.observedChanges.files}
                  unreportedEditCount={
                    input.observedChanges.unreportedEditCount
                  }
                />
              ),
            },
          ],
    [
      input.actorNames,
      input.mission,
      input.missionSurfaces,
      input.observedChanges.files,
      input.observedChanges.unreportedEditCount,
      input.resolveReachability,
      input.umbrella,
    ],
  );
}

/** Open Inspector once per explicit Mission entry and close it on recovery. */
export function useCodingSessionMissionSurfaceActivation(input: {
  activeTab: string | null;
  bodyWidthPx: number;
  close: () => void;
  isMultiExecution: boolean;
  mission: boolean;
  select: (id: string) => void;
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
      input.select("agents");
    }
  }, [input.bodyWidthPx, input.isMultiExecution, input.mission, input.select]);
  React.useEffect(() => {
    if (!input.mission) {
      openedRef.current = false;
      if (
        input.activeTab === "mission-inspector" ||
        input.activeTab === "mission-context" ||
        input.activeTab === "mission-audit"
      ) {
        input.close();
      }
      return;
    }
    if (!openedRef.current) {
      openedRef.current = true;
      input.select("mission-inspector");
    }
  }, [input.activeTab, input.close, input.mission, input.select]);
}
