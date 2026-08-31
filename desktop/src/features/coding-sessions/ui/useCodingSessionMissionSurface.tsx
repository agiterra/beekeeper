import * as React from "react";

import type { CodingSessionContextLoad } from "@/features/coding-sessions/lib/codingSessionContextLoad";
import type { CodingSessionGoal } from "@/features/coding-sessions/lib/codingSessionGoal";
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
};

/** Build Mission's canonical inspector surface without subscribing in Conversation. */
export function useCodingSessionMissionSurface(input: {
  active: boolean;
  channelId: string;
  contextLoads: readonly {
    key: string;
    load: CodingSessionContextLoad | null;
  }[];
  focusedExecutionKey: string | null;
  goal: CodingSessionGoal | null;
  isNarrow: boolean;
  observedChanges: CodingSessionObservedChanges;
  onFocusParticipant: (executionKey: string | null) => void;
  onOpenTrace: () => void;
  participants: readonly CodingSessionParticipantPresence[];
  resolveActorName: CodingSessionActorNameResolver;
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
  const surfaces = React.useMemo(
    () =>
      input.active
        ? ([
            {
              id: "mission-inspector",
              label: "Inspector",
              content: (
                <CodingSessionMissionInspector
                  errorMessage={evidence.errorMessage}
                  focusedExecutionKey={input.focusedExecutionKey}
                  loading={evidence.isLoading}
                  model={model}
                  onFocusParticipant={input.onFocusParticipant}
                  onOpenFileTrace={input.onOpenTrace}
                  onRefresh={evidence.refresh}
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
      input.focusedExecutionKey,
      input.isNarrow,
      input.onFocusParticipant,
      input.onOpenTrace,
      model,
    ],
  );
  return React.useMemo(
    () => ({ surfaces, missionState: model.missionState }),
    [model.missionState, surfaces],
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
