import type { TranscriptItem } from "@/features/agents/ui/agentSessionTypes";

export type CodingSessionContextWindow = {
  usedTokens: number;
  maxTokens: number | null;
  usedPercentage: number | null;
};

const CONTEXT_WINDOW_TITLE = "Context Window Updated";

/**
 * Read the newest provider-signed context-window observation.
 *
 * The transcript projection deliberately keeps this telemetry out of the
 * narrative. The composer can still use the original signed item for a small
 * meter, but it must disappear when the provider supplied no usable count — a
 * decorative ring would be indistinguishable from a working spinner.
 */
export function deriveCodingSessionContextWindow(
  transcript: readonly TranscriptItem[],
): CodingSessionContextWindow | null {
  for (let index = transcript.length - 1; index >= 0; index -= 1) {
    const item = transcript[index];
    if (item.type !== "lifecycle" || item.title !== CONTEXT_WINDOW_TITLE) {
      continue;
    }

    const values = parseNumericLines(item.text);
    const usedTokens =
      readNonnegative(values, "usedTokens") ??
      readNonnegative(values, "inputTokens") ??
      readNonnegative(values, "totalTokens");
    if (usedTokens === null) return null;

    const maxTokens =
      readPositive(values, "maxTokens") ??
      readPositive(values, "contextWindow") ??
      readPositive(values, "contextWindowTokens");
    const usedPercentage =
      maxTokens === null
        ? null
        : Math.max(0, Math.min(100, (usedTokens / maxTokens) * 100));
    return { usedTokens, maxTokens, usedPercentage };
  }

  return null;
}

function parseNumericLines(text: string): ReadonlyMap<string, number> {
  const values = new Map<string, number>();
  for (const line of text.split("\n")) {
    const separator = line.indexOf(":");
    if (separator <= 0) continue;
    const key = line.slice(0, separator).trim();
    const value = Number(line.slice(separator + 1).trim());
    if (key.length > 0 && Number.isFinite(value)) values.set(key, value);
  }
  return values;
}

function readNonnegative(
  values: ReadonlyMap<string, number>,
  key: string,
): number | null {
  const value = values.get(key);
  return value !== undefined && value >= 0 ? value : null;
}

function readPositive(
  values: ReadonlyMap<string, number>,
  key: string,
): number | null {
  const value = values.get(key);
  return value !== undefined && value > 0 ? value : null;
}
