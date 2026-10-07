/**
 * SV-100: one transcript model per execution generation, shared by every view
 * that shows it.
 *
 * OWNERSHIP CONTRACT (iteration 1)
 *
 * - Unit: one catalog record = one (signer, target) generation of one
 *   execution. Its model is `deriveCodingSessionTranscriptModel` over the
 *   record's WHOLE transcript — every item available now, which is not a
 *   claim that paging has finished — with two explicit inputs: `isWorking`
 *   and `generationSuperseded`. Background tasks are derived once over the
 *   whole generation (`deriveCodingSessionBlockBackgroundTasks` with the
 *   transcript as its own block list) and handed to the model, which is the
 *   SV-91 rule moved from every block to the owner.
 * - Revision: (transcript array identity, isWorking, workingTurnId,
 *   generationSuperseded).
 *   SV-118 keeps an unchanged generation's transcript array identical, so an
 *   event in execution A cannot re-derive execution B. A changed revision
 *   derives exactly once, then `stabilizeCodingSessionTranscriptModel` keeps
 *   every semantically unchanged block's reference (entry-by-entry equality,
 *   never length or endpoints).
 * - Views select, never re-derive: an umbrella turn block (same consecutive
 *   `turnId` rule as `groupTranscriptIntoTurnBlocks`, addressed by its
 *   `blockSeq`) selects the model blocks that own its items. A model block is
 *   owned by the FIRST timeline block that holds any of its items, so a turn
 *   split by an unturned row renders once, whole, at its first part (the
 *   solo view's grouping; one turn in 121 on the Tank Loop export).
 *   Variants: `whole` (Conversation, Brief, Trace), and Mission Live's
 *   `mission-narrative` / `mission-execution` split of the same turn — the
 *   turn's facts (completion, background tasks, supersession, isWorking)
 *   come from the shared model; only which entries show differs.
 * - Lifetime: a store belongs to one mounted umbrella timeline, keyed by its
 *   scope (channel + umbrella); `retain` evicts generations the umbrella no
 *   longer holds; nothing is process-global.
 * - Immutability: published models and selections are frozen; a revision
 *   makes new objects and never edits an old one.
 */

import * as React from "react";

import type { TranscriptItem } from "@/features/agents/ui/agentSessionTypes";
import {
  codingSessionBackgroundTasksByTurnEqual,
  deriveCodingSessionBlockBackgroundTasks,
  deriveCodingSessionTranscriptModel,
  joinConsecutiveCodingSessionProse,
  stabilizeCodingSessionTranscriptModel,
} from "./codingSessionTranscriptModel";
import { deriveCodingSessionTurnFold } from "./codingSessionTranscriptModelFold";
import {
  CODING_SESSION_CONTINUITY_STATUSES,
  CODING_SESSION_CONTINUITY_TITLE,
} from "./codingSessionTranscriptItems";
import type {
  CodingSessionTranscriptBlock,
  CodingSessionTranscriptEntry,
  CodingSessionTranscriptModel,
  CodingSessionTranscriptTurn,
  CodingSessionTurnBackgroundTask,
} from "./codingSessionTranscriptModelTypes";
import type { CodingSessionCatalogRecord } from "./codingSessionTypes";
import { groupTranscriptIntoTurnBlocks } from "./codingSessionUmbrellaTimeline";

/** What one generation's model is derived from. */
export type CodingSessionExecutionModelInput = {
  record: CodingSessionCatalogRecord;
  executionKey: string;
  /**
   * The execution's working block is a turn block of THIS generation (the
   * umbrella's `resolveWorkingBlockKeys`). The model applies it to its last
   * turn, exactly as the solo workspace does.
   */
  isWorking: boolean;
  /**
   * The working block's turn id when `isWorking` (the turn the working line
   * belongs to, which with interleaved turns is not the last-appearing one);
   * null when not working.
   */
  workingTurnId: string | null;
  /** A newer generation of the same execution exists (SV-91 `unreported`). */
  generationSuperseded: boolean;
};

