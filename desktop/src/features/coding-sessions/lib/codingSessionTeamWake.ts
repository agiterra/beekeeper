import {
  buildCodingSessionTargetKey,
  type CodingSessionCommandTarget,
} from "./codingSessionCommand";
import type { CodingSessionMissionInspectorInput } from "./codingSessionMissionInspectorModel";
import {
  MAX_CUSTODIED_WAKES,
  MAX_PUBLISH_FAILURES,
  MAX_RE_ARMED_WAKES,
} from "./codingSessionTeamWakeLedgers";

export {
  clearCodingSessionTeamWakePublishFailure,
  codingSessionTeamWakeCustodyFor,
  codingSessionTeamWakePublishFailed,
  codingSessionTeamWakeReArmCount,
  recordCodingSessionTeamWakeCustody,
  recordCodingSessionTeamWakePublishFailure,
  recordCodingSessionTeamWakeReArm,
  releaseCodingSessionTeamWakeCustody,
} from "./codingSessionTeamWakeLedgers";
export type {
  CodingSessionTeamWakeCustody,
  CodingSessionTeamWakePublishFailure,
  CodingSessionTeamWakeReArm,
} from "./codingSessionTeamWakeLedgers";
import type {
  CodingSessionTeamWakeCustody,
  CodingSessionTeamWakePublishFailure,
  CodingSessionTeamWakeReArm,
} from "./codingSessionTeamWakeLedgers";
import type {
  CodingSessionExecution,
  CodingSessionUmbrellaRecord,
} from "./codingSessionTypes";

const HEX64 = /^[0-9a-f]{64}$/;
/**
 * v2 adds the `reArmed` ledger. v1 blobs are migrated in place on read — the
 * *storage key* deliberately does not move, because a bumped key would strand
 * every already-resolved source and let a months-old report wake the lead
 * again on the next launch.
 */
const STATE_SCHEMA = "buzz-coding-session-team-wake-state/v2";
const LEGACY_STATE_SCHEMA = "buzz-coding-session-team-wake-state/v1";
const STORAGE_PREFIX = "buzz:coding-session-team-wake:v1:";
const MAX_PENDING_WAKES = 128;
const MAX_RESOLVED_SOURCES = 32_000;
const MAX_COMMAND_ID_BYTES = 256;
const MAX_ROLE_BYTES = 64;

/**
 * How long Desktop waits for the provider's own wake before covering for it.
 *
 * Measured from *local observation*, never from any signed `created_at` (I4).
 * The window is unchanged by the receipt work: with receipts read, a wake that
 * is merely slow now settles inside the window instead of racing it.
 */
export const CODING_SESSION_TEAM_WAKE_GRACE_MS = 15_000;

export type CodingSessionTeamWakeCandidate = {
  sourceEventId: string;
  sourceCreatedAtMs: number;
  sourceEventSeq: number | null;
  sourceTargetKey: string | null;
  /** The reporting seat's actor, when the signed source names one. */
  sourceActorPubkey: string | null;
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
  observed: Array<{ sourceEventId: string; fallbackNotBeforeMs: number }>;
  resolvedSourceEventIds: string[];
  reArmed: CodingSessionTeamWakeReArm[];
  custodied: CodingSessionTeamWakeCustody[];
  publishFailed: CodingSessionTeamWakePublishFailure[];
};

export type CodingSessionTeamWakePlan = {
  lead: CodingSessionExecution | null;
  candidates: CodingSessionTeamWakeCandidate[];
  refusal: string | null;
};

