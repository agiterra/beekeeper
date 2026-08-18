import {
  type CodingSessionUmbrellaCreateObservation,
  groupCodingSessionCatalog,
} from "./codingSessionUmbrellaModel";
import type {
  CodingSessionCatalogRecord,
  CodingSessionCatalogSnapshot,
  CodingSessionExecution,
  CodingSessionUmbrellaRecord,
  CodingSessionWorkspaceStatus,
} from "./codingSessionTypes";

export type CodingSessionWorkspaceResolution =
  | { kind: "loading" }
  | {
      kind: "ready";
      session: CodingSessionCatalogRecord;
      /**
       * The umbrella the routed generation belongs to. An umbrella of one is
       * the pre-Step-4 shape; the workspace renders exactly today's
       * single-session tree for it.
       */
      umbrella: CodingSessionUmbrellaRecord;
      /** The member execution whose generation the route addresses. */
      focusedExecution: CodingSessionExecution;
    }
  | {
      kind: "untrusted";
      description: string;
    }
  | {
      kind: "missing";
      description: string;
    };

export function resolveCodingSessionWorkspace(input: {
  catalog: CodingSessionCatalogSnapshot;
  generationId: string;
}): CodingSessionWorkspaceResolution {
  const exact = input.catalog.entries.find(
    (entry) => entry.generationId === input.generationId,
  );
  // The catalog carries the channel's receipt-joined create observations when
  // it collected any; without them founder and operator resolve to null and
  // every surface stays observer-grade rather than gated.
  const creates = input.catalog.creates ?? [];
  if (exact) {
    // Every catalog record groups into exactly one umbrella, so the routed
    // record always resolves; the single-record fallback only guards against
    // a grouping bug ever hiding a session the flat lookup already found.
    const grouped =
      resolveUmbrellaForGeneration(
        input.catalog.entries,
        input.generationId,
        creates,
      ) ?? resolveUmbrellaForGeneration([exact], input.generationId, creates);
    if (grouped) {
      return {
        kind: "ready",
        session: resolveLiveGenerationRecord(exact, grouped.execution),
        umbrella: grouped.umbrella,
        focusedExecution: grouped.execution,
      };
    }
  }
  if (input.catalog.isLoading) {
    return { kind: "loading" };
  }
  if (input.catalog.authorityErrorMessage) {
    return {
      kind: "untrusted",
      description: input.catalog.authorityErrorMessage,
    };
  }
  return {
    kind: "missing",
    description:
      input.catalog.errorMessage ??
      "This exact coding-session generation is not available in the relay catalog.",
  };
}

/**
 * Follow a resumed execution off its dead generation, and nothing else.
 *
 * A resume mints the NEXT generation: the routed one keeps its immutable
 * `disconnected` metadata forever, so a route left pointing at it shows a
 * banner that can never clear. Receipt-driven navigation is the primary cure
 * (see `useCodingSessionResumeSettle`); this is the self-heal for when it is
 * missed — a dropped receipt, or an app restart mid-resume.
 *
 * Only a `disconnected` prior generation heals. Every other collapsed
 * generation stays exactly where the link pointed, because history deep links
 * into an ended or interrupted generation are a person reading the past, not
 * a stale route.
 */
function resolveLiveGenerationRecord(
  routed: CodingSessionCatalogRecord,
  execution: CodingSessionExecution,
): CodingSessionCatalogRecord {
  if (routed.generationId === execution.activeGeneration.generationId) {
    return routed;
  }
  if (routed.status !== "disconnected") return routed;
  return execution.activeGeneration;
}

/**
 * Locate the umbrella and member execution containing a routed generation.
 *
 * Prior generations count as membership too: routing to a collapsed earlier
 * generation still resolves the same umbrella, so history deep links keep the
 * surrounding session context.
 */
export function resolveUmbrellaForGeneration(
  entries: readonly CodingSessionCatalogRecord[],
  generationId: string,
  creates: readonly CodingSessionUmbrellaCreateObservation[] = [],
): {
  umbrella: CodingSessionUmbrellaRecord;
  execution: CodingSessionExecution;
} | null {
  for (const umbrella of groupCodingSessionCatalog(entries, creates)) {
    for (const execution of umbrella.executions) {
      const owns =
        execution.activeGeneration.generationId === generationId ||
        execution.priorGenerations.some(
          (record) => record.generationId === generationId,
        );
      if (owns) return { umbrella, execution };
    }
  }
  return null;
}

const WORKING_STATUSES = new Set([
  "busy",
  "running",
  "streaming",
  "thinking",
  "working",
]);
const IDLE_STATUSES = new Set([
  "cancelled",
  "completed",
  "done",
  "failed",
  "idle",
  "interrupted",
  "stopped",
]);