/** Which part of a block's turn a view shows. */
export type CodingSessionBlockSelectionVariant =
  | "whole"
  | "mission-narrative"
  | "mission-execution";

export type CodingSessionBlockSelectionOptions = {
  variant?: CodingSessionBlockSelectionVariant;
  /** Mission's rehydration-claim rule (`hidesCodingSessionRehydrationClaim`). */
  hideRehydrationClaim?: boolean;
};

/** One generation's shared interpretation, as published at one revision. */
export type CodingSessionExecutionModel = {
  readonly generationId: string;
  readonly executionKey: string;
  /** The whole generation's model (stabilized against the prior revision). */
  readonly model: CodingSessionTranscriptModel;
  /** Background tasks per turn over the whole generation (SV-91). */
  readonly backgroundTasksByTurn: ReadonlyMap<
    string,
    readonly CodingSessionTurnBackgroundTask[]
  >;
  /**
   * The model-shaped selection one umbrella turn block renders: pass it as
   * `CodingSessionTranscript`'s `model`. Same inputs → same object.
   */
  selectBlock(
    blockSeq: number,
    options?: CodingSessionBlockSelectionOptions,
  ): CodingSessionTranscriptModel;
  /** The block's first selected turn that opens with a user prompt (minimap). */
  selectMinimapTurn(blockSeq: number): CodingSessionTranscriptTurn | null;
  /** The items of the timeline block `blockSeq`, for callers that need them. */
  blockItems(blockSeq: number): readonly TranscriptItem[];
};

/** Deterministic work counters (tests and the replay benchmark). */
export type CodingSessionExecutionModelStats = {
  /** `model()` calls. */
  requests: number;
  /** Requests answered by the retained revision, no derivation. */
  hits: number;
  /** Full-generation model derivations. */
  derivations: number;
  /** `selectBlock` calls / those that returned the retained selection. */
  selections: number;
  selectionHits: number;
  /** Generations dropped by `retain` or `reset`. */
  evicted: number;
};

export interface CodingSessionExecutionModelStore {
  /** The generation's shared model at this input revision. */
  model(input: CodingSessionExecutionModelInput): CodingSessionExecutionModel;
  /** Drop every generation not named here. */
  retain(generationIds: Iterable<string>): void;
  /** Drop everything. */
  reset(): void;
  stats(): CodingSessionExecutionModelStats;
}

// ---------------------------------------------------------------------------
// Rehydration claim (Mission, finding 9)
// ---------------------------------------------------------------------------

/** The `session_rehydrated` prose, read from the map that mints it. */
const REHYDRATED_CONTINUITY_PROSE =
  CODING_SESSION_CONTINUITY_STATUSES.get("session_rehydrated") ?? "Rehydrated";

/**
 * The item test behind `hidesCodingSessionRehydrationClaim`: the provider's
 * `session_rehydrated` continuity row, matched on its title plus the exact
 * prose `CODING_SESSION_CONTINUITY_STATUSES` mints for that slug. Whether
 * Mission hides it (no prior generation on screen) is the caller's call.
 */
export function isCodingSessionRehydrationClaimItem(
  item: TranscriptItem,
): boolean {
  return (
    item.type === "lifecycle" &&
    item.title === CODING_SESSION_CONTINUITY_TITLE &&
    item.text.startsWith(REHYDRATED_CONTINUITY_PROSE)
  );
}

// ---------------------------------------------------------------------------
// Store
// ---------------------------------------------------------------------------

const EMPTY_ITEMS: readonly TranscriptItem[] = Object.freeze([]);
const EMPTY_BLOCKS: readonly CodingSessionTranscriptBlock[] = Object.freeze([]);
const NO_TASKS: readonly CodingSessionTurnBackgroundTask[] = Object.freeze([]);
const NO_CHANGED_FILES: CodingSessionTranscriptTurn["changedFiles"] =
  Object.freeze([]) as unknown as CodingSessionTranscriptTurn["changedFiles"];
