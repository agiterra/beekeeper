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
import { formatCodingSessionExecutionLabel } from "./codingSessionLabels";
import type {
  CodingSessionCatalogRecord,
  CodingSessionExecution,
  CodingSessionStatus,
  CodingSessionUmbrellaRecord,
  CodingSessionWorkspaceStatus,
} from "./codingSessionTypes";
import { formatCoordinationAge } from "@/shared/coordination/sessionCoordinationFormat";

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
  /** Channel whose signed 44221 carried this create. */
  channelId: string;
  /** The sessionRef the signed create claimed, or null (pre-umbrella create). */
  sessionRef: string | null;
  /** The human operator who signed the 44221. */
  signerPubkey: string;
  /** Event `created_at`, in seconds. */
  createdAt: number;
  eventId: string;
  /** Explicit authority anchor named by this create, or null for legacy. */
  genesisRef: string | null;
  /** Genesis signer resolved by that exact event id, never by session tag. */
  genesisFounderPubkey: string | null;
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
 * - Founder = signer of the exact genesis named by receipt-joined creates.
 *   Legacy sessions alone fall back to the earliest create signer. An
 *   execution's operator remains the signer of the create that minted it.
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

/**
 * Does this umbrella hold history the single-generation tree cannot show?
 *
 * The umbrella surface is the only view that renders `priorGenerations`, and
 * it was routed to on execution count alone. That misses the commonest way a
 * session acquires history: a resume, which adds a generation to the *same*
 * execution rather than a second execution. The result was a resumed session
 * rendering its newest generation only — a transcript that looked erased when
 * every earlier turn was present, verified, and already in the ingress store.
 *
 * Generations count for the same reason executions do: both are collapsed
 * history the flat tree drops on the floor. Every surface that mirrors "does
 * this umbrella render the umbrella workspace" must ask this, not count
 * executions — the participant roster and lane visibility both do.
 */
export function umbrellaHasCollapsedHistory(
  umbrella: CodingSessionUmbrellaRecord,
): boolean {
  return (
    umbrella.executions.length > 1 ||
    umbrella.executions.some(
      (execution) => execution.priorGenerations.length > 0,
    )
  );
}

export function listCodingSessionUmbrellaParticipants(
  umbrella: CodingSessionUmbrellaRecord,
  resolveActorName?: CodingSessionActorNameResolver,
): CodingSessionUmbrellaParticipant[] {
  const labels = umbrella.executions.map((execution) =>
    executionLabel(execution, resolveActorName),
  );
  const counts = new Map<string, number>();
  for (const label of labels) {
    counts.set(label, (counts.get(label) ?? 0) + 1);
  }
  // Two executions of the same runtime and model, created by the same
  // provider, produced two chips reading `Codex · default · 1958c6c4…9644` —
  // identical, so the selector offered a choice nobody could make (observed
  // live 2026-08-24). The signer disambiguates *across* providers; within one
  // it says nothing, and the execution's own session id is the only thing left
  // that differs.
  const signerLabels = umbrella.executions.map((execution, index) =>
    (counts.get(labels[index]) ?? 0) > 1
      ? `${labels[index]} · ${truncatePubkey(execution.signerPubkey)}`
      : labels[index],
  );
  const signerCounts = new Map<string, number>();
  for (const label of signerLabels) {
    signerCounts.set(label, (signerCounts.get(label) ?? 0) + 1);
  }
  const participants: CodingSessionUmbrellaParticipant[] =
    umbrella.executions.map((execution, index) => {
      const label = signerLabels[index];
      // When the signer did not separate them either, it is provably the same
      // for every colliding row — so it identifies nothing and is dropped
      // rather than stacked in front of the id that does the work. Live, that
      // read `Claude · sonnet · 1958c6c4…9644 · 2655de24` three times over.
      const disambiguated =
        (signerCounts.get(label) ?? 0) > 1
          ? `${labels[index]} · ${executionShortId(execution)}`
          : label;
      return {
        kind: "execution",
        executionKey: execution.executionKey,
        label: disambiguated,
        execution,
      };
    });
  // The Session lane exists whenever the umbrella workspace renders — a
  // resumed single execution included. Counting executions here left a resumed
  // session with a visible lane the composer could not address.
  if (umbrella.sessionRef !== null && umbrellaHasCollapsedHistory(umbrella)) {
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
  const authority = resolveFounder(sessionRef, ordered, creates);
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
    founderPubkey: authority.founderPubkey,
    genesisRef: authority.genesisRef,
    genesisResolution: authority.genesisResolution,
    status: deriveUmbrellaStatus(ordered, latest),
    lastEventAt: new Date(
      Math.max(...ordered.map((execution) => execution.latestEventMs), 0),
    ).toISOString(),
    conflictCount: ordered.reduce(
      (count, execution) => count + execution.conflictCount,
      0,
    ),
    foreignAttachmentCount:
      authority.founderPubkey === null
        ? 0
        : ordered.filter(
            (execution) =>
              execution.operatorPubkey !== null &&
              execution.operatorPubkey !== authority.founderPubkey,
          ).length,
  };
}

