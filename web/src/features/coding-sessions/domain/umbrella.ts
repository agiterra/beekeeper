/**
 * Grouping per-generation records into the sessions a reader recognises.
 *
 * Two levels above a generation: an *execution* is one provider runtime
 * session across its generations — identity `(signer, driver, instanceId,
 * sessionId)`, the `cs-target` minus `generation` — and an *umbrella* is every
 * execution sharing a `sessionRef`. A record with no `sessionRef` is an
 * implicit umbrella of one (D7).
 *
 * Rewritten from
 * `desktop/src/features/coding-sessions/lib/codingSessionUmbrellaModel.ts`.
 * No I/O, no framework.
 */
import { truncatePubkey } from "../../../shared/lib/pubkey.ts";
import type { CodingSessionObserverFacts } from "./catalog.ts";
import {
  codingSessionTargetFactKey,
  generationExecutionLabel,
} from "./catalog.ts";
import { buildCodingSessionExecutionKey, encodeStructuredKey } from "./keys.ts";
import { resolveCodingSessionReachability } from "./lease.ts";
import type { CodingSessionLifecycleCommand } from "./lifecycleCommand.ts";
import { foldNewestByKey } from "./sessionRecords.ts";
import type {
  CodingSessionExecution,
  CodingSessionFounderResolution,
  CodingSessionGenerationRecord,
  CodingSessionReachabilityReport,
  CodingSessionStatus,
  CodingSessionUmbrella,
} from "./types.ts";

/** The complete answer the session list and session page render from. */
export type CodingSessionObserverSnapshot = {
  umbrellas: CodingSessionUmbrella[];
  /** Reachability per generation id, per D8. */
  reachabilityByGenerationId: Map<string, CodingSessionReachabilityReport>;
  /**
   * Always true: `classifyCodingSessionEvent` verifies every fact's signature
   * on this device before it reaches this fold. Stated as data so a future
   * regression is visible in the UI rather than silent.
   */
  signaturesVerified: boolean;
  /** True when a history page came back at its 1000-event limit. */
  historyTruncated: boolean;
  malformedCount: number;
  invalidSignatureCount: number;
  conflictCount: number;
};

export type BuildSnapshotOptions = {
  /** Wall clock in ms, read once after the last query returned. */
  nowMs: number;
  /** False until a lease read has actually completed — see D8. */
  leasesRead: boolean;
  /** True when any history page came back full at limit 1000. */
  historyTruncated?: boolean;
};

/** Fold verified facts into the observer's rendered state. */
export function buildCodingSessionObserverSnapshot(
  facts: CodingSessionObserverFacts,
  options: BuildSnapshotOptions,
): CodingSessionObserverSnapshot {
  const umbrellas = groupCodingSessionGenerations(facts);
  const reachabilityByGenerationId = new Map<
    string,
    CodingSessionReachabilityReport
  >();
  for (const umbrella of umbrellas) {
    for (const execution of umbrella.executions) {
      const generations = [
        ...execution.priorGenerations,
        execution.activeGeneration,
      ];
      for (const generation of generations) {
        const leases =
          facts.leasesByTarget.get(
            codingSessionTargetFactKey(generation.channelId, generation.target),
          ) ?? [];
        reachabilityByGenerationId.set(
          generation.generationId,
          resolveCodingSessionReachability({
            leases,
            acceptedCommandId:
              facts.acceptedCommandIdByGenerationId.get(
                generation.generationId,
              ) ?? null,
            providerAuthorityPubkey: generation.providerAuthorityPubkey,
            isCurrentGeneration:
              generation.generationId ===
              execution.activeGeneration.generationId,
            leasesRead: options.leasesRead,
            lastReportedStatus: generation.status,
            lastReportedAt: generation.statusAt,
            nowMs: options.nowMs,
          }),
        );
      }
    }
  }
  return {
    umbrellas,
    reachabilityByGenerationId,
    signaturesVerified: true,
    historyTruncated: options.historyTruncated ?? false,
    malformedCount: facts.malformedCount,
    invalidSignatureCount: facts.invalidSignatureCount,
    conflictCount: facts.conflictCount,
  };
}