const RESULT_BODY_SUFFIX = ":assistant-result";
const SETTLED_TURN_PREFIX = "settled:";

/** What makes two `model()` inputs the same revision. */
type RevisionKey = {
  transcript: readonly TranscriptItem[];
  isWorking: boolean;
  workingTurnId: string | null;
  generationSuperseded: boolean;
  executionKey: string;
  authority: string | null;
  driver: string | null;
  instanceId: string | null;
  sessionId: string | null;
  generation: number | null;
};

type TimelineBlock = {
  turnId: string | null;
  items: readonly TranscriptItem[];
};

type Ownership = {
  blocks: ReadonlyMap<number, readonly CodingSessionTranscriptBlock[]>;
  diagnostics: ReadonlyMap<number, readonly TranscriptItem[]>;
};

type Revision = {
  key: RevisionKey;
  model: CodingSessionTranscriptModel;
  backgroundTasksByTurn: ReadonlyMap<
    string,
    readonly CodingSessionTurnBackgroundTask[]
  >;
  /** Lazily grouped timeline blocks, indexed by `blockSeq`. */
  timeline: readonly TimelineBlock[] | null;
  published: CodingSessionExecutionModel;
};

type GenerationState = {
  revision: Revision | null;
  /** The latest selection per (blockSeq, variant, hide), across revisions. */
  selections: Map<string, CodingSessionTranscriptModel>;
  /** Variant turns rebuilt from one source turn, reused while it is. */
  variantTurns: WeakMap<
    CodingSessionTranscriptTurn,
    Map<string, CodingSessionTranscriptTurn | null>
  >;
};

type Counters = CodingSessionExecutionModelStats;

export function createCodingSessionExecutionModelStore(): CodingSessionExecutionModelStore {
  const generations = new Map<string, GenerationState>();
  const counters: Counters = {
    requests: 0,
    hits: 0,
    derivations: 0,
    selections: 0,
    selectionHits: 0,
    evicted: 0,
  };
  return {
    model(input) {
      counters.requests += 1;
      const generationId = input.record.generationId;
      let state = generations.get(generationId);
      if (!state) {
        state = {
          revision: null,
          selections: new Map(),
          variantTurns: new WeakMap(),
        };
        generations.set(generationId, state);
      }
      const key = revisionKeyOf(input);
      const prior = state.revision;
      if (prior && revisionKeysEqual(prior.key, key)) {
        counters.hits += 1;
        return prior.published;
      }
      counters.derivations += 1;
      const revision = deriveRevision(state, input, key, counters);
      state.revision = revision;
      return revision.published;
    },
    retain(generationIds) {
      const keep = new Set(generationIds);
      for (const generationId of [...generations.keys()]) {
        if (keep.has(generationId)) continue;
        generations.delete(generationId);
        counters.evicted += 1;
      }
    },
    reset() {
      counters.evicted += generations.size;
      generations.clear();
    },
    stats() {
      return { ...counters };
    },
  };
}

/**
 * The store for one mounted umbrella timeline. A changed `scopeKey` (another
 * channel or umbrella) starts a fresh store; nothing is process-global.
 */
export function useCodingSessionExecutionModelStore(
  scopeKey: string,
): CodingSessionExecutionModelStore {
  const held = React.useRef<{
    scopeKey: string;
    store: CodingSessionExecutionModelStore;
  } | null>(null);
  // Render-time lazy init, keyed, as `useRetainedCodingSessionCatalogProjection`
  // does: a discarded render can at worst mint a store the next one replaces.
  if (held.current === null || held.current.scopeKey !== scopeKey) {
    held.current = {
      scopeKey,
      store: createCodingSessionExecutionModelStore(),
    };
  }
  return held.current.store;
}

function revisionKeyOf(input: CodingSessionExecutionModelInput): RevisionKey {
  const target = input.record.commandTarget;
  return {
    transcript: input.record.transcript,
    isWorking: input.isWorking,
    workingTurnId: input.isWorking ? input.workingTurnId : null,
    generationSuperseded: input.generationSuperseded,
    executionKey: input.executionKey,
    authority: input.record.providerAuthorityPubkey ?? null,
    driver: target?.driver ?? null,
    instanceId: target?.instanceId ?? null,
    sessionId: target?.sessionId ?? null,
    generation: target?.generation ?? null,
  };
}

