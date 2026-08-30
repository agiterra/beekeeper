import { formatCoordinationAge } from "@/shared/coordination/sessionCoordinationFormat";

import { formatCodingSessionExecutionLabel } from "./codingSessionLabels";
import {
  deriveCodingSessionTaskModel,
  type CodingSessionTaskModel,
} from "./codingSessionTaskModel";
import type {
  CodingSessionExecution,
  CodingSessionUmbrellaRecord,
  CodingSessionWorkspaceStatus,
} from "./codingSessionTypes";
import {
  codingSessionDispositionWord,
  type CodingSessionActorNameResolver,
} from "./codingSessionUmbrellaModel";

const MAX_ACTIVITY_CHARACTERS = 48;

export type CodingSessionStreamStatusResolver = (
  execution: CodingSessionExecution,
) => CodingSessionWorkspaceStatus;

/**
 * Resolve the trusted provider-signed `turn_started` receipt time for one
 * execution and provider turn. The timestamp is Unix epoch milliseconds.
 */
export type CodingSessionTurnStartedAtResolver = (
  execution: CodingSessionExecution,
  turnId: string,
) => number | null;

export type CodingSessionParticipantPresence = {
  executionKey: string;
  /** Agent · Role when the profile is known; never a pubkey. */
  label: string;
  /** Runtime · model, retained for diagnostics and hover copy. */
  secondaryLabel: string | null;
  role: string | null;
  status: CodingSessionWorkspaceStatus;
  disposition: string;
  /** The seat's signed in-progress plan step, bounded for the roster. */
  activity: string | null;
  lastTurnLabel: string;
};

export type CodingSessionLiveActivity = {
  executionKey: string;
  label: string;
  activity: string | null;
  /** Signed receipt time when available, otherwise transcript fallback. */
  startedAtMs: number | null;
  /** Count of projected signed tool items in the still-open turn. */
  openToolCount: number | null;
  turnKey: string;
};

export type CodingSessionStreamPresence = {
  participants: CodingSessionParticipantPresence[];
  liveActivity: CodingSessionLiveActivity[];
};

/**
 * Project the participant and live-activity bars from the workspace's trusted
 * signed records. This model owns no relay reads and guesses no provider
 * state: its caller supplies W1 after reachability demotion. Task text and
 * tool counts come from projected 44225 items; turn timing prefers the
 * caller's trusted provider-signed 44224 `turn_started` receipt.
 */
export function deriveCodingSessionStreamPresence(input: {
  umbrella: CodingSessionUmbrellaRecord;
  resolveStatus: CodingSessionStreamStatusResolver;
  resolveActorName?: CodingSessionActorNameResolver;
  resolveTurnStartedAt?: CodingSessionTurnStartedAtResolver;
  canSteer?: boolean;
  nowMs?: number;
}): CodingSessionStreamPresence {
  const nowMs = input.nowMs ?? Date.now();
  const canSteer = input.canSteer ?? false;
  const participants = input.umbrella.executions.map((execution) => {
    const record = execution.activeGeneration;
    const actorName = record.agentRef
      ? (input.resolveActorName?.(record.agentRef) ?? null)
      : null;
    const identity = formatCodingSessionExecutionLabel({
      agentRef: record.agentRef,
      role: record.role,
      agentDisplayName: actorName,
      runtime: record.runtime,
      model: record.model,
    });
    const label =
      record.agentRef && record.role && !actorName && identity.secondary
        ? `${identity.primary} · ${identity.secondary}`
        : identity.primary;
    const status = input.resolveStatus(execution);
    const openTurnId =
      status.kind === "working" ? readOpenTurnId(execution) : null;
    const taskModel =
      openTurnId === null
        ? null
        : activeTaskModelForTurn(record.transcript, openTurnId);
    return {
      execution,
      participant: {
        executionKey: execution.executionKey,
        label,
        secondaryLabel: identity.secondary,
        role: record.role,
        status,
        disposition: codingSessionDispositionWord(status, canSteer),
        activity: activeTaskText(taskModel),
        lastTurnLabel: formatLastTurnLabel(execution, nowMs),
      } satisfies CodingSessionParticipantPresence,
      openTurnId,
    };
  });

  const ordered = [
    ...participants.filter(({ participant }) => isLead(participant.role)),
    ...participants.filter(({ participant }) => !isLead(participant.role)),
  ];

  return {
    participants: ordered.map(({ participant }) => participant),
    liveActivity: ordered.flatMap(({ execution, participant, openTurnId }) => {
      if (participant.status.kind !== "working") return [];
      const turn = openTurnId
        ? readOpenTurnFacts(execution, openTurnId, input.resolveTurnStartedAt)
        : null;
      return [
        {
          executionKey: execution.executionKey,
          label: participant.label,
          activity: participant.activity,
          startedAtMs: turn?.startedAtMs ?? null,
          openToolCount: turn?.openToolCount ?? null,
          turnKey: `${execution.executionKey}:${openTurnId ?? execution.activeGeneration.statusAt ?? execution.activeGeneration.lastEventAt}`,
        },
      ];
    }),
  };
}

