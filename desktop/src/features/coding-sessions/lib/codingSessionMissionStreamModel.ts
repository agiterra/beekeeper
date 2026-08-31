import type { TranscriptItem } from "@/features/agents/ui/agentSessionTypes";
import type {
  CodingSessionUmbrellaTimelineEntry,
  CodingSessionUmbrellaTurnBlock,
} from "./codingSessionUmbrellaTimeline";
import type { CodingSessionMissionDensity } from "./codingSessionMissionDensity";

/** Structured attention evidence that Brief is never allowed to suppress. */
export function isMissionBriefVisibleItem(item: TranscriptItem): boolean {
  if (item.type === "message" || item.type === "plan") return true;
  if (item.type === "lifecycle") {
    return (
      item.renderClass === "error" ||
      item.renderClass === "permission" ||
      item.renderClass === "status"
    );
  }
  return item.type === "tool" && (item.isError || item.status === "failed");
}

/**
 * Project one signed chronology into a density. Live and Trace retain every
 * entry. Brief removes only structured routine execution; messages, plans,
 * results, permissions, failures, and all typed mission state remain visible.
 */
export function projectCodingSessionMissionTimeline(
  entries: readonly CodingSessionUmbrellaTimelineEntry[],
  density: CodingSessionMissionDensity,
): CodingSessionUmbrellaTimelineEntry[] {
  if (density !== "brief") return [...entries];
  const projected: CodingSessionUmbrellaTimelineEntry[] = [];
  for (const entry of entries) {
    if (entry.kind !== "turn-block") {
      projected.push(entry);
      continue;
    }
    const items = entry.items.filter(isMissionBriefVisibleItem);
    if (items.length > 0) {
      projected.push({
        ...entry,
        items,
      } satisfies CodingSessionUmbrellaTurnBlock);
    }
  }
  return projected;
}
