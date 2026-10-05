import type { TranscriptItem } from "@/features/agents/ui/agentSessionTypes";
import { isCompletedCodingSessionTurnBlock } from "@/features/coding-sessions/lib/codingSessionHandoff";
import type {
  CodingSessionTranscriptBlock,
  CodingSessionTranscriptTurn,
  CodingSessionTurnRestingStatus,
  CodingSessionTurnSettlement,
} from "@/features/coding-sessions/lib/codingSessionTranscriptModelTypes";
import type {
  CodingSessionStatus,
  CodingSessionUmbrellaRecord,
} from "@/features/coding-sessions/lib/codingSessionTypes";
import {
  codingSessionUmbrellaEntryKey,
  type CodingSessionUmbrellaTimelineEntry,
  type CodingSessionUmbrellaTurnBlock,
} from "@/features/coding-sessions/lib/codingSessionUmbrellaTimeline";

/**
 * Whether a turn's unended tool calls are running, did not finish, or have a
 * status nobody on screen can vouch for.
 *
 * Settled is a **fact of the transcript first**: a turn that reported a
 * completion, or that a later turn followed, is over — whatever the session
 * flag says. Only a turn with neither falls back to what the caller knows
 * about the session: the turn being worked on is live, and otherwise the
 * caller's resting status decides. A caller that cannot say passes
 * `unknown`, and an unended call then reads "Status unknown" rather than a
 * spinner or a verdict.
 */
export function resolveCodingSessionTurnSettlement(
  turn: Pick<
    CodingSessionTranscriptTurn,
    "completion" | "isWorking" | "superseded"
  >,
  restingStatus: CodingSessionTurnRestingStatus,
): CodingSessionTurnSettlement {
  if (turn.completion !== null || turn.superseded) return "settled";
  if (turn.isWorking) return "live";
  if (restingStatus === "running") return "live";
  if (restingStatus === "stopped") return "settled";
  return "unknown";
}

/**
 * The resting status a provider's signed status vouches for.
 *
 * `running` is the producer still on it. Idle, completed, stopped, failed
 * and interrupted are each a definite "not running". Everything else —
 * starting, waiting for input, disconnected, unknown — says nothing about
 * whether a call already in flight ended, so it stays `unknown`.
 */
export function resolveCodingSessionExecutionRestingStatus(
  status: CodingSessionStatus,
): CodingSessionTurnRestingStatus {
  switch (status) {
    case "running":
      return "running";
    case "idle":
    case "completed":
    case "stopped":
    case "failed":
    case "interrupted":
      return "stopped";
    default:
      return "unknown";
  }
}

/**
 * What the single-session workspace hands its transcript (SV-42, SV-43): the
 * working flag for its open turn and the resting status for one nobody is
 * working on, both through {@link resolveCodingSessionExecutionRestingStatus}
 * — the map Mission's blocks read — so one signed status settles the same
 * way in both views. `failed` is stopped in each, and `starting` is live in
 * neither: a provider still starting has not begun a turn, so an unended
 * call under it reads "Status unknown" here exactly as it does in Mission.
 *
 * `workspaceStatus` is the header's word (`deriveCodingSessionWorkspaceStatus`,
 * whose `codingSessionWireWorkspaceStatus` is a label map only). The header
 * reads a signed `starting` as its own word, "Starting", so the two surfaces
 * agree: the transcript shows a live turn exactly when the header says
 * Working. The header can only narrow: a turn is worked on when it says
 * Working and the signed status does not say starting or stopped, and a status the lease demoted to
 * "No provider answering" vouches for nothing, so its calls read unknown
 * rather than live under a spinner nobody is driving. A signed `running`
 * the header no longer reads as Working is likewise unknown.
 */
export function resolveCodingSessionWorkspaceSettlement(input: {
  wireStatus: CodingSessionStatus | undefined;
  workspaceStatus: {
    kind: string;
    attention?: "disconnected" | "failed" | "unreachable";
  };
}): {
  isWorking: boolean;
  restingStatus: CodingSessionTurnRestingStatus;
} {
  if (input.workspaceStatus.attention === "unreachable") {
    return { isWorking: false, restingStatus: "unknown" };
  }
  const signed =
    input.wireStatus === undefined
      ? "unknown"
      : resolveCodingSessionExecutionRestingStatus(input.wireStatus);
  const headerWorking = input.workspaceStatus.kind === "working";
  // A signed `running` the header overrode with a newer word (a transcript
  // that settled after the metadata was published) is not evidence of a live
  // call any more, and not of a stopped one either.
  const restingStatus =
    signed === "running" && !headerWorking ? "unknown" : signed;
  const isWorking =
    headerWorking &&
    input.wireStatus !== "starting" &&
    restingStatus !== "stopped";
  return { isWorking, restingStatus };
}

