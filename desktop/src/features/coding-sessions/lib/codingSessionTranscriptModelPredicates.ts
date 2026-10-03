import type { TranscriptItem } from "@/features/agents/ui/agentSessionTypes";

/**
 * Item classifiers shared by the transcript model's derivation, grouping and
 * fold. Split out of `codingSessionTranscriptModel.ts` for the 1000-line
 * ceiling; nothing here holds state.
 */

/**
 * Lifecycle rows that belong in the diagnostics rail rather than the reading
 * order. "Content dropped" is deliberately absent: a dropped item is something
 * the reader must see in place, not a telemetry line.
 *
 * "Status" covers the provider's generic status slugs. The continuity slugs
 * are deliberately NOT among them: `buildStatusLifecycleItem` gives those the
 * `CODING_SESSION_CONTINUITY_TITLE` title instead, precisely so this gate
 * misses them and they stay in the reading order.
 */
const DIAGNOSTIC_LIFECYCLE_TITLES = new Set([
  "Account Info",
  "Context compact boundary",
  "Context compacted",
  "Context Window Updated",
  "Context cleared",
  "Status",
  "System Init",
  "Unrecognized transcript event",
  "Unrecognized transcript item",
]);

export function isCompletedSuccessfulTool(
  item: TranscriptItem,
): item is Extract<TranscriptItem, { type: "tool" }> {
  return (
    item.type === "tool" &&
    !item.isError &&
    item.status === "completed" &&
    item.renderClass !== "error"
  );
}

export function isTurnResult(
  item: TranscriptItem,
): item is Extract<TranscriptItem, { type: "lifecycle" }> {
  return item.type === "lifecycle" && item.title === "Turn result";
}

export function isInterrupted(item: TranscriptItem): boolean {
  return item.type === "lifecycle" && item.title === "Interrupted";
}

export function isTurnTerminal(item: TranscriptItem): boolean {
  return isTurnResult(item) || isInterrupted(item);
}

export function isErrorItem(item: TranscriptItem): boolean {
  if (isSystemInitMetadata(item)) return false;
  if (item.type === "tool") {
    return (
      item.isError || item.status === "failed" || item.renderClass === "error"
    );
  }
  if (item.type !== "lifecycle") return false;
  const searchable = `${item.title} ${item.text}`.toLowerCase();
  return (
    item.renderClass === "error" ||
    /\b(error|failed|failure)\b/.test(searchable)
  );
}

export function isSystemInitMetadata(item: TranscriptItem): boolean {
  return (
    item.type === "lifecycle" &&
    item.title === "System Init" &&
    item.renderClass === "status"
  );
}

export function isCeremonialDiagnostic(item: TranscriptItem): boolean {
  return item.type === "lifecycle" && item.title === "Context Window Updated";
}

export function isDiagnosticItem(item: TranscriptItem): boolean {
  if (item.renderClass === "raw-rail" || item.renderClass === "suppressed") {
    return true;
  }
  if (item.type !== "lifecycle") return false;
  if (item.title.startsWith("Unrecognized item kind:")) return true;
  return DIAGNOSTIC_LIFECYCLE_TITLES.has(item.title);
}

export function isCeremonialCompletionValue(value: string): boolean {
  return COMPLETION_CEREMONY_VALUES.has(value.trim().toLowerCase());
}

/**
 * Outcome words that mean "the turn ended normally" (or restate a state the
 * completion line already shows). Anything else — `max_tokens`, `refusal`,
 * `max_turn_requests`, … — is news about the turn and is shown, never folded
 * into hover metadata.
 */
const COMPLETION_CEREMONY_VALUES = new Set([
  "completed",
  "end_turn",
  "error",
  "failed",
  "interrupted",
  "result",
  "success",
  "unknown",
]);

export function normalizeContent(value: string): string {
  return value.trim().replace(/\s+/g, " ");
}
