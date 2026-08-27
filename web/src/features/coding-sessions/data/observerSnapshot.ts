/**
 * The snapshot the browser observer hands its surfaces.
 *
 * Everything here is pure: it takes the facts the store proved and shapes them
 * for rendering. The read rules themselves (authority, founder, status fold,
 * reachability) all live in the domain — this file adds no judgement of its
 * own, only the per-execution transcript blocks (D9) and the connection facts
 * a surface has to disclose (D10).
 */
import {
  buildCodingSessionObserverSnapshot,
  type CodingSessionExecution,
  type CodingSessionObserverFacts,
  type CodingSessionReachabilityReport,
  type CodingSessionTranscriptBlock,
  type CodingSessionUmbrella,
  orderCodingSessionTranscriptBlocks,
  type ProjectedTranscriptItem,
} from "../domain/index.ts";

/** What the relay socket is doing, as a surface may state it. */
export type CodingSessionObserverConnection =
  /** Nothing has been started yet. */
  "idle" | "connecting" | "open" | "error";

/** Tallies a surface discloses rather than hides. */
export type CodingSessionObserverCounts = {
  sessions: number;
  executions: number;
  /** Events that failed to decode; each one rendered nothing. */
  malformed: number;
  /** Events whose signature did not verify on this device. */
  invalidSignature: number;
  /** Distinct payloads at one semantic key; each pair rendered nothing. */
  conflicts: number;
};

/** Everything a coding-session surface renders for one channel. */
export type ChannelSessionObserverSnapshot = {
  /** Umbrella sessions, newest activity first (D7). */
  sessions: CodingSessionUmbrella[];
  /** Every execution across every session, in session order. */
  executions: CodingSessionExecution[];
  /**
   * One transcript block per execution, keyed by `execution.executionKey`.
   * Blocks interleave; the items inside one never do (D9).
   */
  transcriptBlocksByExecution: Map<string, CodingSessionTranscriptBlock>;
  /** The same blocks, ordered and grouped by `umbrella.umbrellaKey`. */
  transcriptBlocksBySession: Map<string, CodingSessionTranscriptBlock[]>;
  /** Reachability per generation id (D8). */
  reachabilityByGenerationId: Map<string, CodingSessionReachabilityReport>;
  counts: CodingSessionObserverCounts;
  /** True when a history page came back full at its 1000-event limit (D10). */
  truncatedAt1000: boolean;
  /** Always true; stated as data so a future regression is visible (W3). */
  signaturesVerified: boolean;
  /** True once the first history read finished, whatever it found. */
  historyRead: boolean;
  /** False until a lease read completed — see D8. */
  leasesRead: boolean;
  connection: CodingSessionObserverConnection;
  /** The last relay error, verbatim, or null. */
  lastError: string | null;
};

/** Inputs the snapshot cannot derive from the facts alone. */
export type ChannelSessionSnapshotOptions = {
  /** Wall clock in ms, read once per rebuild. */
  nowMs: number;
  leasesRead: boolean;
  historyRead: boolean;
  truncatedAt1000: boolean;
  connection: CodingSessionObserverConnection;
  lastError: string | null;
};

/** The snapshot a surface renders before anything has been read. */
export function createEmptyChannelSessionSnapshot(
  connection: CodingSessionObserverConnection = "idle",
  lastError: string | null = null,
): ChannelSessionObserverSnapshot {
  return {
    sessions: [],
    executions: [],
    transcriptBlocksByExecution: new Map(),
    transcriptBlocksBySession: new Map(),
    reachabilityByGenerationId: new Map(),
    counts: {
      sessions: 0,
      executions: 0,
      malformed: 0,
      invalidSignature: 0,
      conflicts: 0,
    },
    truncatedAt1000: false,
    signaturesVerified: true,
    historyRead: false,
    leasesRead: false,
    connection,
    lastError,
  };
}

/** Fold proven facts into what the channel's session surfaces render. */
export function buildChannelSessionSnapshot(
  facts: CodingSessionObserverFacts,
  options: ChannelSessionSnapshotOptions,
): ChannelSessionObserverSnapshot {
  const domain = buildCodingSessionObserverSnapshot(facts, {
    nowMs: options.nowMs,
    leasesRead: options.leasesRead,
    historyTruncated: options.truncatedAt1000,
  });
  const executions: CodingSessionExecution[] = [];
  const transcriptBlocksByExecution = new Map<
    string,
    CodingSessionTranscriptBlock
  >();
  const transcriptBlocksBySession = new Map<
    string,
    CodingSessionTranscriptBlock[]
  >();

  for (const umbrella of domain.umbrellas) {
    const blocks: CodingSessionTranscriptBlock[] = [];
    for (const execution of umbrella.executions) {
      executions.push(execution);
      const block = buildExecutionBlock(execution);
      transcriptBlocksByExecution.set(execution.executionKey, block);
      blocks.push(block);
    }
    transcriptBlocksBySession.set(
      umbrella.umbrellaKey,
      orderCodingSessionTranscriptBlocks(blocks),
    );
  }

  return {
    sessions: domain.umbrellas,
    executions,
    transcriptBlocksByExecution,
    transcriptBlocksBySession,
    reachabilityByGenerationId: domain.reachabilityByGenerationId,
    counts: {
      sessions: domain.umbrellas.length,
      executions: executions.length,
      malformed: domain.malformedCount,
      invalidSignature: domain.invalidSignatureCount,
      conflicts: domain.conflictCount,
    },
    truncatedAt1000: domain.historyTruncated,
    signaturesVerified: domain.signaturesVerified,
    historyRead: options.historyRead,
    leasesRead: options.leasesRead,
    connection: options.connection,
    lastError: options.lastError,
  };
}

/**
 * One execution's rows across every generation it has had.
 *
 * Generations concatenate in order rather than merge by `eventSeq`: a resume
 * mints generation+1 and resets `eventSeq` to zero, so sorting the union by
 * sequence would shuffle a resumed session's second life into its first.
 */
function buildExecutionBlock(
  execution: CodingSessionExecution,
): CodingSessionTranscriptBlock {
  const generations = [
    ...execution.priorGenerations,
    execution.activeGeneration,
  ];
  const items: ProjectedTranscriptItem[] = [];
  for (const generation of generations) {
    items.push(...generation.transcript);
  }
  const startedAt =
    items.length > 0
      ? items[0].timestamp
      : Math.min(...generations.map((generation) => generation.lastEventAt));
  return {
    blockKey: execution.executionKey,
    label: execution.label,
    items,
    startedAt,
  };
}

/**
 * Find one session by the reference a URL carries.
 *
 * Matches the `sessionRef` first, then the umbrella key, so a session with no
 * `sessionRef` (an umbrella of one) is still addressable.
 */
export function selectChannelSession(
  snapshot: ChannelSessionObserverSnapshot,
  sessionRef: string,
): CodingSessionUmbrella | null {
  return (
    snapshot.sessions.find((session) => session.sessionRef === sessionRef) ??
    snapshot.sessions.find((session) => session.umbrellaKey === sessionRef) ??
    null
  );
}

/** The ordered transcript blocks for one session, or an empty list. */
export function selectSessionTranscriptBlocks(
  snapshot: ChannelSessionObserverSnapshot,
  umbrella: CodingSessionUmbrella,
): CodingSessionTranscriptBlock[] {
  return snapshot.transcriptBlocksBySession.get(umbrella.umbrellaKey) ?? [];
}