/**
 * Each turn's settlement in a derived transcript model, by turn id — what
 * `CodingSessionTurn` hands its rows, for a surface outside the stream that
 * shows the same calls (the Agents panel's spawns). A call whose turn id is
 * not here sits outside any turn, where the stream's default is `live`.
 */
export function resolveCodingSessionTurnSettlementsById(
  blocks: readonly CodingSessionTranscriptBlock[],
  restingStatus: CodingSessionTurnRestingStatus,
): ReadonlyMap<string, CodingSessionTurnSettlement> {
  const settlements = new Map<string, CodingSessionTurnSettlement>();
  for (const block of blocks) {
    if (block.kind !== "turn") continue;
    settlements.set(
      block.id,
      resolveCodingSessionTurnSettlement(block, restingStatus),
    );
  }
  return settlements;
}

/**
 * The settlement of an open call in one generation's transcript, by
 * Mission's block rules (`resolveCodingSessionUmbrellaBlockRestingStatuses`)
 * at item granularity, for the Agents panel beside Mission's stream:
 *
 * - a generation that is no longer the execution's active one is over;
 * - a turn some other turn of the same transcript starts after (its last
 *   item precedes that turn's first item) is over (SV-44);
 * - otherwise the active generation's signed status decides: running is
 *   live, a definite stop is settled, anything else is unknown.
 *
 * A turn that reported its own completion is already `stopped` in the
 * transcript-only reading (`partitionCodingSessionSubagentItems`), so it
 * never reaches here as open.
 */
export function resolveCodingSessionGenerationCallSettlement(input: {
  activeGeneration: { generationId: string; status: CodingSessionStatus };
  generationId: string;
  transcript: readonly TranscriptItem[];
}): (call: Pick<TranscriptItem, "turnId">) => CodingSessionTurnSettlement {
  if (input.activeGeneration.generationId !== input.generationId) {
    return () => "settled";
  }
  const signed = resolveCodingSessionExecutionRestingStatus(
    input.activeGeneration.status,
  );
  const base: CodingSessionTurnSettlement =
    signed === "running"
      ? "live"
      : signed === "stopped"
        ? "settled"
        : "unknown";
  const lastIndexByTurn = new Map<string, number>();
  let latestStart = -1;
  input.transcript.forEach((item, index) => {
    if (!item.turnId) return;
    if (!lastIndexByTurn.has(item.turnId)) latestStart = index;
    lastIndexByTurn.set(item.turnId, index);
  });
  return (call) => {
    const last = call.turnId ? lastIndexByTurn.get(call.turnId) : undefined;
    if (last !== undefined && latestStart > last) return "settled";
    return base;
  };
}

/**
 * The resting status of every umbrella turn block, keyed by
 * `codingSessionUmbrellaEntryKey`, from the timeline's own facts.
 *
 * Pass the chronological timeline (`buildUmbrellaTimeline`), not a density's
 * projection: Brief drops items, and a later block is evidence only if it is
 * still in the list.
 *
 * A block can be a fragment of its turn — a turn split by an ungrouped item
 * yields two blocks with one `turnId` — so "a later block exists" is not on
 * its own proof the turn ended. These are:
 *
 * - the block, or a later block of the same turn, carries the completion;
 * - a later block of the same execution belongs to another generation;
 * - another turn of the same generation **starts after this turn's last
 *   block** (SV-44, the rule `findSupersededTurns` applies to the single
 *   workspace): a turn whose blocks still interleave with a later turn's — a
 *   queued prompt that opened its own turn while this one kept publishing —
 *   is still going, and "a later turn exists" alone would read its in-flight
 *   calls as "did not finish" while the workspace shows them live;
 * - the block's generation is no longer the execution's active one.
 *
 * Failing all three, the block holds the execution's latest turn and its
 * active generation's signed status decides. A block of an execution the
 * umbrella does not list reads `unknown`: nothing here vouches for it.
 */
