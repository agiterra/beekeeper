import {
  buildCodingSessionTargetKey,
  type CodingSessionCommandTarget,
} from "./codingSessionCommand";
import type { CodingSessionMissionInspectorInput } from "./codingSessionMissionInspectorModel";
import type {
  CodingSessionExecution,
  CodingSessionUmbrellaRecord,
} from "./codingSessionTypes";

const HEX64 = /^[0-9a-f]{64}$/;
const STATE_SCHEMA = "buzz-coding-session-team-wake-state/v1";
const STORAGE_PREFIX = "buzz:coding-session-team-wake:v1:";
const MAX_PENDING_WAKES = 128;
const MAX_COMMAND_ID_BYTES = 256;
const MAX_ROLE_BYTES = 64;

export type CodingSessionTeamWakeCandidate = {
  sourceEventId: string;
  sourceCreatedAtMs: number;
  sourceEventSeq: number | null;
  sourceTargetKey: string | null;
  kind: "operation_ready" | "turn_ended_without_required_operation";
  operationType: "report" | null;
  seatRole: string;
  causedByCommandId: string | null;
  preferredCommandId: string | null;
};

export type CodingSessionTeamWakeAttempt = {
  sourceEventId: string;
  commandId: string;
  leadTargetKey: string;
  candidate: CodingSessionTeamWakeCandidate;
};

export type CodingSessionTeamWakeState = {
  schema: typeof STATE_SCHEMA;
  cursor: { createdAtMs: number; sourceEventId: string } | null;
  pending: CodingSessionTeamWakeAttempt[];
};

export type CodingSessionTeamWakePlan = {
  lead: CodingSessionExecution | null;
  candidates: CodingSessionTeamWakeCandidate[];
  refusal: string | null;
};

function normalizedRole(role: string | null): string | null {
  const value = role?.trim().toLowerCase() ?? "";
  return value.length > 0 ? value : null;
}

function exactPubkey(value: string | null | undefined): string | null {
  const normalized = value?.toLowerCase() ?? "";
  return HEX64.test(normalized) ? normalized : null;
}

function exactEventId(value: unknown): value is string {
  return typeof value === "string" && HEX64.test(value);
}

function safeCreatedAtMs(value: unknown): number | null {
  if (typeof value !== "number" || !Number.isSafeInteger(value) || value < 0) {
    return null;
  }
  const millis = value * 1000;
  return Number.isSafeInteger(millis) ? millis : null;
}

function boundedText(value: unknown, maxBytes: number): value is string {
  return (
    typeof value === "string" &&
    value.length > 0 &&
    ![...value].some((character) => {
      const codePoint = character.codePointAt(0) ?? 0;
      return codePoint < 32 || codePoint === 127;
    }) &&
    new TextEncoder().encode(value).length <= maxBytes
  );
}

function optionalBoundedText(value: unknown, maxBytes: number): boolean {
  return value === null || boundedText(value, maxBytes);
}

function compareCandidate(
  left: CodingSessionTeamWakeCandidate,
  right: CodingSessionTeamWakeCandidate,
): number {
  return (
    left.sourceCreatedAtMs - right.sourceCreatedAtMs ||
    left.sourceEventId.localeCompare(right.sourceEventId)
  );
}

function isAfterCursor(
  candidate: CodingSessionTeamWakeCandidate,
  cursor: CodingSessionTeamWakeState["cursor"],
): boolean {
  return (
    cursor === null ||
    candidate.sourceCreatedAtMs > cursor.createdAtMs ||
    (candidate.sourceCreatedAtMs === cursor.createdAtMs &&
      candidate.sourceEventId > cursor.sourceEventId)
  );
}

function matchingPrompt(
  execution: CodingSessionExecution,
  terminal: CodingSessionExecution["activeGeneration"]["transcript"][number],
) {
  return [...execution.activeGeneration.transcript]
    .reverse()
    .find(
      (item) =>
        item.type === "message" &&
        item.role === "user" &&
        item.turnId === terminal.turnId &&
        typeof item.commandId === "string",
    );
}

