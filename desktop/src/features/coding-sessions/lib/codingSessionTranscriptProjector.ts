/**
 * Retained, resumable projection of ONE transcript generation (SV-118).
 *
 * The full projection re-presents every entry of a generation on every
 * streamed event. Its fold is a left fold, so when the new entry list only
 * grew at the end — the ordinary streaming case — the retained fold state can
 * take the new entries alone. Anything else (a late backfill, a conflict that
 * withholds an entry, a reorder, a changed scope) discards the state and
 * replays from the first entry, which is exactly the full projection.
 *
 * Publication is immutable: every published array is frozen and every item is
 * deep-frozen once, when it is created. A tool result that completes an
 * earlier call stores a NEW object in the next published array, so an array or
 * item handed out earlier never changes underneath its holder, and unchanged
 * items keep their reference identity across appends (memoized rows stay put).
 */

import type { TranscriptItem } from "@/features/agents/ui/agentSessionTypes";
import {
  type TrustedCodingSessionTranscriptEntry,
  trustedCodingSessionTranscriptEnvelope,
} from "./codingSessionTranscriptPresentation";
import {
  type CodingSessionProjectedTranscriptItem,
  type CodingSessionTranscriptFold,
  createCodingSessionTranscriptFold,
} from "./codingSessionTranscriptProjection";

/** The caller-owned scope a generation is projected under. */
export type CodingSessionTranscriptProjectionContext = Readonly<{
  channelId: string;
  generationId: string;
  bridgeSource: Readonly<{ pubkey: string; label: string }>;
}>;

/**
 * Work counters, so a caller (and the replay benchmark) can see what an update
 * cost rather than infer it. `prefixComparisons` and `copiedItems` grow with
 * history on purpose: proving the prefix and publishing a fresh frozen array
 * are linear, only the presentation work is not.
 */
export type CodingSessionTranscriptProjectorStats = {
  /** `update()` calls. */
  updates: number;
  /** Identical input and context: the previous array, no work. */
  retained: number;
  /** Resumed from the previous fold (suffix append). */
  appended: number;
  /** Fold discarded and replayed from the first entry. */
  rebuilt: number;
  /** Entries run through the item builders (the expensive work). */
  presentedEntries: number;
  /** Entry identity comparisons made checking the prefix. */
  prefixComparisons: number;
  /** Item references copied into published arrays. */
  copiedItems: number;
};

export type CodingSessionTranscriptProjector = {
  /**
   * `orderedEntries` is ONE generation's already-selected entries
   * (`selectExactTrustedCodingSessionTranscriptEntries`: conflict-free, exact
   * channel/signer/target, sorted by eventSeq then eventId).
   */
  update(
    orderedEntries: readonly TrustedCodingSessionTranscriptEntry[],
    context: CodingSessionTranscriptProjectionContext,
  ): readonly TranscriptItem[];
  /** Discard all retained state; the next update rebuilds. */
  reset(): void;
  readonly stats: Readonly<CodingSessionTranscriptProjectorStats>;
};

const EMPTY: readonly TranscriptItem[] = Object.freeze([]);

type Retained = {
  /** Our own copy of the entries folded so far (never the caller's array). */
  entries: TrustedCodingSessionTranscriptEntry[];
  context: CodingSessionTranscriptProjectionContext;
  fold: CodingSessionTranscriptFold;
  published: readonly TranscriptItem[];
};