function revisionKeysEqual(left: RevisionKey, right: RevisionKey): boolean {
  return (
    left.transcript === right.transcript &&
    left.isWorking === right.isWorking &&
    left.workingTurnId === right.workingTurnId &&
    left.generationSuperseded === right.generationSuperseded &&
    left.executionKey === right.executionKey &&
    left.authority === right.authority &&
    left.driver === right.driver &&
    left.instanceId === right.instanceId &&
    left.sessionId === right.sessionId &&
    left.generation === right.generation
  );
}

function deriveRevision(
  state: GenerationState,
  input: CodingSessionExecutionModelInput,
  key: RevisionKey,
  counters: Counters,
): Revision {
  const { record, executionKey, isWorking, generationSuperseded } = input;
  const workingTurnId = isWorking ? input.workingTurnId : null;
  const transcript = record.transcript;
  const previous = state.revision;
  const derivedTasks = deriveCodingSessionBlockBackgroundTasks({
    transcript,
    blockItems: transcript,
    generationSuperseded,
  });
  const backgroundTasksByTurn =
    previous &&
    codingSessionBackgroundTasksByTurnEqual(
      previous.backgroundTasksByTurn,
      derivedTasks,
    )
      ? previous.backgroundTasksByTurn
      : derivedTasks;
  for (const tasks of backgroundTasksByTurn.values()) freezeDeep(tasks);
  const model = stabilizeById(
    previous?.model ?? null,
    deriveCodingSessionTranscriptModel(transcript, {
      isWorking,
      // The working block's own turn, never "whichever appeared last".
      ...(workingTurnId === null ? {} : { workingTurnId }),
      backgroundTasksByTurn,
    }),
  );
  freezeDeep(model);
  const previousTimeline = previous?.timeline ?? null;

  // Per-revision lazies: the grouping, the ownership index, and each
  // selection / minimap answer, computed once however many views ask.
  let timeline: readonly TimelineBlock[] | null = null;
  let ownership: Ownership | null = null;
  const revisionSelections = new Map<string, CodingSessionTranscriptModel>();
  const minimapTurns = new Map<number, CodingSessionTranscriptTurn | null>();

  const timelineOf = (): readonly TimelineBlock[] => {
    if (timeline === null) {
      timeline = Object.freeze(
        groupTranscriptIntoTurnBlocks(record, executionKey).map(
          (block, seq): TimelineBlock => {
            const prior = previousTimeline?.[seq];
            const items =
              prior && arraysReferenceEqual(prior.items, block.items)
                ? prior.items
                : Object.freeze(block.items);
            return Object.freeze({ turnId: block.turnId, items });
          },
        ),
      );
      revision.timeline = timeline;
    }
    return timeline;
  };
  const ownershipOf = (): Ownership => {
    ownership ??= buildOwnership(model, timelineOf());
    return ownership;
  };

  const selectBlock = (
    blockSeq: number,
    options?: CodingSessionBlockSelectionOptions,
  ): CodingSessionTranscriptModel => {
    counters.selections += 1;
    const variant = options?.variant ?? "whole";
    const hide = options?.hideRehydrationClaim === true;
    const selectionKey = `${blockSeq}|${variant}|${hide ? 1 : 0}`;
    const cached = revisionSelections.get(selectionKey);
    if (cached) {
      counters.selectionHits += 1;
      return cached;
    }
    const owned = ownershipOf();
    const blocks: CodingSessionTranscriptBlock[] = [];
    for (const block of owned.blocks.get(blockSeq) ?? EMPTY_BLOCKS) {
      const selected = selectVariantBlock(state, block, variant, hide);
      if (selected) blocks.push(selected);
    }
    const ownedDiagnostics = owned.diagnostics.get(blockSeq) ?? EMPTY_ITEMS;
    const diagnostics =
      variant === "mission-execution"
        ? EMPTY_ITEMS
        : hide
          ? ownedDiagnostics.filter(
              (item) => !isCodingSessionRehydrationClaimItem(item),
            )
          : ownedDiagnostics;
    const retained = state.selections.get(selectionKey);
    let selection: CodingSessionTranscriptModel;
    if (
      retained &&
      arraysReferenceEqual(retained.blocks, blocks) &&
      arraysReferenceEqual(retained.diagnostics, diagnostics)
    ) {
      counters.selectionHits += 1;
      selection = retained;
    } else {
      selection = Object.freeze({
        blocks: (blocks.length === 0
          ? EMPTY_BLOCKS
          : Object.freeze(blocks)) as CodingSessionTranscriptBlock[],
        diagnostics: (diagnostics.length === 0
          ? EMPTY_ITEMS
          : Object.isFrozen(diagnostics)
            ? diagnostics
            : Object.freeze(diagnostics)) as TranscriptItem[],
        sessionFacts: EMPTY_ITEMS as TranscriptItem[],
      });
    }
    state.selections.set(selectionKey, selection);
    revisionSelections.set(selectionKey, selection);
    return selection;
  };

  const selectMinimapTurn = (
    blockSeq: number,
  ): CodingSessionTranscriptTurn | null => {
    const cached = minimapTurns.get(blockSeq);
    if (cached !== undefined) return cached;
    let found: CodingSessionTranscriptTurn | null = null;
    for (const block of ownershipOf().blocks.get(blockSeq) ?? EMPTY_BLOCKS) {
      if (block.kind === "turn" && opensWithPrompt(block)) {
        found = block;
        break;
      }
    }
    minimapTurns.set(blockSeq, found);
    return found;
  };

  const blockItems = (blockSeq: number): readonly TranscriptItem[] =>
    timelineOf()[blockSeq]?.items ?? EMPTY_ITEMS;

  const published: CodingSessionExecutionModel = Object.freeze({
    generationId: record.generationId,
    executionKey,
    model,
    backgroundTasksByTurn,
    selectBlock,
    selectMinimapTurn,
    blockItems,
  });
  const revision: Revision = {
    key,
    model,
    backgroundTasksByTurn,
    timeline: null,
    published,
  };
  return revision;
}

