/**
 * The umbrella session's interleaved narrative: N executions' turn blocks and
 * the conversation lane, merged into one time-ordered list.
 *
 * Transcript facts from different signers NEVER merge. Each turn block is a
 * window into exactly one (signer, target) stream, its items in that stream's
 * own `eventSeq` projection order; interleaving happens *between* blocks,
 * never within them. There is no code path that can place another signer's
 * item inside a block — a block is built from a single execution's records
 * and carries its provenance on the entry.
 *
 * Ordering: blocks sort by their first item's timestamp, clamped
 * non-decreasing within each execution so two blocks of one stream can never
 * cross-order; ties break by `(signerPubkey, executionKey)` for determinism.
 * Conversation messages sort by `created_at`. Generation bumps surface as
 * lifecycle rows.
 */
import type { TranscriptItem } from "@/features/agents/ui/agentSessionTypes";
import type { CodingSessionLaneMessage } from "./codingSessionConversationLane";
import type {
  CodingSessionCatalogRecord,
  CodingSessionExecution,
  CodingSessionUmbrellaRecord,
} from "./codingSessionTypes";

/** One execution's turn block: items from exactly one (signer, target) stream. */
export type CodingSessionUmbrellaTurnBlock = {
  kind: "turn-block";
  executionKey: string;
  /** The fact-stream signer whose claim this block renders. */
  signerPubkey: string;
  /** The exact generation the items belong to. */
  generation: number;
  generationId: string;
  /** The producer's turn identity, or null for an ungrouped item. */
  turnId: string | null;
  /**
   * This block's ordinal within its own generation's stream, assigned at build
   * time. `turnId` is NOT unique — a turn split by a mid-turn ungrouped item
   * yields two blocks carrying the same turnId — and the merged timeline's
   * index shifts whenever a lane message interleaves. The ordinal is both
   * unique within the generation and stable against interleaving, which is
   * what a React key needs.
   */
  blockSeq: number;
  items: TranscriptItem[];
  timestampMs: number;
};

/** A conversation-lane message interleaved between blocks. */
export type CodingSessionUmbrellaConversationEntry = {
  kind: "conversation";
  message: CodingSessionLaneMessage;
  timestampMs: number;
};

/**
 * A system row: an execution starting a new generation, or an execution
 * joining a session that was already running.
 *
 * The join row exists because a second provider's first block would otherwise
 * simply appear, distinguishable from the founder's work only by a grey
 * provenance chip. The founding execution never gets one — a session of one
 * has nothing to announce, and a row there would be pure noise.
 */
export type CodingSessionUmbrellaLifecycleEntry = {
  kind: "lifecycle";
  executionKey: string;
  signerPubkey: string;
  event: "generation-started" | "execution-joined";
  generation: number;
  timestampMs: number;
};

export type CodingSessionUmbrellaTimelineEntry =
  | CodingSessionUmbrellaTurnBlock
  | CodingSessionUmbrellaConversationEntry
  | CodingSessionUmbrellaLifecycleEntry;

/**
 * Build the interleaved umbrella timeline.
 *
 * For an umbrella of one with no conversation messages this degenerates to
 * the single execution's blocks in stream order — flattening their items
 * reproduces exactly the transcript the single-session surface renders today.
 */
export function buildUmbrellaTimeline(
  umbrella: CodingSessionUmbrellaRecord,
  conversationMessages: readonly CodingSessionLaneMessage[] = [],
): CodingSessionUmbrellaTimelineEntry[] {
  const entries: CodingSessionUmbrellaTimelineEntry[] = [];
  // `umbrella.executions` is attach-ordered, so index 0 is the execution the
  // session began as: it joins nothing.
  for (const [index, execution] of umbrella.executions.entries()) {
    entries.push(...executionEntries(execution, index > 0));
  }
  for (const message of conversationMessages) {
    entries.push({
      kind: "conversation",
      message,
      timestampMs: message.timestampMs,
    });
  }
  // Stable sort: entries of one execution enter in stream order with
  // non-decreasing timestamps, so equal keys preserve that order and unequal
  // timestamps only ever move whole blocks relative to *other* streams.
  return entries
    .map((entry, index) => ({ entry, index }))
    .sort(
      (left, right) =>
        left.entry.timestampMs - right.entry.timestampMs ||
        entryKindRank(left.entry) - entryKindRank(right.entry) ||
        entryTieKey(left.entry).localeCompare(entryTieKey(right.entry)) ||
        left.index - right.index,
    )
    .map(({ entry }) => entry);
}

/**
 * Group one generation's projected transcript into turn blocks.
 *
 * Consecutive items sharing a non-null `turnId` form one block. An item with
 * no turn identity becomes a singleton block: ungrouped telemetry stays
 * ungrouped rather than being guessed into a neighbour's turn.
 *
 * Block timestamps are the first item's, clamped non-decreasing along the
 * stream (an unparseable timestamp pins at its predecessor) so blocks of one
 * stream can never cross-order.
 */