export function codingSessionTeamWakeEvidenceIsComplete(input: {
  isLoading: boolean;
  errorMessage: string | null;
}): boolean {
  return !input.isLoading && input.errorMessage === null;
}

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
        sourceActorPubkey: actor,
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
          sourceActorPubkey: exactPubkey(execution.activeGeneration.agentRef),
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
  return {
    schema: STATE_SCHEMA,
    cursor: null,
    pending: [],
    observed: [],
    resolvedSourceEventIds: [],
    reArmed: [],
    custodied: [],
    publishFailed: [],
  };
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
    (candidate.sourceActorPubkey === undefined ||
      candidate.sourceActorPubkey === null ||
      exactPubkey(candidate.sourceActorPubkey) !== null) &&
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
  if (
    (record.schema !== STATE_SCHEMA && record.schema !== LEGACY_STATE_SCHEMA) ||
    !Array.isArray(record.pending)
  ) {
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
  const observed = (
    Array.isArray(record.observed) ? record.observed : []
  ).filter(
    (row): row is { sourceEventId: string; fallbackNotBeforeMs: number } =>
      !!row &&
      typeof row === "object" &&
      !Array.isArray(row) &&
      exactEventId(row.sourceEventId) &&
      Number.isSafeInteger(row.fallbackNotBeforeMs) &&
      row.fallbackNotBeforeMs >= 0,
  );
  const resolvedSourceEventIds = Array.isArray(record.resolvedSourceEventIds)
    ? record.resolvedSourceEventIds.filter(exactEventId)
    : [];
  if (resolvedSourceEventIds.length > MAX_RESOLVED_SOURCES) return null;
  // v1 knew no re-arms, so the migration is exactly "none have been spent".
  // Nothing else about a v1 blob changes, which is what keeps an upgrade from
  // resurrecting sources the previous version had already retired.
  const reArmed = (Array.isArray(record.reArmed) ? record.reArmed : []).filter(
    (row): row is CodingSessionTeamWakeReArm =>
      !!row &&
      typeof row === "object" &&
      !Array.isArray(row) &&
      exactEventId(row.sourceEventId) &&
      boundedText(row.leadTargetKey, MAX_COMMAND_ID_BYTES) &&
      boundedText(row.commandId, MAX_COMMAND_ID_BYTES),
  );
  const custodied = (
    Array.isArray(record.custodied) ? record.custodied : []
  ).filter(
    (row): row is CodingSessionTeamWakeCustody =>
      !!row &&
      typeof row === "object" &&
      !Array.isArray(row) &&
      exactEventId(row.sourceEventId) &&
      boundedText(row.leadTargetKey, MAX_COMMAND_ID_BYTES) &&
      boundedText(row.commandId, MAX_COMMAND_ID_BYTES),
  );
  const publishFailed = (
    Array.isArray(record.publishFailed) ? record.publishFailed : []
  ).filter(
    (row): row is CodingSessionTeamWakePublishFailure =>
      !!row &&
      typeof row === "object" &&
      !Array.isArray(row) &&
      exactEventId(row.sourceEventId) &&
      boundedText(row.leadTargetKey, MAX_COMMAND_ID_BYTES),
  );
  return {
    schema: STATE_SCHEMA,
    cursor: cursor ? { ...cursor } : null,
    pending: pending.slice(-MAX_PENDING_WAKES).map((attempt) => ({
      ...attempt,
      candidate: normalizeCandidate(attempt.candidate),
    })),
    observed: observed.slice(-MAX_PENDING_WAKES).map((row) => ({ ...row })),
    resolvedSourceEventIds: [...new Set(resolvedSourceEventIds)],
    reArmed: reArmed.slice(-MAX_RE_ARMED_WAKES).map((row) => ({ ...row })),
    custodied: custodied.slice(-MAX_CUSTODIED_WAKES).map((row) => ({
      ...row,
      // A row written before the origin was recorded says nothing about who
      // held it, and the conservative reading is "not ours".
      fromProvider: row.fromProvider !== false,
    })),
    publishFailed: publishFailed
      .slice(-MAX_PUBLISH_FAILURES)
      .map((row) => ({ ...row })),
  };
}

/** A v1 candidate carries no actor; an absent one is unknown, never empty. */
function normalizeCandidate(
  candidate: CodingSessionTeamWakeCandidate,
): CodingSessionTeamWakeCandidate {
  return {
    ...candidate,
    sourceActorPubkey: exactPubkey(candidate.sourceActorPubkey),
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
      observed: state.observed.slice(-MAX_PENDING_WAKES),
      reArmed: state.reArmed.slice(-MAX_RE_ARMED_WAKES),
      custodied: state.custodied.slice(-MAX_CUSTODIED_WAKES),
      publishFailed: state.publishFailed.slice(-MAX_PUBLISH_FAILURES),
    }),
  );
}

export function observeCodingSessionTeamWake(
  state: CodingSessionTeamWakeState,
  sourceEventId: string,
  fallbackNotBeforeMs: number,
): CodingSessionTeamWakeState {
  if (
    !exactEventId(sourceEventId) ||
    !Number.isSafeInteger(fallbackNotBeforeMs) ||
    fallbackNotBeforeMs < 0 ||
    state.observed.some((row) => row.sourceEventId === sourceEventId)
  ) {
    return state;
  }
  return {
    ...state,
    observed: [...state.observed, { sourceEventId, fallbackNotBeforeMs }].slice(
      -MAX_PENDING_WAKES,
    ),
  };
}