export function resolveCodingSessionUmbrellaBlockRestingStatuses(
  umbrella: Pick<CodingSessionUmbrellaRecord, "executions">,
  entries: readonly CodingSessionUmbrellaTimelineEntry[],
): ReadonlyMap<string, CodingSessionTurnRestingStatus> {
  const statuses = new Map<string, CodingSessionTurnRestingStatus>();
  const spans = collectTurnSpans(entries);
  const later = new Map<string, LaterBlocks>();
  for (let index = entries.length - 1; index >= 0; index -= 1) {
    const block = entries[index];
    if (block?.kind !== "turn-block") continue;
    let seen = later.get(block.executionKey);
    if (!seen) {
      seen = {
        completedTurns: new Set(),
        generations: new Set(),
      };
      later.set(block.executionKey, seen);
    }
    statuses.set(
      codingSessionUmbrellaEntryKey(block),
      restingStatusOf(umbrella, block, seen, spans),
    );
    recordLaterBlock(seen, block);
  }
  return statuses;
}

type LaterBlocks = {
  completedTurns: Set<string>;
  generations: Set<string>;
};

/**
 * Where each turn sits in the chronological timeline: its last block's index
 * per turn, and the latest first-block index of any turn per execution and
 * generation. A turn ended by a later one is exactly a turn whose last block
 * comes before some turn's first block in the same generation.
 */
type TurnSpans = {
  lastIndexByTurn: Map<string, number>;
  latestStartByGeneration: Map<string, number>;
};

function collectTurnSpans(
  entries: readonly CodingSessionUmbrellaTimelineEntry[],
): TurnSpans {
  const lastIndexByTurn = new Map<string, number>();
  const latestStartByGeneration = new Map<string, number>();
  const started = new Set<string>();
  entries.forEach((block, index) => {
    if (block?.kind !== "turn-block" || block.turnId === null) return;
    const turn = spanKey(block.executionKey, block.generationId, block.turnId);
    lastIndexByTurn.set(turn, index);
    if (started.has(turn)) return;
    started.add(turn);
    const generation = spanKey(block.executionKey, block.generationId);
    latestStartByGeneration.set(
      generation,
      Math.max(latestStartByGeneration.get(generation) ?? -1, index),
    );
  });
  return { lastIndexByTurn, latestStartByGeneration };
}

function spanKey(...parts: string[]): string {
  return JSON.stringify(parts);
}

function restingStatusOf(
  umbrella: Pick<CodingSessionUmbrellaRecord, "executions">,
  block: CodingSessionUmbrellaTurnBlock,
  later: LaterBlocks,
  spans: TurnSpans,
): CodingSessionTurnRestingStatus {
  if (isCompletedCodingSessionTurnBlock(block)) return "stopped";
  const otherGeneration =
    later.generations.size > 1 ||
    (later.generations.size === 1 &&
      !later.generations.has(block.generationId));
  if (otherGeneration) return "stopped";
  if (block.turnId !== null) {
    const lastIndex = spans.lastIndexByTurn.get(
      spanKey(block.executionKey, block.generationId, block.turnId),
    );
    const latestStart = spans.latestStartByGeneration.get(
      spanKey(block.executionKey, block.generationId),
    );
    if (
      lastIndex !== undefined &&
      latestStart !== undefined &&
      latestStart > lastIndex
    ) {
      return "stopped";
    }
    if (later.completedTurns.has(turnKey(block.generationId, block.turnId))) {
      return "stopped";
    }
  }
  const execution = umbrella.executions.find(
    (candidate) => candidate.executionKey === block.executionKey,
  );
  if (!execution) return "unknown";
  if (execution.activeGeneration.generationId !== block.generationId) {
    return "stopped";
  }
  return resolveCodingSessionExecutionRestingStatus(
    execution.activeGeneration.status,
  );
}

function recordLaterBlock(
  later: LaterBlocks,
  block: CodingSessionUmbrellaTurnBlock,
): void {
  later.generations.add(block.generationId);
  if (block.turnId === null) return;
  if (isCompletedCodingSessionTurnBlock(block)) {
    later.completedTurns.add(turnKey(block.generationId, block.turnId));
  }
}

function turnKey(generationId: string, turnId: string): string {
  return JSON.stringify([generationId, turnId]);
}
