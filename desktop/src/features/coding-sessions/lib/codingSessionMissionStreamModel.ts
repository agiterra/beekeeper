import type { TranscriptItem } from "@/features/agents/ui/agentSessionTypes";
import type {
  CodingSessionUmbrellaTimelineEntry,
  CodingSessionUmbrellaTurnBlock,
} from "./codingSessionUmbrellaTimeline";
import { codingSessionUmbrellaEntryKey } from "./codingSessionUmbrellaTimeline";
import type { CodingSessionMissionDensity } from "./codingSessionMissionDensity";
import { CODING_SESSION_MISSION_TRANSACTION_ROW_LIMIT } from "./codingSessionMissionContracts";
import type { CodingSessionMissionTransactionRow } from "./codingSessionMissionTransactionRows";

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

/** A signed 44244 transaction, interleaved into the narrative chronology. */
export type CodingSessionMissionTransactionEntry = {
  kind: "transaction";
  row: CodingSessionMissionTransactionRow;
};

/** The one visible row that stands for transactions dropped by the bound. */
export type CodingSessionMissionTruncationEntry = {
  kind: "transaction-truncation";
  hiddenCount: number;
};

/** What the Mission stream renders: the narrative plus the causality plane. */
export type CodingSessionMissionStreamEntry =
  | CodingSessionUmbrellaTimelineEntry
  | CodingSessionMissionTransactionEntry
  | CodingSessionMissionTruncationEntry;

/** A React key for any Mission stream entry, including the merged rows. */
export function codingSessionMissionStreamEntryKey(
  entry: CodingSessionMissionStreamEntry,
): string {
  if (entry.kind === "transaction") return entry.row.key;
  if (entry.kind === "transaction-truncation") {
    return "transaction-truncation";
  }
  return codingSessionUmbrellaEntryKey(entry);
}

function entrySeconds(entry: CodingSessionUmbrellaTimelineEntry): number {
  return Number.isFinite(entry.timestampMs)
    ? Math.floor(entry.timestampMs / 1_000)
    : 0;
}

/**
 * Project one signed chronology into a density, and interleave the typed team
 * transactions that belong to it.
 *
 * Live and Trace retain every narrative entry. Brief removes only structured
 * routine execution; messages, plans, results, permissions, failures — and
 * **every** transaction row — remain visible: typed mission state is never
 * hidden by a reading density.
 *
 * Ordering is display-only and uses unix seconds, never author time for any
 * decision. Two transactions at the same second tie-break on their signed
 * event id; a transaction ties *after* a narrative entry at the same second,
 * which keeps the narrative's own carefully-clamped order untouched.
 *
 * With no transactions the return value is exactly what Conversation's path
 * produced before this merge existed.
 */
export function projectCodingSessionMissionTimeline(
  entries: readonly CodingSessionUmbrellaTimelineEntry[],
  density: CodingSessionMissionDensity,
  transactions?: readonly CodingSessionMissionTransactionRow[],
): CodingSessionMissionStreamEntry[] {
  const projected: CodingSessionUmbrellaTimelineEntry[] = [];
  if (density !== "brief") {
    projected.push(...entries);
  } else {
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
  }
  if (transactions === undefined || transactions.length === 0) {
    return projected;
  }

  const ordered = [...transactions].sort(
    (left, right) =>
      left.createdAt - right.createdAt ||
      left.meta.sourceEventId.localeCompare(right.meta.sourceEventId),
  );
  const hiddenCount = Math.max(
    0,
    ordered.length - CODING_SESSION_MISSION_TRANSACTION_ROW_LIMIT,
  );
  const retained = hiddenCount > 0 ? ordered.slice(hiddenCount) : ordered;

  const merged: CodingSessionMissionStreamEntry[] = [];
  let entryIndex = 0;
  let rowIndex = 0;
  let truncationEmitted = hiddenCount === 0;
  while (entryIndex < projected.length || rowIndex < retained.length) {
    const entry = projected[entryIndex];
    const row = retained[rowIndex];
    const takeEntry =
      row === undefined ||
      (entry !== undefined && entrySeconds(entry) <= row.createdAt);
    if (takeEntry && entry !== undefined) {
      merged.push(entry);
      entryIndex += 1;
      continue;
    }
    if (row === undefined) break;
    if (!truncationEmitted) {
      merged.push({ kind: "transaction-truncation", hiddenCount });
      truncationEmitted = true;
    }
    merged.push({ kind: "transaction", row });
    rowIndex += 1;
  }
  return merged;
}
