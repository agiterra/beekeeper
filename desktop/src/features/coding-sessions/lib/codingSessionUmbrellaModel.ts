/**
 * Pure grouping of the flat per-generation catalog into umbrella sessions.
 *
 * Two levels above the catalog record: an *execution* is one provider runtime
 * session across its generations — identity `(signer, driver, instanceId,
 * sessionId)`, the `cs-target` minus `generation` — and an *umbrella* is all
 * executions sharing a `sessionRef`. Records with no sessionRef form an
 * implicit umbrella of one, so a single-execution session renders exactly as
 * today. No React, no I/O; the catalog itself stays flat.
 */
import { truncatePubkey } from "@/shared/lib/pubkey";
import type { CodingSessionCommandTarget } from "./codingSessionCommand";
import { encodeStructuredKey } from "./codingSessionKeys";
import { formatCodingSessionRuntimeLabel } from "./codingSessionLabels";
import type {
  CodingSessionCatalogRecord,
  CodingSessionExecution,
  CodingSessionStatus,
  CodingSessionUmbrellaRecord,
} from "./codingSessionTypes";

/**
 * One observed, accepted 44221 create, as far as umbrella authority needs it.
 *
 * The operator-signed create is the authoritative membership claim; the 44223
 * echo the catalog carries is a projection convenience. Callers that replay
 * creates supply these so founder/operator resolution can bind executions to
 * the humans who signed them; without them both resolve to null and every
 * rendering stays observer-grade.
 */
export type CodingSessionUmbrellaCreateObservation = {
  /** The sessionRef the signed create claimed, or null (pre-umbrella create). */
  sessionRef: string | null;
  /** The human operator who signed the 44221. */
  signerPubkey: string;
  /** Event `created_at`, in seconds. */
  createdAt: number;
  eventId: string;
  /**
   * The execution target the provider minted for this create (joined via the
   * 44224 receipt's commandId), when known.
   */
  target: CodingSessionCommandTarget | null;
};

/** Collision-free execution identity: the target tuple minus `generation`. */
export function buildCodingSessionExecutionKey(
  signerPubkey: string | null,
  target: CodingSessionCommandTarget,
): string {
  return encodeStructuredKey(
    "coding-session-execution/v1",
    signerPubkey ?? "",
    "target",
    target.driver,
    target.instanceId,
    target.sessionId,
  );
}

/**
 * Group the flat catalog into umbrella records.
 *
 * - Generations of one execution collapse: highest generation is active,
 *   earlier ones become prior history (never merged, still per-record).
 * - Executions sharing a non-null `sessionRef` share an umbrella; records
 *   without one become implicit umbrellas of one.
 * - Founder = signer of the earliest create bearing the sessionRef
 *   (tie-break: lowest event id); an execution's operator = the signer of the
 *   create that minted its target. Both are null when no create observation
 *   resolves them.
 * - Executions whose known operator differs from the founder are counted as
 *   foreign attachments — flagged, never hidden and never merged.
 */
export function groupCodingSessionCatalog(
  entries: readonly CodingSessionCatalogRecord[],
  creates: readonly CodingSessionUmbrellaCreateObservation[] = [],
): CodingSessionUmbrellaRecord[] {
  const executions = collectExecutions(entries, creates);

  const umbrellas = new Map<
    string,
    { sessionRef: string | null; executions: ExecutionAccumulator[] }
  >();
  for (const execution of executions) {
    const key =
      execution.sessionRef !== null
        ? execution.sessionRef
        : `implicit:${execution.executionKey}`;
    const existing = umbrellas.get(key);
    if (existing) {
      existing.executions.push(execution);
    } else {
      umbrellas.set(key, {
        sessionRef: execution.sessionRef,
        executions: [execution],
      });
    }
  }

  const records = [...umbrellas.entries()].map(([umbrellaKey, group]) =>
    buildUmbrellaRecord(
      umbrellaKey,
      group.sessionRef,
      group.executions,
      creates,
    ),
  );
  records.sort(
    (left, right) =>
      Date.parse(right.lastEventAt) - Date.parse(left.lastEventAt) ||
      left.umbrellaKey.localeCompare(right.umbrellaKey),
  );
  return records;
}

