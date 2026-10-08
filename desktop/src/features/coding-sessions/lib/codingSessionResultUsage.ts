/**
 * The `result` item's per-turn usage block — split out of
 * `codingSessionTranscriptItems.ts`, which builds the row it rides on.
 */
import type { TranscriptItem } from "@/features/agents/ui/agentSessionTypes";
import { isRecord } from "./codingSessionDefensive";

/** The six numbers `TurnUsageReport` may carry, in the order the wire lists them. */
const RESULT_USAGE_FIELDS = [
  "inputTokens",
  "outputTokens",
  "cacheReadTokens",
  "cacheWriteTokens",
  "toolCalls",
  "contextWindow",
] as const;

/**
 * The `result` item's per-turn `usage` block, read defensively.
 *
 * Only the six fields the wire's `TurnUsageReport` defines survive, and only
 * when they are finite numbers — the block is additive and every field is
 * independently optional, so an unreadable or absent field is dropped rather
 * than reported as `0`. A block with nothing readable in it becomes `null`,
 * which is what "the driver reported no usage" means to every consumer.
 */
export function buildResultUsage(
  raw: unknown,
): NonNullable<Extract<TranscriptItem, { type: "lifecycle" }>["usage"]> | null {
  if (!isRecord(raw)) return null;
  const usage: Record<string, number> = {};
  for (const field of RESULT_USAGE_FIELDS) {
    const value = raw[field];
    if (typeof value === "number" && Number.isFinite(value)) {
      usage[field] = value;
    }
  }
  return Object.keys(usage).length > 0 ? usage : null;
}