export function codingSessionTeamWakeFallbackNotBefore(
  state: CodingSessionTeamWakeState,
  sourceEventId: string,
): number | null {
  return (
    state.observed.find((row) => row.sourceEventId === sourceEventId)
      ?.fallbackNotBeforeMs ?? null
  );
}

export function baselineCodingSessionTeamWakes(
  candidates: readonly CodingSessionTeamWakeCandidate[],
): CodingSessionTeamWakeState {
  return {
    ...emptyState(),
    resolvedSourceEventIds: [
      ...new Set(candidates.map((row) => row.sourceEventId)),
    ],
  };
}

export function migrateCodingSessionTeamWakeCursor(
  state: CodingSessionTeamWakeState,
  candidates: readonly CodingSessionTeamWakeCandidate[],
): CodingSessionTeamWakeState {
  if (state.cursor === null) return state;
  const resolved = new Set(state.resolvedSourceEventIds);
  for (const candidate of candidates) {
    if (!isAfterCursor(candidate, state.cursor)) {
      resolved.add(candidate.sourceEventId);
    }
  }
  return {
    ...state,
    cursor: null,
    resolvedSourceEventIds: [...resolved].slice(0, MAX_RESOLVED_SOURCES),
  };
}

/**
 * The next source Desktop may still have to cover for, or `null`.
 *
 * `suppressedSourceEventIds` is the receipt-backed half added in this batch: a
 * source whose operation any command currently holds — custody at
 * `turn_queued`, or permanent resolution at `turn_started`/echo — belongs to
 * the lead runner, and Desktop must not mint a second command for it however
 * long the start takes to arrive.
 *
 * `skipSourceEventIds` is the caller's per-mount exclusion list. It exists so
 * that one source whose publish threw does not stop every source behind it
 * from being covered: the caller skips it and the scan continues.
 */
export function pendingCodingSessionTeamWake(input: {
  state: CodingSessionTeamWakeState;
  candidates: readonly CodingSessionTeamWakeCandidate[];
  leadTarget: CodingSessionCommandTarget;
  acknowledgedCommandIds: ReadonlySet<string>;
  acknowledgedSourceEventIds?: ReadonlySet<string>;
  suppressedSourceEventIds?: ReadonlySet<string>;
  skipSourceEventIds?: ReadonlySet<string>;
  attemptedThisMount: ReadonlySet<string>;
}): CodingSessionTeamWakeCandidate | null {
  const targetKey = buildCodingSessionTargetKey(input.leadTarget);
  const candidateById = new Map(
    input.candidates.map((candidate) => [candidate.sourceEventId, candidate]),
  );
  const unresolved = input.state.pending.filter(
    (attempt) =>
      !input.acknowledgedCommandIds.has(attempt.commandId) &&
      !input.acknowledgedSourceEventIds?.has(attempt.sourceEventId) &&
      !input.suppressedSourceEventIds?.has(attempt.sourceEventId) &&
      !input.skipSourceEventIds?.has(attempt.sourceEventId),
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
        !input.state.resolvedSourceEventIds.includes(candidate.sourceEventId) &&
        !input.acknowledgedSourceEventIds?.has(candidate.sourceEventId) &&
        !input.suppressedSourceEventIds?.has(candidate.sourceEventId) &&
        !input.skipSourceEventIds?.has(candidate.sourceEventId) &&
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
  acknowledgedSourceEventIds?: ReadonlySet<string>;
}): CodingSessionTeamWakeState {
  const pending = input.state.pending.filter(
    (attempt) =>
      attempt.sourceEventId !== input.candidate.sourceEventId &&
      !input.acknowledgedCommandIds.has(attempt.commandId) &&
      !input.acknowledgedSourceEventIds?.has(attempt.sourceEventId),
  );
  pending.push({
    sourceEventId: input.candidate.sourceEventId,
    commandId: input.commandId,
    leadTargetKey: buildCodingSessionTargetKey(input.leadTarget),
    candidate: { ...input.candidate },
  });
  return {
    schema: STATE_SCHEMA,
    cursor: input.state.cursor,
    pending: pending.slice(-MAX_PENDING_WAKES),
    observed: input.state.observed.filter(
      (row) => row.sourceEventId !== input.candidate.sourceEventId,
    ),
    resolvedSourceEventIds: input.state.resolvedSourceEventIds,
    reArmed: input.state.reArmed,
    custodied: input.state.custodied,
    publishFailed: input.state.publishFailed,
  };
}