function operationCandidates(
  umbrella: CodingSessionUmbrellaRecord,
  evidence: CodingSessionMissionInspectorInput,
): CodingSessionTeamWakeCandidate[] {
  const assignmentRoles = new Map(
    (evidence.assignments ?? []).map((assignment) => [
      assignment.sourceEventId,
      normalizedRole(assignment.assigneeRole),
    ]),
  );
  const seatActors = new Map(
    umbrella.executions.flatMap((execution) => {
      const actor = exactPubkey(execution.activeGeneration.agentRef);
      const role = normalizedRole(execution.activeGeneration.role);
      return actor && role && role !== "lead" ? [[actor, role] as const] : [];
    }),
  );
  return evidence.reports.flatMap((report) => {
    const eventId = exactEventId(report.sourceEventId)
      ? report.sourceEventId
      : null;
    const actor = exactPubkey(report.authorPubkey);
    const createdAtMs = safeCreatedAtMs(report.sourceCreatedAt);
    const assignedRole = assignmentRoles.get(report.assignmentRef) ?? null;
    const seatRole = actor ? (seatActors.get(actor) ?? null) : null;
    if (!eventId || !actor || createdAtMs === null || !seatRole) return [];
    if (assignedRole !== null && assignedRole !== seatRole) return [];
    return [
      {
        sourceEventId: eventId,
        sourceCreatedAtMs: createdAtMs,
        sourceEventSeq: null,
        sourceTargetKey: null,
        kind: "operation_ready" as const,
        operationType: "report" as const,
        seatRole,
        causedByCommandId: null,
        preferredCommandId:
          typeof report.deliveryCommandId === "string" &&
          report.deliveryCommandId.length > 0
            ? report.deliveryCommandId
            : null,
      },
    ];
  });
}

function hasReportForTurn(input: {
  operations: readonly CodingSessionTeamWakeCandidate[];
  evidence: CodingSessionMissionInspectorInput;
  execution: CodingSessionExecution;
  promptAtMs: number | null;
  terminalAtMs: number;
}): boolean {
  const actor = exactPubkey(input.execution.activeGeneration.agentRef);
  if (!actor) return false;
  const operationIds = new Set(
    input.operations.map((operation) => operation.sourceEventId),
  );
  return input.evidence.reports.some((report) => {
    const createdAtMs = safeCreatedAtMs(report.sourceCreatedAt);
    return (
      operationIds.has(report.sourceEventId) &&
      exactPubkey(report.authorPubkey) === actor &&
      createdAtMs !== null &&
      createdAtMs <= input.terminalAtMs &&
      (input.promptAtMs === null || createdAtMs >= input.promptAtMs)
    );
  });
}

function terminalCandidates(
  umbrella: CodingSessionUmbrellaRecord,
  evidence: CodingSessionMissionInspectorInput,
  operations: readonly CodingSessionTeamWakeCandidate[],
): CodingSessionTeamWakeCandidate[] {
  return umbrella.executions.flatMap((execution) => {
    const role = normalizedRole(execution.activeGeneration.role);
    if (!role || role === "lead") return [];
    const expectedTargetKey = execution.activeGeneration.commandTarget
      ? buildCodingSessionTargetKey(execution.activeGeneration.commandTarget)
      : null;
    return execution.activeGeneration.transcript.flatMap((terminal) => {
      if (
        terminal.type !== "lifecycle" ||
        terminal.title !== "Turn result" ||
        !exactEventId(terminal.sourceEventId) ||
        !Number.isSafeInteger(terminal.sourceEventSeq) ||
        (terminal.sourceEventSeq ?? -1) < 0 ||
        typeof terminal.sourceTargetKey !== "string" ||
        terminal.sourceTargetKey !== expectedTargetKey
      ) {
        return [];
      }
      const terminalAtMs = Date.parse(terminal.timestamp);
      if (!Number.isFinite(terminalAtMs)) return [];
      const prompt = matchingPrompt(execution, terminal);
      const promptAtMs = prompt ? Date.parse(prompt.timestamp) : Number.NaN;
      if (
        hasReportForTurn({
          operations,
          evidence,
          execution,
          promptAtMs: Number.isFinite(promptAtMs) ? promptAtMs : null,
          terminalAtMs,
        })
      ) {
        return [];
      }
      return [
        {
          sourceEventId: terminal.sourceEventId,
          sourceCreatedAtMs: terminalAtMs,
          sourceEventSeq: terminal.sourceEventSeq ?? null,
          sourceTargetKey: terminal.sourceTargetKey,
          kind: "turn_ended_without_required_operation" as const,
          operationType: null,
          seatRole: role,
          causedByCommandId: prompt?.commandId ?? null,
          preferredCommandId: null,
        },
      ];
    });
  });
}