/**
 * Map the provider's wire status (44223 metadata) onto the header's honest
 * states. The single wire→header mapping, shared by the umbrella header and
 * the transcript-first fallback below.
 */
export function codingSessionWireWorkspaceStatus(
  status: CodingSessionCatalogRecord["status"] | undefined,
): CodingSessionWorkspaceStatus {
  if (status === "running" || status === "starting") {
    return { kind: "working", label: "Working" };
  }
  if (status === "stopped") {
    return { kind: "ended", label: "Ended" };
  }
  if (status === "disconnected") {
    return {
      kind: "unknown",
      label: "Disconnected",
      attention: "disconnected",
    };
  }
  if (status === "failed") {
    return { kind: "unknown", label: "Needs attention", attention: "failed" };
  }
  if (status === undefined || status === "unknown") {
    return { kind: "unknown", label: "Status unknown" };
  }
  return { kind: "idle", label: "Idle" };
}

/** A lifecycle item that closes a turn. */
function isTurnTerminator(
  item: CodingSessionCatalogRecord["transcript"][number],
): boolean {
  return (
    item.type === "lifecycle" &&
    (item.title === "Turn result" || item.title === "Interrupted")
  );
}

export function deriveCodingSessionWorkspaceStatus(
  transcript: CodingSessionCatalogRecord["transcript"],
  lifecycleStatus?: CodingSessionCatalogRecord["status"],
  statusAt?: CodingSessionCatalogRecord["statusAt"],
): CodingSessionWorkspaceStatus {
  // A signed lifecycle status outranks anything the transcript implies. The
  // provider's crash recovery synthesizes a terminal "Turn result" item, so a
  // disconnected execution whose transcript ends in one would otherwise read
  // "Idle" in the header while the composer offers Reconnect underneath it —
  // three surfaces, three answers, for one signed fact.
  if (lifecycleStatus === "stopped") {
    return { kind: "ended", label: "Ended" };
  }
  // A terminal-bad wire status outranks anything the transcript implies,
  // regardless of timestamps: the provider's crash recovery synthesizes a
  // "Turn result" item in the same instant it publishes `disconnected`, so a
  // freshness comparison alone can still read Idle off the synthetic tail.
  if (lifecycleStatus === "disconnected" || lifecycleStatus === "failed") {
    return codingSessionWireWorkspaceStatus(lifecycleStatus);
  }
  const newest = transcript[transcript.length - 1];
  // Wire freshness: when the newest 44223 metadata is NEWER than the newest
  // transcript item, the metadata is the later word. TurnStarted publishes
  // `running` before any transcript item of that turn exists, and a provider
  // that died mid-turn reports `disconnected`/`failed` the same way — both
  // states the transcript alone can never show.
  if (
    statusAt !== null &&
    statusAt !== undefined &&
    lifecycleStatus !== undefined &&
    (newest === undefined || statusAt > Date.parse(newest.timestamp))
  ) {
    return codingSessionWireWorkspaceStatus(lifecycleStatus);
  }
  // Open-turn test: normal turns emit NO lifecycle "Status" items — a turn
  // is user/assistant/tool items closed by one "Turn result". So while a
  // turn streams, the newest LIFECYCLE item is still the previous turn's
  // terminator and a lifecycle-only scan would report Idle forever. A newest
  // item that is not itself a terminator and belongs to a different turn
  // than the last terminator (or precedes any terminator at all) means a
  // turn is in flight.
  if (newest !== undefined && !isTurnTerminator(newest)) {
    let lastTerminator:
      | CodingSessionCatalogRecord["transcript"][number]
      | null = null;
    for (let index = transcript.length - 1; index >= 0; index -= 1) {
      if (isTurnTerminator(transcript[index])) {
        lastTerminator = transcript[index];
        break;
      }
    }
    const newestTurnId = newest.turnId ?? null;
    if (
      lastTerminator === null ||
      (newestTurnId !== null &&
        newestTurnId !== (lastTerminator.turnId ?? null))
    ) {
      return { kind: "working", label: "Working" };
    }
  }
  for (let index = transcript.length - 1; index >= 0; index -= 1) {
    const item = transcript[index];
    if (item.type !== "lifecycle") continue;

    if (item.title === "Turn result" || item.title === "Interrupted") {
      return { kind: "idle", label: "Idle" };
    }
    if (item.title !== "Status") continue;

    const normalized = item.text.trim().toLowerCase();
    if (WORKING_STATUSES.has(normalized)) {
      return { kind: "working", label: "Working" };
    }
    if (IDLE_STATUSES.has(normalized)) {
      return { kind: "idle", label: "Idle" };
    }
  }
  // The transcript said nothing decisive — fall back to the provider's
  // signed metadata status (published `idle`/`running` at create time).
  return codingSessionWireWorkspaceStatus(lifecycleStatus);
}