/**
 * {@link stabilizeCodingSessionTranscriptModel}, with the prior blocks lined
 * up by identity first. That function compares block `i` with prior block
 * `i`, so one block arriving before the others (a late unturned row) would
 * shift every later block off its prior twin and re-render all of them. The
 * equality test is still its own, entry by entry.
 */
function stabilizeById(
  previous: CodingSessionTranscriptModel | null,
  next: CodingSessionTranscriptModel,
): CodingSessionTranscriptModel {
  if (!previous) return next;
  const priorByKey = new Map<string, CodingSessionTranscriptBlock[]>();
  for (const block of previous.blocks) {
    const key = `${block.kind}:${block.id}`;
    const list = priorByKey.get(key);
    if (list) list.push(block);
    else priorByKey.set(key, [block]);
  }
  const aligned: CodingSessionTranscriptModel = {
    blocks: next.blocks.map(
      (block) =>
        priorByKey.get(`${block.kind}:${block.id}`)?.shift() as
          | CodingSessionTranscriptBlock
          | undefined,
    ) as CodingSessionTranscriptBlock[],
    diagnostics: previous.diagnostics,
    sessionFacts: previous.sessionFacts,
  };
  const stable = stabilizeCodingSessionTranscriptModel(aligned, next);
  if (stable !== aligned) return stable;
  // Every block matched its prior twin: keep the prior model when nothing moved.
  return arraysReferenceEqual(previous.blocks, aligned.blocks)
    ? previous
    : {
        blocks: aligned.blocks,
        diagnostics: aligned.diagnostics,
        sessionFacts: aligned.sessionFacts,
      };
}