/** Derive wake facts only from signed catalog/evidence projections. */
export function deriveCodingSessionTeamWakePlan(input: {
  umbrella: CodingSessionUmbrellaRecord;
  evidence: CodingSessionMissionInspectorInput;
  currentUserPubkey: string | null;
  sessionClosed: boolean;
}): CodingSessionTeamWakePlan {
  if (input.sessionClosed) return { lead: null, candidates: [], refusal: null };
  const founder = exactPubkey(input.umbrella.founderPubkey);
  const currentUser = exactPubkey(input.currentUserPubkey);
  if (!founder || currentUser !== founder) {
    return { lead: null, candidates: [], refusal: null };
  }
  if (!input.umbrella.sessionRef || !input.umbrella.genesisRef) {
    return { lead: null, candidates: [], refusal: null };
  }
  const leads = input.umbrella.executions.filter(
    (execution) =>
      normalizedRole(execution.activeGeneration.role) === "lead" &&
      execution.activeGeneration.commandTarget !== null,
  );
  if (leads.length !== 1) {
    return {
      lead: null,
      candidates: [],
      refusal:
        leads.length === 0
          ? "Automatic team delivery has no live lead target."
          : "Automatic team delivery found more than one live lead target.",
    };
  }
  const operations = operationCandidates(input.umbrella, input.evidence);
  return {
    lead: leads[0],
    candidates: [
      ...operations,
      ...terminalCandidates(input.umbrella, input.evidence, operations),
    ].sort(compareCandidate),
    refusal: null,
  };
}

export function codingSessionTeamWakeText(
  candidate: CodingSessionTeamWakeCandidate,
): string {
  return candidate.kind === "operation_ready"
    ? JSON.stringify({
        operationId: candidate.sourceEventId,
        type: candidate.operationType,
      })
    : JSON.stringify({
        schema: "buzz-team-wake/v1",
        type: candidate.kind,
        terminalEventId: candidate.sourceEventId,
        seatRole: candidate.seatRole,
        causedByCommandId: candidate.causedByCommandId,
      });
}

export function codingSessionTeamWakeStorageKey(input: {
  communityScope: string;
  channelId: string;
  sessionRef: string;
}): string {
  return `${STORAGE_PREFIX}${encodeURIComponent(
    [input.communityScope, input.channelId, input.sessionRef].join("\u0000"),
  )}`;
}

function emptyState(): CodingSessionTeamWakeState {
  return { schema: STATE_SCHEMA, cursor: null, pending: [] };
}

function isCandidate(value: unknown): value is CodingSessionTeamWakeCandidate {
  if (!value || typeof value !== "object" || Array.isArray(value)) return false;
  const candidate = value as Partial<CodingSessionTeamWakeCandidate>;
  return (
    exactEventId(candidate.sourceEventId) &&
    Number.isSafeInteger(candidate.sourceCreatedAtMs) &&
    (candidate.sourceCreatedAtMs ?? -1) >= 0 &&
    (candidate.sourceEventSeq === null ||
      (Number.isSafeInteger(candidate.sourceEventSeq) &&
        (candidate.sourceEventSeq ?? -1) >= 0)) &&
    (candidate.sourceTargetKey === null ||
      boundedText(candidate.sourceTargetKey, MAX_COMMAND_ID_BYTES)) &&
    (candidate.kind === "operation_ready" ||
      candidate.kind === "turn_ended_without_required_operation") &&
    ((candidate.kind === "operation_ready" &&
      candidate.operationType === "report") ||
      (candidate.kind === "turn_ended_without_required_operation" &&
        candidate.operationType === null)) &&
    boundedText(candidate.seatRole, MAX_ROLE_BYTES) &&
    optionalBoundedText(candidate.causedByCommandId, MAX_COMMAND_ID_BYTES) &&
    optionalBoundedText(candidate.preferredCommandId, MAX_COMMAND_ID_BYTES)
  );
}

export function readCodingSessionTeamWakeState(
  storage: Pick<Storage, "getItem">,
  key: string,
): CodingSessionTeamWakeState | null {
  let value: unknown;
  try {
    const raw = storage.getItem(key);
    if (raw === null) return null;
    value = JSON.parse(raw);
  } catch {
    return null;
  }
  if (!value || typeof value !== "object" || Array.isArray(value)) return null;
  const record = value as Partial<CodingSessionTeamWakeState>;
  if (record.schema !== STATE_SCHEMA || !Array.isArray(record.pending)) {
    return null;
  }
  const cursor = record.cursor;
  if (
    cursor !== null &&
    (!cursor ||
      !Number.isSafeInteger(cursor.createdAtMs) ||
      cursor.createdAtMs < 0 ||
      !exactEventId(cursor.sourceEventId))
  ) {
    return null;
  }
  const pending = record.pending.filter(
    (attempt): attempt is CodingSessionTeamWakeAttempt => {
      if (!attempt || typeof attempt !== "object" || Array.isArray(attempt)) {
        return false;
      }
      const row = attempt as Partial<CodingSessionTeamWakeAttempt>;
      return (
        exactEventId(row.sourceEventId) &&
        boundedText(row.commandId, MAX_COMMAND_ID_BYTES) &&
        boundedText(row.leadTargetKey, MAX_COMMAND_ID_BYTES) &&
        isCandidate(row.candidate)
      );
    },
  );
  return {
    schema: STATE_SCHEMA,
    cursor: cursor ? { ...cursor } : null,
    pending: pending.slice(-MAX_PENDING_WAKES).map((attempt) => ({
      ...attempt,
      candidate: { ...attempt.candidate },
    })),
  };
}