/**
 * The composer's participant list for an umbrella.
 *
 * One entry per execution, labelled runtime · model, plus a "Session" entry
 * addressing the conversation lane. The Session entry appears only when the
 * umbrella actually has a lane to address — a claimed sessionRef and more
 * than one execution. At N=1 the list is the single execution alone, so the
 * composer renders no selector and a single-provider session looks exactly
 * like today.
 */
export type CodingSessionUmbrellaParticipant =
  | {
      kind: "execution";
      executionKey: string;
      label: string;
      execution: CodingSessionExecution;
    }
  | { kind: "session"; sessionRef: string; label: "Session" };

export function listCodingSessionUmbrellaParticipants(
  umbrella: CodingSessionUmbrellaRecord,
): CodingSessionUmbrellaParticipant[] {
  const labels = umbrella.executions.map((execution) =>
    executionLabel(execution),
  );
  const counts = new Map<string, number>();
  for (const label of labels) {
    counts.set(label, (counts.get(label) ?? 0) + 1);
  }
  const participants: CodingSessionUmbrellaParticipant[] =
    umbrella.executions.map((execution, index) => {
      const label = labels[index];
      const disambiguated =
        (counts.get(label) ?? 0) > 1
          ? `${label} · ${truncatePubkey(execution.signerPubkey)}`
          : label;
      return {
        kind: "execution",
        executionKey: execution.executionKey,
        label: disambiguated,
        execution,
      };
    });
  if (umbrella.sessionRef !== null && umbrella.executions.length > 1) {
    participants.push({
      kind: "session",
      sessionRef: umbrella.sessionRef,
      label: "Session",
    });
  }
  return participants;
}

// ---------------------------------------------------------------------------
// Internals
// ---------------------------------------------------------------------------

type ExecutionAccumulator = CodingSessionExecution & {
  sessionRef: string | null;
  /** Best-effort attach ordering: create time when known, else earliest event. */
  attachOrderMs: number;
  earliestEventMs: number;
  latestEventMs: number;
  conflictCount: number;
};

function collectExecutions(
  entries: readonly CodingSessionCatalogRecord[],
  creates: readonly CodingSessionUmbrellaCreateObservation[],
): ExecutionAccumulator[] {
  const byKey = new Map<string, CodingSessionCatalogRecord[]>();
  const keyOrder: string[] = [];
  for (const record of entries) {
    const key = record.commandTarget
      ? buildCodingSessionExecutionKey(
          record.providerAuthorityPubkey,
          record.commandTarget,
        )
      : encodeStructuredKey(
          "coding-session-execution/v1",
          record.providerAuthorityPubkey ?? "",
          "untargeted",
          record.generationId,
        );
    const bucket = byKey.get(key);
    if (bucket) {
      bucket.push(record);
    } else {
      byKey.set(key, [record]);
      keyOrder.push(key);
    }
  }

  return keyOrder.map((executionKey) => {
    const records = byKey.get(executionKey) ?? [];
    const generations = [...records].sort(
      (left, right) =>
        (left.commandTarget?.generation ?? 0) -
          (right.commandTarget?.generation ?? 0) ||
        left.generationId.localeCompare(right.generationId),
    );
    const activeGeneration = generations[generations.length - 1];
    const priorGenerations = generations.slice(0, -1);
    // The echo can lag a generation bump; the newest non-null claim wins, and
    // a missing echo on the active generation falls back to any prior one.
    const sessionRef =
      [...generations]
        .reverse()
        .map((record) => record.sessionRef)
        .find((ref) => ref !== null) ?? null;
    const eventTimes = generations
      .map((record) => Date.parse(record.lastEventAt))
      .filter((time) => Number.isFinite(time));
    const earliestEventMs = eventTimes.length ? Math.min(...eventTimes) : 0;
    const latestEventMs = eventTimes.length ? Math.max(...eventTimes) : 0;
    const create = resolveExecutionCreate(activeGeneration, creates);
    return {
      executionKey,
      signerPubkey: activeGeneration.providerAuthorityPubkey ?? "",
      activeGeneration,
      priorGenerations,
      operatorPubkey: create?.signerPubkey ?? null,
      sessionRef,
      attachOrderMs:
        create !== null ? create.createdAt * 1000 : earliestEventMs,
      earliestEventMs,
      latestEventMs,
      conflictCount: generations.reduce(
        (count, record) => count + record.conflictCount,
        0,
      ),
    };
  });
}