/** Does the turn have a prompt row — the minimap's rule? */
function opensWithPrompt(turn: CodingSessionTranscriptTurn): boolean {
  return turn.entries.some(
    (entry) =>
      entry.kind === "item" &&
      entry.item.type === "message" &&
      entry.item.role === "user",
  );
}

// ---------------------------------------------------------------------------
// Ownership: model block -> the FIRST timeline block holding any of its items
// ---------------------------------------------------------------------------

function buildOwnership(
  model: CodingSessionTranscriptModel,
  timeline: readonly TimelineBlock[],
): Ownership {
  const seqById = new Map<string, number>();
  const firstSeqByTurn = new Map<string, number>();
  timeline.forEach((block, seq) => {
    if (block.turnId !== null && !firstSeqByTurn.has(block.turnId)) {
      firstSeqByTurn.set(block.turnId, seq);
    }
    for (const item of block.items) {
      if (!seqById.has(item.id)) seqById.set(item.id, seq);
    }
  });
  const seqOf = (id: string): number | undefined => {
    const direct = seqById.get(id);
    if (direct !== undefined || !id.endsWith(RESULT_BODY_SUFFIX)) return direct;
    // A Turn result's synthesized answer row lives where its result does.
    return seqById.get(id.slice(0, -RESULT_BODY_SUFFIX.length));
  };

  const blocks = new Map<number, CodingSessionTranscriptBlock[]>();
  for (const block of model.blocks) {
    let owner = Number.POSITIVE_INFINITY;
    const consider = (id: string) => {
      const seq = seqOf(id);
      if (seq !== undefined && seq < owner) owner = seq;
    };
    if (block.kind === "turn") {
      owner = firstSeqByTurn.get(block.id) ?? owner;
      if (block.id.startsWith(SETTLED_TURN_PREFIX)) {
        consider(block.id.slice(SETTLED_TURN_PREFIX.length));
      }
      for (const entry of block.entries) forEachEntryItemId(entry, consider);
      for (const item of block.diagnostics) consider(item.id);
    } else {
      forEachEntryItemId(block.entry, consider);
    }
    if (!Number.isFinite(owner)) continue;
    const list = blocks.get(owner);
    if (list) list.push(block);
    else blocks.set(owner, [block]);
  }

  const diagnostics = new Map<number, TranscriptItem[]>();
  for (const item of model.diagnostics) {
    const seq = seqOf(item.id);
    if (seq === undefined) continue;
    const list = diagnostics.get(seq);
    if (list) list.push(item);
    else diagnostics.set(seq, [item]);
  }
  for (const list of blocks.values()) Object.freeze(list);
  for (const list of diagnostics.values()) Object.freeze(list);
  return { blocks, diagnostics };
}

function forEachEntryItemId(
  entry: CodingSessionTranscriptEntry,
  visit: (id: string) => void,
): void {
  if (entry.kind === "item") {
    visit(entry.item.id);
  } else if (entry.kind === "tool-group") {
    for (const item of entry.items) visit(item.id);
  } else {
    for (const spawn of entry.spawns) {
      visit(spawn.call.id);
      for (const child of spawn.children) visit(child.id);
    }
  }
}

// ---------------------------------------------------------------------------
// Variants
// ---------------------------------------------------------------------------

/** A tool row in either lens: a tool item, a tool group, a subagent batch. */
function isExecutionEntry(entry: CodingSessionTranscriptEntry): boolean {
  return entry.kind !== "item" || entry.item.type === "tool";
}

function isHiddenEntry(entry: CodingSessionTranscriptEntry): boolean {
  return (
    entry.kind === "item" && isCodingSessionRehydrationClaimItem(entry.item)
  );
}

