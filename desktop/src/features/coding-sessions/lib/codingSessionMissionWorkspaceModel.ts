import type { CodingSessionGoal } from "./codingSessionGoal";
import type {
  CodingSessionMissionInspectorInput,
  CodingSessionSeatPlanInput,
} from "./codingSessionMissionInspectorModel";
import type { CodingSessionParticipantPresence } from "./codingSessionStreamPresence";
import type { CodingSessionContextLoad } from "./codingSessionContextLoad";
import type { CodingSessionObservedChanges } from "./codingSessionTranscriptModel";

/**
 * Merge canonical team-transaction evidence with trusted projections already
 * owned by the live workspace. Missing inputs stay missing: this seam never
 * infers a goal, source event, participant, context load, or observed edit.
 */
export function mergeCodingSessionMissionWorkspaceInput(input: {
  evidence: CodingSessionMissionInspectorInput;
  goal: CodingSessionGoal | null;
  goalAuthorLabel: string;
  observedChanges: CodingSessionObservedChanges;
  participants: readonly CodingSessionParticipantPresence[];
  contextLoads: ReadonlyMap<string, CodingSessionContextLoad | null>;
  seatPlans: readonly CodingSessionSeatPlanInput[];
}): CodingSessionMissionInspectorInput {
  return {
    ...input.evidence,
    goal: input.goal
      ? {
          kind: "available",
          sourceEventId: input.goal.eventId,
          authorLabel: input.goalAuthorLabel,
          text: input.goal.content,
        }
      : input.evidence.goal,
    seatPlans: input.seatPlans.map((plan) => ({ ...plan })),
    observedChanges: {
      files: input.observedChanges.files.map((file) => ({
        ...file,
        diffs: file.diffs.map((diff) => ({ ...diff })),
      })),
      unreportedEditCount: input.observedChanges.unreportedEditCount,
    },
    participants: input.participants.map((participant) => ({
      ...participant,
      status: { ...participant.status },
    })),
    contextLoads: new Map(
      [...input.contextLoads].map(([key, load]) => [
        key,
        load ? { ...load } : null,
      ]),
    ),
  };
}