/** Group generations into executions, then executions into umbrellas (D7). */
export function groupCodingSessionGenerations(
  facts: CodingSessionObserverFacts,
): CodingSessionUmbrella[] {
  const executions = collectExecutions(facts);
  const groups = new Map<
    string,
    { sessionRef: string | null; channelId: string; executions: Execution[] }
  >();
  for (const execution of executions) {
    const key =
      execution.sessionRef !== null
        ? sessionScopeKey(execution.channelId, execution.sessionRef)
        : `implicit:${execution.executionKey}`;
    const existing = groups.get(key);
    if (existing) {
      existing.executions.push(execution);
    } else {
      groups.set(key, {
        sessionRef: execution.sessionRef,
        channelId: execution.channelId,
        executions: [execution],
      });
    }
  }
  const names = foldNewestByKey(facts.names, (name) =>
    sessionScopeKey(name.channelId, name.sessionRef),
  );
  const closures = foldNewestByKey(facts.closures, (closure) =>
    sessionScopeKey(closure.channelId, closure.sessionRef),
  );
  const records = [...groups.entries()].map(([key, group]) =>
    buildUmbrella(key, group, facts, names, closures),
  );
  records.sort(
    (left, right) =>
      right.lastEventAt - left.lastEventAt ||
      left.umbrellaKey.localeCompare(right.umbrellaKey),
  );
  return records;
}

type Execution = CodingSessionExecution & {
  channelId: string;
  sessionRef: string | null;
  attachOrderMs: number;
  latestEventMs: number;
  conflictCount: number;
};

function collectExecutions(facts: CodingSessionObserverFacts): Execution[] {
  const byKey = new Map<string, CodingSessionGenerationRecord[]>();
  const order: string[] = [];
  for (const record of facts.generations) {
    const key = sessionScopeKey(
      record.channelId,
      buildCodingSessionExecutionKey(
        record.providerAuthorityPubkey,
        record.target,
      ),
    );
    const bucket = byKey.get(key);
    if (bucket) {
      bucket.push(record);
    } else {
      byKey.set(key, [record]);
      order.push(key);
    }
  }
  return order.map((executionKey) => {
    const generations = [...(byKey.get(executionKey) ?? [])].sort(
      (left, right) =>
        left.target.generation - right.target.generation ||
        left.generationId.localeCompare(right.generationId),
    );
    const active = generations[generations.length - 1];
    const prior = generations.slice(0, -1);
    // The metadata echo can lag a generation bump; the newest non-null claim
    // wins, and a missing echo on the active generation falls back to a prior.
    const sessionRef =
      [...generations]
        .reverse()
        .map((record) => record.sessionRef)
        .find((ref) => ref !== null) ?? null;
    const create = resolveExecutionCreate(active, facts);
    const latestEventMs = Math.max(
      0,
      ...generations.map((record) => record.lastEventAt),
    );
    return {
      executionKey,
      channelId: active.channelId,
      signerPubkey: active.providerAuthorityPubkey,
      authoritySource: active.authoritySource,
      activeGeneration: active,
      priorGenerations: prior,
      operatorPubkey: create?.signerPubkey ?? null,
      label: generationExecutionLabel(active),
      sessionRef,
      attachOrderMs:
        create !== null
          ? create.createdAt * 1000
          : Math.min(...generations.map((record) => record.lastEventAt)),
      latestEventMs,
      conflictCount: generations.reduce(
        (count, record) => count + record.conflictCount,
        0,
      ),
    };
  });
}

/**
 * The create that minted this execution, or null.
 *
 * `facts.creates` is already receipt-joined, so an unanswered claim can never
 * name an operator. When the store also resolved *which* command the provider
 * confirmed for this generation, that exact id decides and no heuristic runs;
 * only a generation on the disclosed fallback (no readable create for its own
 * stream) falls back to matching on provider and `sessionRef`.
 */