/**
 * Genesis-bearing founder resolution follows explicit event ids only. When no
 * create names a genesis, the legacy projection remains the earliest accepted
 * create signer (or the one execution operator for an implicit umbrella).
 *
 * `genesisResolution` records *how* the answer was reached, so a consumer
 * (the join flow) can tell a genuinely ungoverned session apart from one
 * whose creates simply have not been observed. See
 * `CodingSessionUmbrellaRecord.genesisResolution`.
 */
function resolveFounder(
  sessionRef: string | null,
  executions: readonly ExecutionAccumulator[],
  creates: readonly CodingSessionUmbrellaCreateObservation[],
): {
  founderPubkey: string | null;
  genesisRef: string | null;
  genesisResolution: "governed" | "legacy" | "unresolved" | "conflict";
} {
  if (sessionRef === null) {
    return {
      founderPubkey: executions[0]?.operatorPubkey ?? null,
      genesisRef: null,
      genesisResolution: "legacy",
    };
  }
  const matches = creates
    .filter((create) => create.sessionRef === sessionRef)
    .sort(
      (left, right) =>
        left.createdAt - right.createdAt ||
        left.eventId.localeCompare(right.eventId),
    );
  if (matches.length === 0) {
    // No receipt-joined create observed for a ref-bearing umbrella: a genesis
    // cannot be ruled out, so nothing may treat this session as ungoverned.
    return {
      founderPubkey: null,
      genesisRef: null,
      genesisResolution: "unresolved",
    };
  }
  const genesisRefs = [
    ...new Set(
      matches
        .map((create) => create.genesisRef)
        .filter((value): value is string => typeof value === "string"),
    ),
  ];
  if (genesisRefs.length > 1) {
    return {
      founderPubkey: null,
      genesisRef: null,
      genesisResolution: "conflict",
    };
  }
  const genesisRef = genesisRefs[0] ?? null;
  if (genesisRef !== null) {
    const founders = new Set(
      matches
        .filter((create) => create.genesisRef === genesisRef)
        .map((create) => create.genesisFounderPubkey)
        .filter((value): value is string => typeof value === "string"),
    );
    return {
      founderPubkey: founders.size === 1 ? [...founders][0] : null,
      genesisRef,
      genesisResolution: "governed",
    };
  }
  return {
    founderPubkey: matches[0]?.signerPubkey ?? null,
    genesisRef: null,
    genesisResolution: "legacy",
  };
}

/**
 * A `stop` is per execution and terminal for that execution alone. An umbrella
 * that still holds a live one has not ended.
 */
function isEndedExecution(execution: ExecutionAccumulator): boolean {
  return execution.activeGeneration.status === "stopped";
}

/**
 * The umbrella's status across every execution it holds.
 *
 * Falling through to "the most recently active execution" is what made a
 * session with five executions read **ENDED** in its header the moment the
 * newest one was stopped, while four others — including one answering turns —
 * were still open (observed live 2026-08-24, 11:04Z). A stop is per execution;
 * the umbrella has ended only when every execution has.
 *
 * The order is deliberate: activity outranks quiet, and quiet outranks
 * terminal. Within each tier the most recently active execution decides, so a
 * session that stopped one provider still reports the state of the ones that
 * remain.
 */
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
  if (!isEndedExecution(latest)) return latest.activeGeneration.status;
  // Every remaining tier-3 candidate is quiet, so the newest *observation*
  // decides — the same rule `latest` uses, applied to the survivors. (The
  // caller's array is ordered by attach time, not activity, so this cannot
  // read the last element.)
  const alive = executions.filter((execution) => !isEndedExecution(execution));
  if (alive.length === 0) return latest.activeGeneration.status;
  return alive.reduce((best, candidate) =>
    candidate.latestEventMs > best.latestEventMs ? candidate : best,
  ).activeGeneration.status;
}

/**
 * The shortest thing that still tells two same-signer executions apart: the
 * head of the provider-minted session id from their own command target.
 */
function executionShortId(execution: CodingSessionExecution): string {
  const sessionId = execution.activeGeneration.commandTarget?.sessionId ?? null;
  return sessionId === null
    ? execution.executionKey.slice(-8)
    : sessionId.slice(0, 8);
}

/**
 * Resolve a seated execution's actor to a display name.
 *
 * The participant list is pure, and profile lookup is a hook — so the
 * resolver is passed in by the surface that has one
 * ({@link useUsersBatchQuery}). Without it a seat still labels itself by its
 * role, which is the honest half.
 */
