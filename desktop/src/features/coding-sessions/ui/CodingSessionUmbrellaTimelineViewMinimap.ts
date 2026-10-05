import * as React from "react";

import { deriveCodingSessionTranscriptModel } from "@/features/coding-sessions/lib/codingSessionTranscriptModel";
import {
  deriveCodingSessionMinimapItems,
  type CodingSessionMinimapItem,
  type CodingSessionMinimapTurnSource,
} from "@/features/coding-sessions/lib/codingSessionTranscriptMinimapItems";
import type { CodingSessionTranscriptModel } from "@/features/coding-sessions/lib/codingSessionTranscriptModelTypes";
import {
  codingSessionUmbrellaEntryKey,
  type CodingSessionUmbrellaTimelineEntry,
} from "@/features/coding-sessions/lib/codingSessionUmbrellaTimeline";

type CachedModel = { working: boolean; model: CodingSessionTranscriptModel };

/**
 * The umbrella narrative's minimap items (SV-26): one per turn block whose
 * turn opens with a prompt, in the narrative's reading order, keyed by the
 * block key the view registers its node under.
 *
 * Each block's turn model is derived once per block items array and kept in
 * a per-view `WeakMap` (a ref, never a module cache), so a streamed item
 * re-derives only the block it landed in.
 */
export function useCodingSessionUmbrellaTimelineMinimapItems(input: {
  enabled: boolean;
  entries: readonly CodingSessionUmbrellaTimelineEntry[];
  workingBlockKeys: ReadonlySet<string>;
  currentUserPubkey: string | null;
}): CodingSessionMinimapItem[] {
  const { currentUserPubkey, enabled, entries, workingBlockKeys } = input;
  const cacheRef = React.useRef(new WeakMap<object, CachedModel>());
  return React.useMemo(() => {
    if (!enabled) return [];
    const cache = cacheRef.current;
    const sources: CodingSessionMinimapTurnSource[] = [];
    entries.forEach((entry, rowIndex) => {
      if (entry.kind !== "turn-block") return;
      const key = codingSessionUmbrellaEntryKey(entry);
      const working = workingBlockKeys.has(key);
      let cached = cache.get(entry.items);
      if (cached === undefined || cached.working !== working) {
        cached = {
          working,
          model: deriveCodingSessionTranscriptModel(entry.items, {
            isWorking: working,
          }),
        };
        cache.set(entry.items, cached);
      }
      // The block's first turn that opens with a prompt; a block is one
      // producer turn, so there is almost always exactly one turn here.
      const turn = cached.model.blocks.find(
        (block) =>
          block.kind === "turn" &&
          block.entries.some(
            (candidate) =>
              candidate.kind === "item" &&
              candidate.item.type === "message" &&
              candidate.item.role === "user",
          ),
      );
      if (turn === undefined || turn.kind !== "turn") return;
      sources.push({
        key,
        rowIndex,
        turn,
        fallbackStartedAtMs: entry.timestampMs,
      });
    });
    return deriveCodingSessionMinimapItems(sources, currentUserPubkey);
  }, [currentUserPubkey, enabled, entries, workingBlockKeys]);
}