/** The earliest create observation whose minted target is this execution. */
function resolveExecutionCreate(
  record: CodingSessionCatalogRecord,
  creates: readonly CodingSessionUmbrellaCreateObservation[],
): CodingSessionUmbrellaCreateObservation | null {
  const target = record.commandTarget;
  if (!target) return null;
  const matches = creates
    .filter(
      (create) =>
        create.target !== null &&
        create.target.driver === target.driver &&
        create.target.instanceId === target.instanceId &&
        create.target.sessionId === target.sessionId,
    )
    .sort(
      (left, right) =>
        left.createdAt - right.createdAt ||
        left.eventId.localeCompare(right.eventId),
    );
  return matches[0] ?? null;
}

function buildUmbrellaRecord(
  umbrellaKey: string,
  sessionRef: string | null,
  executions: ExecutionAccumulator[],
  creates: readonly CodingSessionUmbrellaCreateObservation[],
): CodingSessionUmbrellaRecord {
  const ordered = [...executions].sort(
    (left, right) =>
      left.attachOrderMs - right.attachOrderMs ||
      left.executionKey.localeCompare(right.executionKey),
  );
  const founderPubkey = resolveFounder(sessionRef, ordered, creates);
  const latest = ordered.reduce((best, candidate) =>
    candidate.latestEventMs > best.latestEventMs ? candidate : best,
  );
  return {
    umbrellaKey,
    sessionRef,
    title: ordered[0].activeGeneration.title,
    executions: ordered.map(
      ({
        executionKey,
        signerPubkey,
        activeGeneration,
        priorGenerations,
        operatorPubkey,
      }) => ({
        executionKey,
        signerPubkey,
        activeGeneration,
        priorGenerations,
        operatorPubkey,
      }),
    ),
    founderPubkey,
    status: deriveUmbrellaStatus(ordered, latest),
    lastEventAt: new Date(
      Math.max(...ordered.map((execution) => execution.latestEventMs), 0),
    ).toISOString(),
    conflictCount: ordered.reduce(
      (count, execution) => count + execution.conflictCount,
      0,
    ),
    foreignAttachmentCount:
      founderPubkey === null
        ? 0
        : ordered.filter(
            (execution) =>
              execution.operatorPubkey !== null &&
              execution.operatorPubkey !== founderPubkey,
          ).length,
  };
}

/**
 * Founder = signer of the earliest accepted create bearing this sessionRef
 * (tie-break: lowest event id). An implicit umbrella's founder is its one
 * execution's operator, when known.
 */
function resolveFounder(
  sessionRef: string | null,
  executions: readonly ExecutionAccumulator[],
  creates: readonly CodingSessionUmbrellaCreateObservation[],
): string | null {
  if (sessionRef === null) {
    return executions[0]?.operatorPubkey ?? null;
  }
  const matches = creates
    .filter((create) => create.sessionRef === sessionRef)
    .sort(
      (left, right) =>
        left.createdAt - right.createdAt ||
        left.eventId.localeCompare(right.eventId),
    );
  return matches[0]?.signerPubkey ?? null;
}

function deriveUmbrellaStatus(
  executions: readonly ExecutionAccumulator[],
  latest: ExecutionAccumulator,
): CodingSessionStatus {
  if (
    executions.some(
      (execution) => execution.activeGeneration.status === "running",
    )
  ) {
    return "running";
  }
  if (
    executions.some(
      (execution) => execution.activeGeneration.status === "waiting_for_input",
    )
  ) {
    return "waiting_for_input";
  }
  return latest.activeGeneration.status;
}

function executionLabel(execution: CodingSessionExecution): string {
  const record = execution.activeGeneration;
  const runtime =
    record.runtime ?? record.commandTarget?.driver ?? "coding session";
  const runtimeLabel = formatCodingSessionRuntimeLabel(runtime);
  return record.model ? `${runtimeLabel} · ${record.model}` : runtimeLabel;
}