export type CodingSessionActorNameResolver = (
  actorPubkey: string,
) => string | null;

function executionLabel(
  execution: CodingSessionExecution,
  resolveActorName?: CodingSessionActorNameResolver,
): string {
  const record = execution.activeGeneration;
  const runtime =
    record.runtime ?? record.commandTarget?.driver ?? "coding session";
  return formatCodingSessionExecutionLabel({
    agentRef: record.agentRef,
    role: record.role,
    agentDisplayName: record.agentRef
      ? (resolveActorName?.(record.agentRef) ?? null)
      : null,
    runtime,
    model: record.model,
  }).primary;
}

// ---------------------------------------------------------------------------
// Disposition strip
// ---------------------------------------------------------------------------

/**
 * One execution's line in the umbrella disposition strip.
 *
 * The strip answers the question the narrative could not: *is this team still
 * working?* "Last word" is the last execution with activity, so a lead that
 * has already delivered its verdict collapses to a pill while a builder's
 * "Standing by" stays expanded — and the operator cannot tell the run is done
 * (ledger 77, "Umbrella UI (a)").
 */
export type CodingSessionUmbrellaDisposition = {
  executionKey: string;
  /** `Agent · Role` when seated; runtime · model when not — never a pubkey. */
  label: string;
  /** The seat's role slug, or null when this execution carries no seat. */
  role: string | null;
  /** See {@link codingSessionDispositionWord}. */
  disposition: string;
  /** Newest observed transcript timestamp in ms, or null when none has arrived. */
  lastTurnAt: number | null;
};

/**
 * The team word for a resolved workspace status.
 *
 * Only three states have a team word: `live`, `idle`, `released`. Everything
 * else keeps the status's own sentence — a provider nobody is answering for
 * reads "no provider answering", never the comfortable "idle" it is not.
 */
export function codingSessionDispositionWord(
  status: CodingSessionWorkspaceStatus,
): string {
  switch (status.kind) {
    case "working":
      return "live";
    case "idle":
      return "idle";
    case "ended":
      return "released";
    default:
      return status.label.toLowerCase();
  }
}

/**
 * One disposition per execution, the lead's first.
 *
 * `resolveStatus` is supplied by the surface because reachability demotion is
 * a hook the pure model cannot reach — and reading the wire status directly
 * here would let a dead provider report `live`. No new data: every fact comes
 * from the 44223 metadata already on the catalog record.
 */
export function listCodingSessionUmbrellaDispositions(
  umbrella: CodingSessionUmbrellaRecord,
  resolveStatus: (
    execution: CodingSessionExecution,
  ) => CodingSessionWorkspaceStatus,
  resolveActorName?: CodingSessionActorNameResolver,
): CodingSessionUmbrellaDisposition[] {
  const entries = umbrella.executions.map((execution) => ({
    executionKey: execution.executionKey,
    label: executionLabel(execution, resolveActorName),
    role: execution.activeGeneration.role,
    disposition: codingSessionDispositionWord(resolveStatus(execution)),
    lastTurnAt: executionLastTurnAt(execution),
  }));
  // Stable partition, not a sort: the umbrella's own attach order is the only
  // other ordering anything here has agreed on.
  return [
    ...entries.filter((entry) => isLeadRole(entry.role)),
    ...entries.filter((entry) => !isLeadRole(entry.role)),
  ];
}

/** The strip's line, exactly as a person reads it. */
export function formatCodingSessionDispositionLine(
  entry: CodingSessionUmbrellaDisposition,
  nowMs: number = Date.now(),
): string {
  return `${entry.label} · ${entry.disposition} · ${formatLastTurn(entry.lastTurnAt, nowMs)}`;
}

function formatLastTurn(lastTurnAt: number | null, nowMs: number): string {
  // Absence is not a claim: an execution with no transcript has not been
  // quiet for zero seconds, it has said nothing at all.
  if (lastTurnAt === null) return "no turn observed";
  const age = formatCoordinationAge(
    Math.max(0, Math.floor((nowMs - lastTurnAt) / 1_000)),
  );
  return age === "just now" ? "last turn just now" : `last turn ${age} ago`;
}

function isLeadRole(role: string | null): boolean {
  return role?.trim().toLowerCase() === "lead";
}

/** Newest transcript timestamp across every generation of one execution. */
function executionLastTurnAt(execution: CodingSessionExecution): number | null {
  let newest: number | null = null;
  for (const record of [
    ...execution.priorGenerations,
    execution.activeGeneration,
  ]) {
    for (const item of record.transcript) {
      const at = Date.parse(item.timestamp);
      if (!Number.isFinite(at)) continue;
      if (newest === null || at > newest) newest = at;
    }
  }
  return newest;
}
