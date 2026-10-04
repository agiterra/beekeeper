import { isCompletedCodingSessionTurnBlock } from "@/features/coding-sessions/lib/codingSessionHandoff";
import type {
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
 * - a later block of the same execution belongs to another turn or another
 *   generation;
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
  const later = new Map<string, LaterBlocks>();
  for (let index = entries.length - 1; index >= 0; index -= 1) {
    const block = entries[index];
    if (block?.kind !== "turn-block") continue;
    let seen = later.get(block.executionKey);
    if (!seen) {
      seen = {
        completedTurns: new Set(),
        generations: new Set(),
        turnsByGeneration: new Map(),
      };
      later.set(block.executionKey, seen);
    }
    statuses.set(
      codingSessionUmbrellaEntryKey(block),
      restingStatusOf(umbrella, block, seen),
    );
    recordLaterBlock(seen, block);
  }
  return statuses;
}

type LaterBlocks = {
  completedTurns: Set<string>;
  generations: Set<string>;
  turnsByGeneration: Map<string, Set<string>>;
};

function restingStatusOf(
  umbrella: Pick<CodingSessionUmbrellaRecord, "executions">,
  block: CodingSessionUmbrellaTurnBlock,
  later: LaterBlocks,
): CodingSessionTurnRestingStatus {
  if (isCompletedCodingSessionTurnBlock(block)) return "stopped";
  const otherGeneration =
    later.generations.size > 1 ||
    (later.generations.size === 1 &&
      !later.generations.has(block.generationId));
  if (otherGeneration) return "stopped";
  if (block.turnId !== null) {
    const turns = later.turnsByGeneration.get(block.generationId);
    if (turns && (turns.size > 1 || !turns.has(block.turnId))) return "stopped";
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
  let turns = later.turnsByGeneration.get(block.generationId);
  if (!turns) {
    turns = new Set();
    later.turnsByGeneration.set(block.generationId, turns);
  }
  turns.add(block.turnId);
  if (isCompletedCodingSessionTurnBlock(block)) {
    later.completedTurns.add(turnKey(block.generationId, block.turnId));
  }
}

function turnKey(generationId: string, turnId: string): string {
  return JSON.stringify([generationId, turnId]);
}
