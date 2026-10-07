import * as React from "react";

import type { CodingSessionExecutionModel } from "@/features/coding-sessions/lib/codingSessionExecutionModels";
import {
  deriveCodingSessionMinimapItems,
  type CodingSessionMinimapItem,
  type CodingSessionMinimapTurnSource,
} from "@/features/coding-sessions/lib/codingSessionTranscriptMinimapItems";
import {
  codingSessionUmbrellaEntryKey,
  type CodingSessionUmbrellaTimelineEntry,
} from "@/features/coding-sessions/lib/codingSessionUmbrellaTimeline";

/**
 * The umbrella narrative's minimap items (SV-26): one per turn block whose
 * turn opens with a prompt, in the narrative's reading order, keyed by the
 * block key the view registers its node under.
 *
 * SV-100: each block's turn is **selected** from its generation's shared
 * model (`selectMinimapTurn`), the same model the turn blocks render — the
 * minimap derives nothing of its own. A block whose generation has no model
 * (Mission Brief, where the minimap is off) is skipped.
 */
export function useCodingSessionUmbrellaTimelineMinimapItems(input: {
  enabled: boolean;
  entries: readonly CodingSessionUmbrellaTimelineEntry[];
  executionModels: ReadonlyMap<string, CodingSessionExecutionModel>;
  currentUserPubkey: string | null;
}): CodingSessionMinimapItem[] {
  const { currentUserPubkey, enabled, entries, executionModels } = input;
  return React.useMemo(() => {
    if (!enabled) return [];
    const sources: CodingSessionMinimapTurnSource[] = [];
    entries.forEach((entry, rowIndex) => {
      if (entry.kind !== "turn-block") return;
      const turn =
        executionModels
          .get(entry.generationId)
          ?.selectMinimapTurn(entry.blockSeq) ?? null;
      if (turn === null) return;
      sources.push({
        key: codingSessionUmbrellaEntryKey(entry),
        rowIndex,
        turn,
        fallbackStartedAtMs: entry.timestampMs,
      });
    });
    return deriveCodingSessionMinimapItems(sources, currentUserPubkey);
  }, [currentUserPubkey, enabled, entries, executionModels]);
}
