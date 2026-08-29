import {
  type CodingSessionUmbrellaCreateObservation,
  groupCodingSessionCatalog,
} from "./codingSessionUmbrellaModel";
import { formatCoordinationAge } from "@/shared/coordination/sessionCoordinationFormat";
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

// Routing predicate lives beside the umbrella model so the participant roster
// and the workspace branch cannot drift apart; re-exported for the surface.
export { umbrellaHasCollapsedHistory } from "./codingSessionUmbrellaModel";

/**
 * The umbrella header's context label.
 *
 * Counting executions is only truthful when there is more than one; a resumed
 * single-execution umbrella would read "1 executions" while hiding the fact
 * that actually explains the view — that earlier generations are folded into
 * it. Name whichever of the two this umbrella is.
 */
export function codingSessionUmbrellaGenerationLabel(
  umbrella: CodingSessionUmbrellaRecord,
): string {
  if (umbrella.executions.length > 1) {
    return `${umbrella.executions.length} executions`;
  }
  const execution = umbrella.executions[0];
  const priorCount = execution?.priorGenerations.length ?? 0;
  if (!execution || priorCount === 0) return "";
  const generation = execution.activeGeneration.commandTarget?.generation;
  const current =
    generation === undefined ? "resumed" : `generation ${generation}`;
  return `${current} · ${priorCount} earlier`;
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
  if (status === "waiting_for_input") {
    return { kind: "waiting", label: "Waiting" };
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

/**
 * Whether coordination proves a provider can answer for this generation.
 *
 * `known: false` is the honest default and covers three different things —
 * the coordination read has not arrived, it was partial, or it never saw this
 * generation. None of them prove absence, so none of them may demote a status.
 */
export type CodingSessionProviderReachability =
  | { known: false }
  | { known: true; reachable: boolean };

/**
 * Demote a live-sounding status to history when nothing can answer for it.
 *
 * The lease is the only fact in this app that establishes reachability; a
 * 44223 status only establishes what the provider last said. `stopped` and the
 * two attention states already describe the execution correctly on their own
 * and are left alone — replacing "Disconnected" with "No provider answering"
 * would lose the more specific signed fact.
 *
 * `waiting` is demoted exactly as `working` is: reachability outranks stage
 * (SURFACES §2a). Telling an operator that a dead seat is waiting on them
 * invites them to type into something nobody will read.
 */
function demoteUnreachable(
  status: CodingSessionWorkspaceStatus,
  reachability: CodingSessionProviderReachability | undefined,
  statusAt: CodingSessionCatalogRecord["statusAt"] | undefined,
  nowMs: number,
): CodingSessionWorkspaceStatus {
  if (
    reachability === undefined ||
    !reachability.known ||
    reachability.reachable
  ) {
    return status;
  }
  if (status.kind === "ended" || status.kind === "unknown") return status;
  const ageSeconds =
    statusAt === null || statusAt === undefined
      ? null
      : Math.max(0, Math.floor((nowMs - statusAt) / 1_000));
  return {
    kind: "unknown",
    label: "No provider answering",
    attention: "unreachable",
    lastReported: { label: status.label, ageSeconds },
  };
}

/**
 * The clause that turns a demoted status back into a complete sentence.
 *
 * `null` for every status that still describes the present. When a status has
 * been demoted, the last thing the provider said is not thrown away — it is
 * shown *as history*, with its age, which is the distinction Agent Progress
 * already draws (`agentLaneReportedText`) and the one this header lacked.
 */
export function codingSessionWorkspaceStatusDetail(
  status: CodingSessionWorkspaceStatus,
): string | null {
  if (status.kind !== "unknown" || status.lastReported === undefined) {
    return null;
  }
  const { label, ageSeconds } = status.lastReported;
  if (ageSeconds === null) return `last reported ${label}`;
  const age = formatCoordinationAge(ageSeconds);
  // The shared formatter says "just now" for anything under a minute, which
  // does not take an "ago" — a provider that died seconds after its last
  // report is the ordinary case here, not an edge one.
  return age === "just now"
    ? `last reported ${label} just now`
    : `last reported ${label} ${age} ago`;
}

/**
 * **W1 — the one liveness word for an execution.** Every surface calls this.
 *
 * Its source is the provider's signed 44223 status, demoted by the ephemeral
 * lease. The transcript is never the source: it may *narrow* a seat the wire
 * already says is live, and it may *settle* one back to rest when the
 * provider signed a turn terminator — but it may never raise a resting signed
 * status to `working`. That promotion is what printed `live` over an
 * execution whose signed status was `completed` while the rail beside it read
 * `All idle` (SURFACES §15(b), WALK-2026-08-29 finding 2), and it is exactly
 * the shape a killed seat leaves behind: a resting last status, no
 * terminator, a transcript that looks open.
 */
export function deriveCodingSessionWorkspaceStatus(
  transcript: CodingSessionCatalogRecord["transcript"],
  lifecycleStatus?: CodingSessionCatalogRecord["status"],
  statusAt?: CodingSessionCatalogRecord["statusAt"],
  reachability?: CodingSessionProviderReachability,
  nowMs: number = Date.now(),
): CodingSessionWorkspaceStatus {
  return demoteUnreachable(
    deriveReportedWorkspaceStatus(transcript, lifecycleStatus, statusAt),
    reachability,
    statusAt,
    nowMs,
  );
}

/**
 * {@link deriveCodingSessionWorkspaceStatus} for one member execution.
 *
 * The rail, the disposition strip and the rail's footer tally all resolve a
 * seat's status through this, so the three of them cannot answer W1
 * differently for the same execution in the same window — which is what walk
 * findings 1 and 2 both were.
 */
export function deriveCodingSessionExecutionStatus(
  execution: CodingSessionExecution,
  reachability?: CodingSessionProviderReachability,
  nowMs: number = Date.now(),
): CodingSessionWorkspaceStatus {
  const record = execution.activeGeneration;
  return deriveCodingSessionWorkspaceStatus(
    record.transcript,
    record.status,
    record.statusAt,
    reachability,
    nowMs,
  );
}

/** The reported status alone — what the transcript and 44223 metadata say. */
function deriveReportedWorkspaceStatus(
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
  const signed = codingSessionWireWorkspaceStatus(lifecycleStatus);
  // Two different gates, because the transcript holds two different kinds of
  // thing. A lifecycle "Status" row is something the provider *signed*, so it
  // stands whenever the 44223 status makes no claim of its own. The open-turn
  // test is an *inference* drawn from item ordering, so it may only narrow a
  // seat the wire already calls live — never raise a resting one. That
  // promotion is walk finding 2: `live` printed over a signed `completed`.
  const wireMakesNoClaim =
    signed.kind === "unknown" && signed.attention === undefined;
  const mayInferWorking = signed.kind === "working";
  const mayReadSignedWorking = mayInferWorking || wireMakesNoClaim;
  if (turnInFlight(transcript, newest)) {
    if (mayInferWorking) return { kind: "working", label: "Working" };
    return signed;
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
      return mayReadSignedWorking
        ? { kind: "working", label: "Working" }
        : signed;
    }
    if (IDLE_STATUSES.has(normalized)) {
      return { kind: "idle", label: "Idle" };
    }
  }
  // The transcript said nothing decisive — fall back to the provider's
  // signed metadata status (published `idle`/`running` at create time).
  return signed;
}

/**
 * Whether the newest transcript items describe a turn still in flight.
 *
 * Normal turns emit NO lifecycle "Status" items — a turn is
 * user/assistant/tool items closed by one "Turn result". So while a turn
 * streams, the newest LIFECYCLE item is still the previous turn's terminator
 * and a lifecycle-only scan would report Idle forever. A newest item that is
 * not itself a terminator and belongs to a different turn than the last
 * terminator (or precedes any terminator at all) means a turn is in flight.
 *
 * Session-scoped lifecycle facts (continuity disclosures, and any other
 * status the provider publishes with no turn) carry no `turnId`. They are not
 * turn activity and must never be read as one: `session_fresh` is the first
 * item a brand-new session ever has, and without this guard its presence
 * alone — a non-terminator with no terminator before it — reads as a turn
 * streaming forever.
 *
 * This is an *inference*, not a signed statement, which is why its caller may
 * only use it to narrow a status the wire already established.
 */
function turnInFlight(
  transcript: CodingSessionCatalogRecord["transcript"],
  newest: CodingSessionCatalogRecord["transcript"][number] | undefined,
): boolean {
  if (
    newest === undefined ||
    isTurnTerminator(newest) ||
    (newest.turnId ?? null) === null
  ) {
    return false;
  }
  let lastTerminator: CodingSessionCatalogRecord["transcript"][number] | null =
    null;
  for (let index = transcript.length - 1; index >= 0; index -= 1) {
    if (isTurnTerminator(transcript[index])) {
      lastTerminator = transcript[index];
      break;
    }
  }
  const newestTurnId = newest.turnId ?? null;
  return (
    lastTerminator === null ||
    (newestTurnId !== null && newestTurnId !== (lastTerminator.turnId ?? null))
  );
}