export function groupTranscriptIntoTurnBlocks(
  record: CodingSessionCatalogRecord,
  executionKey: string,
): CodingSessionUmbrellaTurnBlock[] {
  const signerPubkey = record.providerAuthorityPubkey ?? "";
  const generation = record.commandTarget?.generation ?? 1;
  const blocks: CodingSessionUmbrellaTurnBlock[] = [];
  let floorMs = 0;
  for (const item of record.transcript) {
    const turnId = itemTurnId(item);
    const open = blocks[blocks.length - 1];
    if (open && turnId !== null && open.turnId === turnId) {
      open.items.push(item);
      continue;
    }
    const timestampMs = clamp(itemTimestampMs(item), floorMs);
    floorMs = timestampMs;
    blocks.push({
      kind: "turn-block",
      executionKey,
      signerPubkey,
      generation,
      generationId: record.generationId,
      turnId,
      blockSeq: blocks.length,
      items: [item],
      timestampMs,
    });
  }
  return blocks;
}

/**
 * Render key for a timeline entry.
 *
 * Unique and stable in one expression: turn blocks key on
 * `(generationId, blockSeq)` — `generationId` is unique per catalog record and
 * `blockSeq` is that record's own build-time ordinal, so neither a turn split
 * across two blocks nor a lane message interleaving between them can collide
 * or shift a key. Lifecycle rows key on their execution's generation and the
 * event they announce, and conversation rows on the signed event id.
 */
export function codingSessionUmbrellaEntryKey(
  entry: CodingSessionUmbrellaTimelineEntry,
): string {
  switch (entry.kind) {
    case "turn-block":
      return `block:${entry.generationId}:${entry.blockSeq}`;
    case "lifecycle":
      return `lifecycle:${entry.event}:${entry.executionKey}:${entry.generation}`;
    default:
      return `conversation:${entry.message.eventId}`;
  }
}

// ---------------------------------------------------------------------------
// Internals
// ---------------------------------------------------------------------------

function executionEntries(
  execution: CodingSessionExecution,
  joined: boolean,
): CodingSessionUmbrellaTimelineEntry[] {
  const generations = [
    ...execution.priorGenerations,
    execution.activeGeneration,
  ];
  const entries: CodingSessionUmbrellaTimelineEntry[] = [];
  let clampMs = 0;
  for (const [index, record] of generations.entries()) {
    const blocks = groupTranscriptIntoTurnBlocks(
      record,
      execution.executionKey,
    );
    const generationStartMs = clamp(
      blocks[0]?.timestampMs ?? recordFallbackMs(record),
      clampMs,
    );
    // The join row lands on this execution's earliest generation, at the same
    // instant as its first block. Both are pushed before that block and share
    // its stream rank and tie key, so the comparator can only preserve the
    // insertion order: join, then work.
    const event =
      index === 0
        ? joined
          ? ("execution-joined" as const)
          : null
        : ("generation-started" as const);
    if (event) {
      entries.push({
        kind: "lifecycle",
        executionKey: execution.executionKey,
        signerPubkey: execution.signerPubkey,
        event,
        generation: record.commandTarget?.generation ?? index + 1,
        timestampMs: generationStartMs,
      });
      clampMs = generationStartMs;
    }
    for (const block of blocks) {
      // Clamp non-decreasing within the stream: a block whose first item
      // carries an earlier (or unparseable) timestamp than its predecessor
      // pins at the predecessor's time instead of jumping the queue.
      block.timestampMs = clamp(block.timestampMs, clampMs);
      clampMs = block.timestampMs;
      entries.push(block);
    }
  }
  return entries;
}

function clamp(timestampMs: number, floorMs: number): number {
  return Number.isFinite(timestampMs)
    ? Math.max(timestampMs, floorMs)
    : floorMs;
}

function recordFallbackMs(record: CodingSessionCatalogRecord): number {
  const parsed = Date.parse(record.lastEventAt);
  return Number.isFinite(parsed) ? parsed : 0;
}

function itemTurnId(item: TranscriptItem): string | null {
  return "turnId" in item && typeof item.turnId === "string"
    ? item.turnId
    : null;
}

function itemTimestampMs(item: TranscriptItem): number {
  const parsed = Date.parse(item.timestamp);
  return Number.isFinite(parsed) ? parsed : Number.NaN;
}

/**
 * Blocks and lifecycle rows of one stream share a rank so a timestamp tie can
 * never let the comparator reorder them against each other — their relative
 * order is fixed by insertion (stream) order. Conversation messages rank
 * after execution entries at the same instant.
 */
function entryKindRank(entry: CodingSessionUmbrellaTimelineEntry): number {
  return entry.kind === "conversation" ? 1 : 0;
}

function entryTieKey(entry: CodingSessionUmbrellaTimelineEntry): string {
  return entry.kind === "conversation"
    ? entry.message.eventId
    : `${entry.signerPubkey} ${entry.executionKey}`;
}
