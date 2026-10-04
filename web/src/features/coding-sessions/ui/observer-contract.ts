/**
 * The seam between the relay reader (lane W2) and the observer UI (lane W3).
 *
 * The UI never talks to a socket. It renders exactly one value — a
 * {@link CodingSessionObserverView} — and every screen in this feature is a
 * pure function of it. That keeps the honest-rendering rules (D5 authority,
 * D8 reachability, D10 truncation) testable without a relay, and it means a
 * reader regression shows up as a wrong *view*, not as a wrong screen.
 *
 * Everything in this module is pure: no React, no fetch, no WebSocket. The
 * React binding lives in `./useCodingSessionObserver.ts`.
 *
 * Intra-feature imports use explicit `.ts` extensions so `node --test`
 * resolves them without a loader — keep that when adding files.
 */
import {
  type CodingSessionExecution,
  type CodingSessionGenerationRecord,
  type CodingSessionObserverSnapshot,
  type CodingSessionReachabilityReport,
  type CodingSessionTranscriptBlock,
  type CodingSessionUmbrella,
  generationExecutionLabel,
  orderCodingSessionTranscriptBlocks,
} from "../domain/index.ts";
import { codingSessionGenerationTranscript } from "../domain/transcriptProseJoin.ts";

/** Where the live relay subscription stands, as the UI is allowed to say it. */
export type CodingSessionObserverConnection =
  /** Nothing has been asked for yet (no channel, or the surface is closed). */
  | "idle"
  /** The first history read is in flight. */
  | "connecting"
  /** History is in and the live subscription is open. */
  | "live"
  /** The socket dropped; the reader is replaying from `lastSeen - 5s`. */
  | "reconnecting"
  /** The reader gave up; `lastError` says why. */
  | "closed";

/** Facts the reader could not use. Rendered as counts, never as content. */
export type CodingSessionObserverCounts = {
  malformed: number;
  invalidSignature: number;
  conflict: number;
};

/**
 * The whole value the observer UI renders.
 *
 * `refresh` is the only thing on it that is not data: the list and detail
 * screens expose it as a "Retry"/"Refresh" button, and it must be safe to
 * call at any time (including while `connection === "connecting"`).
 */
export type CodingSessionObserverView = {
  /** The channel these facts were read from, or null when the repo has none. */
  channelId: string | null;
  /** Umbrella sessions, newest activity first. */
  sessions: readonly CodingSessionUmbrella[];
  /** Every execution across every session, indexed by `executionKey`. */
  executions: ReadonlyMap<string, CodingSessionExecution>;
  /** Ordered transcript blocks per `executionKey` (D9: blocks interleave). */
  transcriptBlocksByExecution: ReadonlyMap<
    string,
    readonly CodingSessionTranscriptBlock[]
  >;
  /** D8 reachability per generation id. */
  reachabilityByGenerationId: ReadonlyMap<
    string,
    CodingSessionReachabilityReport
  >;
  counts: CodingSessionObserverCounts;
  /**
   * Always true today — every fact is signature-checked on this device before
   * it reaches the fold. Carried as data so a future regression is visible on
   * screen instead of silent.
   */
  signaturesVerified: boolean;
  /** True when a history page came back full at the 1000-event limit (D10). */
  truncatedAt1000: boolean;
  /** False until the first history read has returned; drives the skeleton. */
  historyLoaded: boolean;
  connection: CodingSessionObserverConnection;
  lastError: string | null;
  refresh: () => void;
};

/** What lane W2's hook must be: a channel in, a rendered view out. */
export type CodingSessionObserverSource = (
  channelId: string | null,
) => CodingSessionObserverView;

const NO_EXECUTIONS: ReadonlyMap<string, CodingSessionExecution> = new Map();
const NO_BLOCKS: ReadonlyMap<string, readonly CodingSessionTranscriptBlock[]> =
  new Map();
const NO_REACHABILITY: ReadonlyMap<string, CodingSessionReachabilityReport> =
  new Map();

function noop(): void {
  // The idle view has nothing to refresh.
}

/**
 * The view for "there is nothing to read here".
 *
 * Used for a repo with no session channel, and as the value of the seam
 * before any reader is bound. It reports `signaturesVerified: true` because
 * zero facts were rendered — nothing unverified reached the screen.
 */
export function emptyCodingSessionObserverView(
  overrides: Partial<CodingSessionObserverView> = {},
): CodingSessionObserverView {
  return {
    channelId: null,
    sessions: [],
    executions: NO_EXECUTIONS,
    transcriptBlocksByExecution: NO_BLOCKS,
    reachabilityByGenerationId: NO_REACHABILITY,
    counts: { malformed: 0, invalidSignature: 0, conflict: 0 },
    signaturesVerified: true,
    truncatedAt1000: false,
    historyLoaded: false,
    connection: "idle",
    lastError: null,
    refresh: noop,
    ...overrides,
  };
}

/** Generations of one execution, oldest first, active last. */
function executionGenerations(
  execution: CodingSessionExecution,
): CodingSessionGenerationRecord[] {
  return [...execution.priorGenerations, execution.activeGeneration];
}

/**
 * Group one execution's projected rows into D9 blocks.
 *
 * One block is one `(signer, target)` stream. Rows inside a block keep the
 * projection's order (eventSeq, then event id); blocks are ordered by their
 * earliest row. Two providers have no shared clock, so rows from different
 * blocks are never merged.
 */
