import * as React from "react";

import type { CodingSessionContextLoad } from "@/features/coding-sessions/lib/codingSessionContextLoad";
import type { CodingSessionGoal } from "@/features/coding-sessions/lib/codingSessionGoal";
import type {
  CodingSessionMissionTransactionInput,
  CodingSessionSeatAuthority,
  CodingSessionTeamWakeDelivery,
} from "@/features/coding-sessions/lib/codingSessionMissionContracts";
import { deriveCodingSessionMissionInspectorModel } from "@/features/coding-sessions/lib/codingSessionMissionInspectorModel";
import { mergeCodingSessionMissionWorkspaceInput } from "@/features/coding-sessions/lib/codingSessionMissionWorkspaceModel";
import { projectCodingSessionMissionState } from "@/features/coding-sessions/lib/codingSessionMissionStateProjection";
import type { CodingSessionParticipantPresence } from "@/features/coding-sessions/lib/codingSessionStreamPresence";
import { deriveCodingSessionTaskModel } from "@/features/coding-sessions/lib/codingSessionTaskModel";
import type { CodingSessionObservedChanges } from "@/features/coding-sessions/lib/codingSessionTranscriptModel";
import type { CodingSessionUmbrellaRecord } from "@/features/coding-sessions/lib/codingSessionTypes";
import type { CodingSessionActorNameResolver } from "@/features/coding-sessions/lib/codingSessionUmbrellaModel";
import { CODING_SESSION_UNKNOWN_ACTOR } from "@/features/coding-sessions/lib/codingSessionTurnByline";
import { useCodingSessionMissionEvidence } from "@/features/coding-sessions/lib/useCodingSessionMissionEvidence";
import type { CodingSessionReachabilityResolver } from "@/features/coding-sessions/hooks/useCodingSessionProviderReachability";
import { CodingSessionChangesRail } from "./CodingSessionChangesRail";
import { CodingSessionExecutionRail } from "./CodingSessionExecutionRail";
import { shouldAutoOpenAgentsSurface } from "./CodingSessionUmbrellaWorkspaceModel";
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
};

/**
 * Lane D adds `transactions` and `unseatedReportEventIds` to the Mission
 * evidence projection in the same batch as this consolidation; in this
 * worktree the fields do not exist on `CodingSessionMissionInspectorInput`
 * yet. This is the **one** place that reads them through an index-access cast,
 * so integration is a single deletion: the finalizer removes this helper and
 * reads the typed fields directly.
 */
// U-F8: shared frozen empties. Returning fresh `[]` literals made
// `pending.transactions` change identity on every `inspectorInput` change,
// which propagates through the hook's result into every consumer's memo deps —
// exactly the render-stability trap AGENTS.md § "React render perf" names.
const NO_TRANSACTIONS: readonly CodingSessionMissionTransactionInput[] =
  Object.freeze([]);
const NO_UNSEATED_REPORT_IDS: readonly string[] = Object.freeze([]);

export function readPendingLaneDEvidence(inspectorInput: unknown): {
  transactions: readonly CodingSessionMissionTransactionInput[];
  unseatedReportEventIds: readonly string[];
} {
  const pending = inspectorInput as {
    transactions?: readonly CodingSessionMissionTransactionInput[];
    unseatedReportEventIds?: readonly string[];
  };
  return {
    transactions: pending.transactions ?? NO_TRANSACTIONS,
    unseatedReportEventIds:
      pending.unseatedReportEventIds ?? NO_UNSEATED_REPORT_IDS,
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
  const evidence = useCodingSessionMissionEvidence(scope);
  const trustedGoal =
    input.goal !== null &&
    input.goal.channelId === input.channelId &&
    input.goal.sessionRef === input.umbrella.sessionRef &&
    input.goal.founderPubkey === input.umbrella.founderPubkey
      ? input.goal
      : null;
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
  const inspectorInput = React.useMemo(
    () =>
      mergeCodingSessionMissionWorkspaceInput({
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
    [
      evidence.inspectorInput,
      contextLoads,
      input.observedChanges,
      input.participants,
      input.resolveActorName,
      seatPlans,
      missionState,
      trustedGoal,
    ],
  );
  const model = React.useMemo(
    () => deriveCodingSessionMissionInspectorModel(inspectorInput),
    [inspectorInput],
  );
  const pending = React.useMemo(
    () => readPendingLaneDEvidence(evidence.inspectorInput),
    [evidence.inspectorInput],
  );
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
                  loading={evidence.isLoading}
                  model={model}
                  onFocusParticipant={input.onFocusParticipant}
                  onOpenFileTrace={input.onOpenTrace}
                  onRefresh={evidence.refresh}
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
                  loading={evidence.isLoading}
                  model={model}
                  onRefresh={evidence.refresh}
                  variant={input.isNarrow ? "drawer" : "panel"}
                />
              ),
            },
          ] satisfies CodingSessionSurfaceDescriptor[])
        : [],
    [
      evidence.errorMessage,
      evidence.isLoading,
      evidence.refresh,
      input.active,
      input.deliveries,
      input.focusedExecutionKey,
      input.goalEditor,
      input.isNarrow,
      input.onFocusParticipant,
      input.onOpenTrace,
      input.seatAuthorities,
      model,
      pending.unseatedReportEventIds,
    ],
  );
  return React.useMemo(
    () => ({
      surfaces,
      missionState: model.missionState,
      transactions: pending.transactions,
      unseatedReportEventIds: pending.unseatedReportEventIds,
    }),
    [
      model.missionState,
      pending.transactions,
      pending.unseatedReportEventIds,
      surfaces,
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
        input.activeTab === "mission-context"
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