export function createCodingSessionTranscriptProjector(): CodingSessionTranscriptProjector {
  const stats: CodingSessionTranscriptProjectorStats = {
    updates: 0,
    retained: 0,
    appended: 0,
    rebuilt: 0,
    presentedEntries: 0,
    prefixComparisons: 0,
    copiedItems: 0,
  };
  // Per instance: objects already deep-frozen. Items share sub-objects (the
  // context's bridge source, wire `input` records), so each is walked once.
  const sealed = new WeakSet<object>();
  const seal = (item: CodingSessionProjectedTranscriptItem) => {
    deepFreeze(item, sealed);
    return item;
  };
  let retained: Retained | null = null;

  const publish = (state: Retained): readonly TranscriptItem[] => {
    // A copy, never the fold's buffer: a later paired result replaces a slot
    // in the buffer, and the holder of this array must not see it move.
    const items = state.fold.items;
    stats.copiedItems += items.length;
    state.published = Object.freeze(items.slice());
    return state.published;
  };

  const rebuild = (
    entries: readonly TrustedCodingSessionTranscriptEntry[],
    context: CodingSessionTranscriptProjectionContext,
  ): readonly TranscriptItem[] => {
    stats.rebuilt += 1;
    if (entries.length === 0) {
      retained = {
        entries: [],
        context,
        fold: createCodingSessionTranscriptFold(foldOptions(context), seal),
        published: EMPTY,
      };
      return EMPTY;
    }
    const state: Retained = {
      entries: entries.slice(),
      context,
      fold: createCodingSessionTranscriptFold(foldOptions(context), seal),
      published: EMPTY,
    };
    retained = state;
    for (const entry of state.entries) presentEntry(state, entry);
    return publish(state);
  };

  const presentEntry = (
    state: Retained,
    entry: TrustedCodingSessionTranscriptEntry,
  ) => {
    stats.presentedEntries += 1;
    let envelope: unknown;
    try {
      envelope = trustedCodingSessionTranscriptEnvelope(entry);
    } catch {
      // A hostile entry (a throwing getter) still occupies its slot: the fold
      // turns the raw value into a bounded fallback item.
      envelope = entry;
    }
    state.fold.push(envelope);
  };

  return {
    stats,
    reset() {
      retained = null;
    },
    update(orderedEntries, context) {
      stats.updates += 1;
      const entries = Array.isArray(orderedEntries) ? orderedEntries : [];
      const state = retained;
      if (state === null || !sameContext(state.context, context)) {
        return rebuild(entries, context);
      }
      const previous = state.entries;
      if (entries.length < previous.length) {
        return rebuild(entries, context);
      }
      for (let index = 0; index < previous.length; index += 1) {
        stats.prefixComparisons += 1;
        if (previous[index] !== entries[index]) {
          return rebuild(entries, context);
        }
      }
      if (entries.length === previous.length) {
        stats.retained += 1;
        return state.published;
      }
      stats.appended += 1;
      for (let index = previous.length; index < entries.length; index += 1) {
        const entry = entries[index] as TrustedCodingSessionTranscriptEntry;
        previous.push(entry);
        presentEntry(state, entry);
      }
      return publish(state);
    },
  };
}

function foldOptions(context: CodingSessionTranscriptProjectionContext) {
  return {
    channelId: context.channelId,
    generationId: context.generationId,
    bridgeSource: context.bridgeSource,
  };
}

/**
 * Field equality, not object identity: callers rebuild the context each
 * render, and an equal scope must not cost a replay.
 */
function sameContext(
  left: CodingSessionTranscriptProjectionContext,
  right: CodingSessionTranscriptProjectionContext,
): boolean {
  try {
    return (
      left.channelId === right.channelId &&
      left.generationId === right.generationId &&
      left.bridgeSource.pubkey === right.bridgeSource.pubkey &&
      left.bridgeSource.label === right.bridgeSource.label
    );
  } catch {
    return false;
  }
}

/**
 * Freeze an item and everything reachable from it, once. This includes the
 * wire records an item shares by reference (`args` is the envelope's `input`),
 * which are immutable by contract already. Iterative so a deep or cyclic
 * value cannot overflow the stack, and best-effort per object: a value that
 * refuses to freeze (a typed array, a proxy trap) stays as it is rather than
 * aborting publication.
 */
function deepFreeze(root: unknown, sealed: WeakSet<object>): void {
  const stack: unknown[] = [root];
  while (stack.length > 0) {
    const value = stack.pop();
    // Functions are never transcript data; freezing one would reach into
    // whatever module owns it.
    if (typeof value !== "object" || value === null || sealed.has(value)) {
      continue;
    }
    sealed.add(value);
    try {
      for (const key of Reflect.ownKeys(value)) {
        const descriptor = Reflect.getOwnPropertyDescriptor(value, key);
        if (descriptor && "value" in descriptor) stack.push(descriptor.value);
      }
      Object.freeze(value);
    } catch {
      // Best effort; see above.
    }
  }
}