function activeTaskModelForTurn(
  transcript: CodingSessionExecution["activeGeneration"]["transcript"],
  openTurnId: string,
): CodingSessionTaskModel | null {
  const model = deriveCodingSessionTaskModel(transcript);
  return model?.state === "active" && model.turnId === openTurnId
    ? model
    : null;
}

function activeTaskText(model: CodingSessionTaskModel | null): string | null {
  const text = model?.tasks.find((task) => task.status === "in_progress")?.text;
  if (!text) return null;
  return truncateActivity(text);
}

function truncateActivity(value: string): string {
  const characters = Array.from(value.trim());
  if (characters.length <= MAX_ACTIVITY_CHARACTERS) return characters.join("");
  return `${characters.slice(0, MAX_ACTIVITY_CHARACTERS - 1).join("")}…`;
}

function readOpenTurnId(execution: CodingSessionExecution): string | null {
  const transcript = execution.activeGeneration.transcript;
  for (let index = transcript.length - 1; index >= 0; index -= 1) {
    const item = transcript[index];
    const turnId = item?.turnId?.trim();
    if (!turnId) continue;
    if (
      item.type === "lifecycle" &&
      (item.title === "Turn result" || item.title === "Interrupted")
    ) {
      return null;
    }
    return turnId;
  }
  return null;
}

function readOpenTurnFacts(
  execution: CodingSessionExecution,
  turnId: string,
  resolveTurnStartedAt: CodingSessionTurnStartedAtResolver | undefined,
): { startedAtMs: number | null; openToolCount: number | null } {
  const receiptStartedAtMs = resolveTurnStartedAt?.(execution, turnId) ?? null;
  let transcriptStartedAtMs: number | null = null;
  let openToolCount = 0;
  for (const item of execution.activeGeneration.transcript) {
    if (item.turnId !== turnId) continue;
    const timestamp = Date.parse(item.timestamp);
    if (
      Number.isFinite(timestamp) &&
      (transcriptStartedAtMs === null || timestamp < transcriptStartedAtMs)
    ) {
      transcriptStartedAtMs = timestamp;
    }
    if (item.type === "tool") openToolCount += 1;
  }
  return {
    startedAtMs: Number.isFinite(receiptStartedAtMs)
      ? receiptStartedAtMs
      : transcriptStartedAtMs,
    // Absence is not zero. A zero count has no clause in the bar.
    openToolCount: openToolCount > 0 ? openToolCount : null,
  };
}

function formatLastTurnLabel(
  execution: CodingSessionExecution,
  nowMs: number,
): string {
  let newestAt: number | null = null;
  for (const record of [
    ...execution.priorGenerations,
    execution.activeGeneration,
  ]) {
    for (const item of record.transcript) {
      const timestamp = Date.parse(item.timestamp);
      if (!Number.isFinite(timestamp)) continue;
      if (newestAt === null || timestamp > newestAt) newestAt = timestamp;
    }
  }
  if (newestAt === null) return "no turn observed";
  const age = formatCoordinationAge(
    Math.max(0, Math.floor((nowMs - newestAt) / 1_000)),
  );
  return age === "just now" ? "last turn just now" : `last turn ${age} ago`;
}

function isLead(role: string | null): boolean {
  return role?.trim().toLowerCase() === "lead";
}