export function writeCodingSessionTeamWakeState(
  storage: Pick<Storage, "setItem">,
  key: string,
  state: CodingSessionTeamWakeState,
): void {
  storage.setItem(
    key,
    JSON.stringify({
      ...state,
      pending: state.pending.slice(-MAX_PENDING_WAKES),
    }),
  );
}

export function baselineCodingSessionTeamWakes(
  candidates: readonly CodingSessionTeamWakeCandidate[],
): CodingSessionTeamWakeState {
  const newest = [...candidates].sort(compareCandidate).at(-1) ?? null;
  return {
    ...emptyState(),
    cursor: newest
      ? {
          createdAtMs: newest.sourceCreatedAtMs,
          sourceEventId: newest.sourceEventId,
        }
      : null,
  };
}

export function pendingCodingSessionTeamWake(input: {
  state: CodingSessionTeamWakeState;
  candidates: readonly CodingSessionTeamWakeCandidate[];
  leadTarget: CodingSessionCommandTarget;
  acknowledgedCommandIds: ReadonlySet<string>;
  attemptedThisMount: ReadonlySet<string>;
}): CodingSessionTeamWakeCandidate | null {
  const targetKey = buildCodingSessionTargetKey(input.leadTarget);
  const candidateById = new Map(
    input.candidates.map((candidate) => [candidate.sourceEventId, candidate]),
  );
  const unresolved = input.state.pending.filter(
    (attempt) => !input.acknowledgedCommandIds.has(attempt.commandId),
  );
  const retry = unresolved.find(
    (attempt) =>
      candidateById.has(attempt.sourceEventId) &&
      (attempt.leadTargetKey !== targetKey ||
        !input.attemptedThisMount.has(attempt.commandId)),
  );
  if (retry) return candidateById.get(retry.sourceEventId) ?? null;
  return (
    input.candidates.find(
      (candidate) =>
        isAfterCursor(candidate, input.state.cursor) &&
        !(
          candidate.preferredCommandId &&
          input.acknowledgedCommandIds.has(candidate.preferredCommandId)
        ),
    ) ?? null
  );
}

export function recordCodingSessionTeamWakeAttempt(input: {
  state: CodingSessionTeamWakeState;
  candidate: CodingSessionTeamWakeCandidate;
  commandId: string;
  leadTarget: CodingSessionCommandTarget;
  acknowledgedCommandIds: ReadonlySet<string>;
}): CodingSessionTeamWakeState {
  const pending = input.state.pending.filter(
    (attempt) =>
      attempt.sourceEventId !== input.candidate.sourceEventId &&
      !input.acknowledgedCommandIds.has(attempt.commandId),
  );
  pending.push({
    sourceEventId: input.candidate.sourceEventId,
    commandId: input.commandId,
    leadTargetKey: buildCodingSessionTargetKey(input.leadTarget),
    candidate: { ...input.candidate },
  });
  const cursorCandidate = {
    createdAtMs: input.candidate.sourceCreatedAtMs,
    sourceEventId: input.candidate.sourceEventId,
  };
  const cursor = isAfterCursor(input.candidate, input.state.cursor)
    ? cursorCandidate
    : input.state.cursor;
  return {
    schema: STATE_SCHEMA,
    cursor,
    pending: pending.slice(-MAX_PENDING_WAKES),
  };
}

export function acknowledgeCodingSessionTeamWakes(
  state: CodingSessionTeamWakeState,
  acknowledgedCommandIds: ReadonlySet<string>,
): CodingSessionTeamWakeState {
  return {
    ...state,
    pending: state.pending.filter(
      (attempt) => !acknowledgedCommandIds.has(attempt.commandId),
    ),
  };
}

export async function buildCodingSessionTeamWakeCommandId(
  candidate: CodingSessionTeamWakeCandidate,
  leadTarget: CodingSessionCommandTarget,
): Promise<string> {
  if (candidate.preferredCommandId) return candidate.preferredCommandId;
  const bytes = new TextEncoder().encode(
    buildCodingSessionTargetKey(leadTarget),
  );
  const digest = await crypto.subtle.digest("SHA-256", bytes);
  const targetDigest = [...new Uint8Array(digest)]
    .slice(0, 12)
    .map((value) => value.toString(16).padStart(2, "0"))
    .join("");
  return `team-wake-v1:${candidate.sourceEventId}:${targetDigest}`;
}