function selectVariantBlock(
  state: GenerationState,
  block: CodingSessionTranscriptBlock,
  variant: CodingSessionBlockSelectionVariant,
  hide: boolean,
): CodingSessionTranscriptBlock | null {
  if (variant === "whole" && !hide) return block;
  if (block.kind === "standalone") {
    if (hide && isHiddenEntry(block.entry)) return null;
    if (variant === "mission-narrative" && isExecutionEntry(block.entry)) {
      return null;
    }
    if (variant === "mission-execution" && !isExecutionEntry(block.entry)) {
      return null;
    }
    return block;
  }
  const variantKey = `${variant}|${hide ? 1 : 0}`;
  let byVariant = state.variantTurns.get(block);
  if (!byVariant) {
    byVariant = new Map();
    state.variantTurns.set(block, byVariant);
  }
  const cached = byVariant.get(variantKey);
  if (cached !== undefined) return cached;
  const derived = deriveVariantTurn(block, variant, hide);
  if (derived !== null && derived !== block) freezeDeep(derived);
  byVariant.set(variantKey, derived);
  return derived;
}

function deriveVariantTurn(
  turn: CodingSessionTranscriptTurn,
  variant: CodingSessionBlockSelectionVariant,
  hide: boolean,
): CodingSessionTranscriptTurn | null {
  const visible = hide
    ? turn.entries.filter((entry) => !isHiddenEntry(entry))
    : turn.entries;
  if (variant === "mission-execution") {
    const entries = visible.filter(isExecutionEntry);
    if (entries.length === 0) return null;
    return {
      ...turn,
      entries,
      completion: null,
      fold: null,
      backgroundTasks: NO_TASKS,
      autonomousWake: null,
      diagnostics: EMPTY_ITEMS as TranscriptItem[],
    };
  }
  if (variant === "whole" && visible.length === turn.entries.length) {
    return turn;
  }
  const entries = rejoinProse(
    variant === "mission-narrative"
      ? visible.filter((entry) => !isExecutionEntry(entry))
      : visible,
  );
  return {
    ...turn,
    entries,
    ...(variant === "mission-narrative"
      ? { changedFiles: NO_CHANGED_FILES }
      : {}),
    fold: deriveCodingSessionTurnFold({
      completion: turn.completion,
      entries,
      isWorking: turn.isWorking,
      startedAt: turn.startedAt,
    }),
  };
}

/**
 * Re-join prose the removed rows had kept apart, exactly as the model joins
 * a turn's visible items; a Turn result's own answer row stays separate.
 */
function rejoinProse(
  entries: readonly CodingSessionTranscriptEntry[],
): CodingSessionTranscriptEntry[] {
  const out: CodingSessionTranscriptEntry[] = [];
  let run: Array<Extract<CodingSessionTranscriptEntry, { kind: "item" }>> = [];
  const flush = () => {
    if (run.length === 0) return;
    const items = run.map((entry) => entry.item);
    const separate = new Set(
      items.filter((item) => item.id.endsWith(RESULT_BODY_SUFFIX)),
    );
    const joined = joinConsecutiveCodingSessionProse(items, separate);
    if (joined.length === items.length) {
      out.push(...run);
    } else {
      const entryOf = new Map(run.map((entry) => [entry.item, entry]));
      for (const item of joined) {
        out.push(entryOf.get(item) ?? { kind: "item", item });
      }
    }
    run = [];
  };
  for (const entry of entries) {
    if (entry.kind === "item") {
      run.push(entry);
      continue;
    }
    flush();
    out.push(entry);
  }
  flush();
  return out;
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/**
 * Freeze a newly built object graph, stopping at anything already frozen
 * (items arrive deep-frozen; reused blocks were frozen when first published).
 * Maps and Sets are left alone: freezing them does not stop `set`.
 */
function freezeDeep(value: unknown): void {
  if (value === null || typeof value !== "object" || Object.isFrozen(value)) {
    return;
  }
  if (value instanceof Map || value instanceof Set) return;
  Object.freeze(value);
  for (const key of Object.keys(value)) {
    freezeDeep((value as Record<string, unknown>)[key]);
  }
}

function arraysReferenceEqual<T>(
  left: readonly T[],
  right: readonly T[],
): boolean {
  return (
    left === right ||
    (left.length === right.length &&
      left.every((value, index) => value === right[index]))
  );
}