export function buildCodingSessionTranscriptBlocks(
  execution: CodingSessionExecution,
  reachabilityByGenerationId: ReadonlyMap<
    string,
    CodingSessionReachabilityReport
  > = new Map(),
): CodingSessionTranscriptBlock[] {
  const blocks = new Map<string, CodingSessionTranscriptBlock>();
  for (const generation of executionGenerations(execution)) {
    const label = generationExecutionLabel(generation);
    const transcript = codingSessionGenerationTranscript(
      generation,
      reachabilityByGenerationId.get(generation.generationId),
    );
    for (const item of transcript) {
      const existing = blocks.get(item.blockKey);
      if (existing === undefined) {
        blocks.set(item.blockKey, {
          blockKey: item.blockKey,
          label,
          items: [item],
          startedAt: item.timestamp,
        });
        continue;
      }
      existing.items.push(item);
      if (item.timestamp < existing.startedAt) {
        existing.startedAt = item.timestamp;
      }
    }
  }
  return orderCodingSessionTranscriptBlocks([...blocks.values()]);
}

/** How the reader describes the state it is in, alongside the snapshot. */
export type CodingSessionObserverStatus = {
  channelId: string | null;
  connection: CodingSessionObserverConnection;
  lastError: string | null;
  historyLoaded: boolean;
  refresh: () => void;
};

/**
 * Turn a domain snapshot into the value the UI renders.
 *
 * This is the one function lane W2 needs: read the relay, fold with
 * `buildCodingSessionObserverSnapshot`, hand the result here, and the screens
 * light up. No UI-shaped derivation happens anywhere else.
 */
export function codingSessionObserverViewFromSnapshot(
  snapshot: CodingSessionObserverSnapshot,
  status: CodingSessionObserverStatus,
): CodingSessionObserverView {
  const executions = new Map<string, CodingSessionExecution>();
  const transcriptBlocksByExecution = new Map<
    string,
    readonly CodingSessionTranscriptBlock[]
  >();
  for (const umbrella of snapshot.umbrellas) {
    for (const execution of umbrella.executions) {
      executions.set(execution.executionKey, execution);
      transcriptBlocksByExecution.set(
        execution.executionKey,
        buildCodingSessionTranscriptBlocks(
          execution,
          snapshot.reachabilityByGenerationId,
        ),
      );
    }
  }
  const sessions = [...snapshot.umbrellas].sort(
    (left, right) =>
      right.lastEventAt - left.lastEventAt ||
      left.umbrellaKey.localeCompare(right.umbrellaKey),
  );
  return {
    channelId: status.channelId,
    sessions,
    executions,
    transcriptBlocksByExecution,
    reachabilityByGenerationId: snapshot.reachabilityByGenerationId,
    counts: {
      malformed: snapshot.malformedCount,
      invalidSignature: snapshot.invalidSignatureCount,
      conflict: snapshot.conflictCount,
    },
    signaturesVerified: snapshot.signaturesVerified,
    truncatedAt1000: snapshot.historyTruncated,
    historyLoaded: status.historyLoaded,
    connection: status.connection,
    lastError: status.lastError,
    refresh: status.refresh,
  };
}

/**
 * The route param that addresses one session.
 *
 * A governed session is addressed by its `sessionRef`; an umbrella of one has
 * no ref, so its opaque `umbrellaKey` stands in. Both are matched by
 * {@link selectCodingSessionUmbrella}.
 */
export function codingSessionRouteRef(umbrella: CodingSessionUmbrella): string {
  return umbrella.sessionRef ?? umbrella.umbrellaKey;
}

/** Resolve the session a `$sessionRef` route param names, or null. */
export function selectCodingSessionUmbrella(
  sessions: readonly CodingSessionUmbrella[],
  sessionRef: string,
): CodingSessionUmbrella | null {
  return (
    sessions.find(
      (umbrella) =>
        umbrella.sessionRef === sessionRef ||
        umbrella.umbrellaKey === sessionRef,
    ) ?? null
  );
}

/**
 * The execution whose reachability the session header speaks for.
 *
 * D8 folds one status for the umbrella, but reachability is a property of a
 * single provider's lease. The header quotes the most recently active
 * execution and the executions list shows the rest, so a second, silent
 * provider can never hide behind a first, live one.
 */
export function selectHeadlineExecution(
  umbrella: CodingSessionUmbrella,
): CodingSessionExecution | null {
  let headline: CodingSessionExecution | null = null;
  for (const execution of umbrella.executions) {
    if (
      headline === null ||
      execution.activeGeneration.lastEventAt >
        headline.activeGeneration.lastEventAt ||
      (execution.activeGeneration.lastEventAt ===
        headline.activeGeneration.lastEventAt &&
        execution.executionKey.localeCompare(headline.executionKey) < 0)
    ) {
      headline = execution;
    }
  }
  return headline;
}

/** Every block of every execution in one session, in D9 block order. */
export function selectCodingSessionTranscriptBlocks(
  view: CodingSessionObserverView,
  umbrella: CodingSessionUmbrella,
): CodingSessionTranscriptBlock[] {
  const blocks: CodingSessionTranscriptBlock[] = [];
  for (const execution of umbrella.executions) {
    const executionBlocks = view.transcriptBlocksByExecution.get(
      execution.executionKey,
    );
    if (executionBlocks !== undefined) {
      blocks.push(...executionBlocks);
    }
  }
  return orderCodingSessionTranscriptBlocks(blocks);
}

/** The D8 reachability report for a generation, defaulting to "not read yet". */
export function selectReachability(
  view: CodingSessionObserverView,
  generationId: string,
): CodingSessionReachabilityReport {
  return (
    view.reachabilityByGenerationId.get(generationId) ?? {
      reachability: "unknown",
      lastReportedStatus: null,
      lastReportedAgeSeconds: null,
    }
  );
}
