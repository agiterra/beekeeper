import { CODING_SESSION_NAME_SUGGEST_INTERVAL_MS } from "./codingSessionNameSuggestion";

/**
 * What one press of Test produced, as a sentence.
 *
 * The interesting result is not "it worked" — it is *what came back* and
 * *how long it took*. A namer that returns a good title in eight seconds is
 * working and is still the wrong choice, because the create dialog asks it
 * again every five; nothing else in the app would ever show that, so the
 * test says it.
 */

export type CodingSessionNamingTestSummary =
  | { tone: "none"; headline: null; detail: null }
  | { tone: "pending"; headline: string; detail: null }
  | { tone: "ok"; headline: string; detail: string | null }
  | { tone: "failed"; headline: string; detail: null };

/** Milliseconds as the shortest honest string: `840ms`, `1.2s`, `11s`. */
export function formatNamingLatency(elapsedMs: number): string {
  const clamped = Math.max(0, Math.round(elapsedMs));
  if (clamped < 1000) return `${clamped}ms`;
  const seconds = clamped / 1000;
  return seconds < 10 ? `${seconds.toFixed(1)}s` : `${Math.round(seconds)}s`;
}

export function codingSessionNamingTestSummary(input: {
  error: string | null;
  isPending: boolean;
  result: { name: string; elapsedMs: number } | null;
}): CodingSessionNamingTestSummary {
  if (input.isPending) {
    return {
      tone: "pending",
      headline: "Naming a sample message…",
      detail: null,
    };
  }
  if (input.error) {
    return { tone: "failed", headline: input.error, detail: null };
  }
  if (!input.result) return { tone: "none", headline: null, detail: null };
  const latency = formatNamingLatency(input.result.elapsedMs);
  return {
    tone: "ok",
    headline: `Named it “${input.result.name}” in ${latency}.`,
    // Only said when it matters. A namer slower than the cadence that asks
    // it will always be a name or two behind what has been typed.
    detail:
      input.result.elapsedMs > CODING_SESSION_NAME_SUGGEST_INTERVAL_MS
        ? "That is slower than the every-five-seconds cadence, so names will lag behind what you write."
        : null,
  };
}