/** The commands currently holding this source's operation, if any. */

/** Record that Desktop's own publish for this source threw. Idempotent. */

/** Forget a recorded publish failure, because a later attempt succeeded. */

/** Whether Desktop's own publish for this source and target threw. */

/** How many Desktop re-arms this source and target have already spent: 0 or 1. */

/**
 * Move every permanently resolved source into the durable ledger.
 *
 * "Permanently resolved" is deliberately narrow: a matching command reached
 * `turn_started`, or the lead echoed the pointer. Custody at `turn_queued`
 * suppresses but is **not** written here — a custodian can still be dropped,
 * and a source retired on custody alone could never be re-armed.
 *
 * The ledger is what survives the bounded transcript and receipt projections
 * ageing out: once a source is in it, no later loss of evidence can resurrect
 * it as a candidate (acceptance #6).
 */
export function acknowledgeCodingSessionTeamWakes(
  state: CodingSessionTeamWakeState,
  acknowledgedCommandIds: ReadonlySet<string>,
  acknowledgedSourceEventIds: ReadonlySet<string> = new Set(),
  resolvedSourceEventIds: ReadonlySet<string> = new Set(),
): CodingSessionTeamWakeState {
  const resolved = new Set(state.resolvedSourceEventIds);
  for (const sourceEventId of [
    ...acknowledgedSourceEventIds,
    ...resolvedSourceEventIds,
  ]) {
    if (exactEventId(sourceEventId)) resolved.add(sourceEventId);
  }
  for (const attempt of state.pending) {
    if (
      acknowledgedCommandIds.has(attempt.commandId) ||
      acknowledgedSourceEventIds.has(attempt.sourceEventId) ||
      resolvedSourceEventIds.has(attempt.sourceEventId)
    ) {
      resolved.add(attempt.sourceEventId);
    }
  }
  const nextResolved = [...resolved].slice(0, MAX_RESOLVED_SOURCES);
  const resolvedSet = new Set(nextResolved);
  return {
    ...state,
    pending: state.pending.filter(
      (attempt) =>
        !acknowledgedCommandIds.has(attempt.commandId) &&
        !acknowledgedSourceEventIds.has(attempt.sourceEventId) &&
        !resolvedSourceEventIds.has(attempt.sourceEventId),
    ),
    observed: state.observed.filter(
      (row) => !resolvedSet.has(row.sourceEventId),
    ),
    // A permanently resolved source needs no custodian: the turn ran.
    custodied: state.custodied.filter(
      (row) => !resolvedSet.has(row.sourceEventId),
    ),
    publishFailed: state.publishFailed.filter(
      (row) => !resolvedSet.has(row.sourceEventId),
    ),
    resolvedSourceEventIds: nextResolved,
  };
}

/**
 * The deterministic command id for one source against one lead generation.
 *
 * `ignorePreferred` exists for the re-arm path: a provider's own
 * `preferredCommandId` names the command that already failed, and reusing it
 * would be answered by the runner's own command-id fence instead of running.
 */
export async function buildCodingSessionTeamWakeCommandId(
  candidate: CodingSessionTeamWakeCandidate,
  leadTarget: CodingSessionCommandTarget,
  options: { ignorePreferred?: boolean } = {},
): Promise<string> {
  if (candidate.preferredCommandId && !options.ignorePreferred) {
    return candidate.preferredCommandId;
  }
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

/**
 * The id of the single Desktop re-arm for one source and lead generation.
 *
 * Always minted in the `team-wake-v1:` form and always suffixed `:r1`, so the
 * runner sees a genuinely new command while the suffix keeps the whole thing
 * derivable — one source and target can name exactly one re-arm, forever.
 */
export async function buildCodingSessionTeamWakeReArmCommandId(
  candidate: CodingSessionTeamWakeCandidate,
  leadTarget: CodingSessionCommandTarget,
): Promise<string> {
  const base = await buildCodingSessionTeamWakeCommandId(
    candidate,
    leadTarget,
    { ignorePreferred: true },
  );
  return `${base}:r1`;
}