function resolveExecutionCreate(
  record: CodingSessionGenerationRecord,
  facts: CodingSessionObserverFacts,
): CodingSessionLifecycleCommand | null {
  const creates = facts.creates.filter(
    (create) =>
      create.action === "create" && create.channelId === record.channelId,
  );
  const accepted = facts.acceptedCommandIdByGenerationId.get(
    record.generationId,
  );
  if (accepted !== undefined) {
    return creates.find((create) => create.commandId === accepted) ?? null;
  }
  const matches = creates
    .filter(
      (create) =>
        create.providerAuthorityPubkey === record.providerAuthorityPubkey &&
        (create.sessionRef === null ||
          record.sessionRef === null ||
          create.sessionRef === record.sessionRef),
    )
    .sort(
      (left, right) =>
        left.createdAt - right.createdAt ||
        left.eventId.localeCompare(right.eventId),
    );
  return matches[0] ?? null;
}

function buildUmbrella(
  umbrellaKey: string,
  group: {
    sessionRef: string | null;
    channelId: string;
    executions: Execution[];
  },
  facts: CodingSessionObserverFacts,
  names: ReadonlyMap<string, { name: string }>,
  closures: ReadonlyMap<
    string,
    { action: "closed" | "open"; signerPubkey: string }
  >,
): CodingSessionUmbrella {
  const ordered = [...group.executions].sort(
    (left, right) =>
      left.attachOrderMs - right.attachOrderMs ||
      left.executionKey.localeCompare(right.executionKey),
  );
  const authority = resolveFounder(
    group.channelId,
    group.sessionRef,
    ordered,
    facts,
  );
  const scope =
    group.sessionRef === null
      ? null
      : sessionScopeKey(group.channelId, group.sessionRef);
  const closure = scope === null ? undefined : closures.get(scope);
  // A close is an owner-authority act. An unresolved founder cannot authorize
  // one, so an unauthorized `closed` is ignored rather than believed.
  const closed =
    closure?.action === "closed" &&
    authority.founderPubkey !== null &&
    closure.signerPubkey === authority.founderPubkey;
  const declaredName = scope === null ? undefined : names.get(scope)?.name;
  return {
    umbrellaKey,
    channelId: group.channelId,
    sessionRef: group.sessionRef,
    name: declaredName ?? ordered[0].activeGeneration.title ?? ordered[0].label,
    executions: ordered.map((execution) => ({
      executionKey: execution.executionKey,
      signerPubkey: execution.signerPubkey,
      authoritySource: execution.authoritySource,
      activeGeneration: execution.activeGeneration,
      priorGenerations: execution.priorGenerations,
      operatorPubkey: execution.operatorPubkey,
      label: execution.label,
    })),
    founderPubkey: authority.founderPubkey,
    genesisRef: authority.genesisRef,
    founderResolution: authority.founderResolution,
    status: foldUmbrellaStatus(ordered),
    closed,
    lastEventAt: Math.max(
      0,
      ...ordered.map((execution) => execution.latestEventMs),
    ),
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
 * Founder resolution follows explicit event ids only (D7).
 *
 * `founderResolution` records *how* the answer was reached, so a reader can
 * tell a genuinely ungoverned session apart from one whose creates simply have
 * not been observed. Guessing the former over the latter is how an ungoverned
 * execution gets treated as part of a governed session.
 *
 * Only receipt-joined creates are considered — `facts.creates` carries no
 * others — so a create nobody's provider ever answered can neither name a
 * founder nor, through the closure gate in {@link buildUmbrella}, close a
 * session.
 */
function resolveFounder(
  channelId: string,
  sessionRef: string | null,
  executions: readonly Execution[],
  facts: CodingSessionObserverFacts,
): {
  founderPubkey: string | null;
  genesisRef: string | null;
  founderResolution: CodingSessionFounderResolution;
} {
  if (sessionRef === null) {
    return {
      founderPubkey: executions[0]?.operatorPubkey ?? null,
      genesisRef: null,
      founderResolution: "legacy",
    };
  }
  const matches = facts.creates
    .filter(
      (create) =>
        create.action === "create" &&
        create.channelId === channelId &&
        create.sessionRef === sessionRef,
    )
    .sort(
      (left, right) =>
        left.createdAt - right.createdAt ||
        left.eventId.localeCompare(right.eventId),
    );
  if (matches.length === 0) {
    // A ref-bearing umbrella with no readable create: a genesis cannot be
    // ruled out, so nothing may treat this session as ungoverned.
    return {
      founderPubkey: null,
      genesisRef: null,
      founderResolution: "unresolved",
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
      founderResolution: "conflict",
    };
  }
  const genesisRef = genesisRefs[0] ?? null;
  if (genesisRef !== null) {
    // Resolved by that exact event id, never by the session tag: a 44226 that
    // merely claims the same sessionRef is not the anchor the create named.
    const genesis = facts.genesisByEventId.get(genesisRef);
    // ...and the anchor must be this session's own. A create pointing at
    // another session's genesis would otherwise hand this session's
    // foundership — and the authority to close it — to a foreign signer, and
    // label the result "governed" while doing so.
    const anchored =
      genesis !== undefined &&
      genesis.channelId === channelId &&
      genesis.sessionRef === sessionRef;
    return {
      founderPubkey: anchored ? genesis.founderPubkey : null,
      genesisRef,
      founderResolution: "governed",
    };
  }
  return {
    founderPubkey: matches[0].signerPubkey,
    genesisRef: null,
    founderResolution: "legacy",
  };
}

/**
 * The umbrella's status across every execution it holds (D8).
 *
 * A stop is per execution and terminal for that execution alone. Falling
 * through to "the most recently active execution" is what made a session with
 * five executions read ENDED the moment the newest one was stopped, while four
 * others — one of them answering turns — were still open.
 */
export function foldUmbrellaStatus(
  executions: readonly {
    activeGeneration: CodingSessionGenerationRecord;
    latestEventMs: number;
  }[],
): CodingSessionStatus {
  if (executions.length === 0) return "unknown";
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
  const alive = executions.filter(
    (execution) => execution.activeGeneration.status !== "stopped",
  );
  // Ended only when EVERY execution is stopped.
  if (alive.length === 0) return "stopped";
  return alive.reduce((best, candidate) =>
    candidate.latestEventMs > best.latestEventMs ? candidate : best,
  ).activeGeneration.status;
}

/** The chip an umbrella renders in the list and header (D8). */
export function codingSessionStatusChipLabel(
  status: CodingSessionStatus,
): string {
  if (status === "running") return "Working";
  if (status === "waiting_for_input") return "Waiting";
  if (status === "stopped") return "Ended";
  if (status === "failed") return "Failed";
  if (status === "interrupted") return "Interrupted";
  if (status === "disconnected") return "Disconnected";
  if (status === "starting") return "Starting";
  if (status === "completed") return "Completed";
  if (status === "idle") return "Idle";
  return "Status unknown";
}

/**
 * The reachability line D8 requires.
 *
 * A live-sounding status over an unproven provider is a lie the header used to
 * tell for hours at a time, so without a lease the status is demoted to
 * history and printed as what it is: the last thing anyone said.
 */
export function codingSessionReachabilityLine(
  report: CodingSessionReachabilityReport,
  statusLabel: string,
): string {
  if (report.reachability === "provider_reachable") {
    return "Provider reachable";
  }
  if (report.reachability === "unknown") {
    return "Reachability not read yet";
  }
  const age =
    report.lastReportedAgeSeconds === null
      ? null
      : formatAgeSeconds(report.lastReportedAgeSeconds);
  return age === null
    ? "No provider answering"
    : `No provider answering · last reported ${statusLabel} ${age} ago`;
}

/** The founder line, including the three honest failure modes. */
export function codingSessionFounderLabel(
  umbrella: CodingSessionUmbrella,
): string {
  if (umbrella.founderResolution === "conflict") return "founder conflict";
  if (umbrella.founderPubkey === null) return "founder unresolved";
  const suffix = umbrella.founderResolution === "legacy" ? " (legacy)" : "";
  return `${truncatePubkey(umbrella.founderPubkey)}${suffix}`;
}

function formatAgeSeconds(seconds: number): string {
  if (seconds < 60) return `${seconds}s`;
  if (seconds < 3600) return `${Math.floor(seconds / 60)}m`;
  if (seconds < 86_400) return `${Math.floor(seconds / 3600)}h`;
  return `${Math.floor(seconds / 86_400)}d`;
}

/** Collision-free `(channel, semantic id)` scope for the grouping maps. */
function sessionScopeKey(channelId: string, semanticKey: string): string {
  return encodeStructuredKey(
    "coding-session-umbrella-scope/v1",
    channelId,
    semanticKey,
  );
}
