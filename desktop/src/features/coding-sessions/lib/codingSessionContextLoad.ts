/**
 * How much of its model's context one execution is holding (SURFACES.md D7,
 * wire fact W12).
 *
 * Read off the wire, never estimated — the same rule and the same rendering
 * `bee sessions status` uses (`crates/beekeeper-cli/src/commands/sessions/
 * crew_cmds.rs:884-933`). The source is the driver's own
 * `context_window_updated` item: it measured the prompt it was about to send,
 * so its number can never exceed the window.
 *
 * **Known limit.** The CLI has a second source — the terminal `result` item's
 * `usage` block — which the desktop cannot reach today, because the transcript
 * projection drops `usage` when it builds the row
 * (`codingSessionTranscriptItems.ts:544-574`). A driver that reports only
 * `result.usage` therefore reads `—` here and a number in the CLI. `—` is the
 * honest answer for "nothing this client can see has said", and is never
 * rendered as zero.
 */

import type { TranscriptItem } from "@/features/agents/ui/agentSessionTypes";

/** The title `buildContextWindowStatusItem` gives a projected occupancy item. */
const CONTEXT_WINDOW_TITLE = "Context Window Updated";

/** One execution's context occupancy, exactly as the driver reported it. */
export type CodingSessionContextLoad = {
  usedTokens: number;
  /**
   * The window itself, when the driver named it. `null` rather than a guess:
   * a percentage against an invented denominator is a lie.
   */
  contextWindow: number | null;
  /** Percent of the window in use, rounded half-up; null when no window is known. */
  pct: number | null;
};

/**
 * Fold a projected transcript into its newest reported context load.
 *
 * `null` means no occupancy item reached this client — reported as nothing,
 * never as zero. Both spellings drivers use for each number are accepted
 * (`used`/`usedTokens`, `size`/`contextWindow`/`contextLimit`/`maxTokens`),
 * because the ACP schema pins neither; this mirrors `context_window_usage`
 * in `crates/beekeeper-core/src/coding_session_payload.rs:1268`.
 */
export function readCodingSessionContextLoad(
  transcript: readonly TranscriptItem[],
): CodingSessionContextLoad | null {
  for (let index = transcript.length - 1; index >= 0; index -= 1) {
    const item = transcript[index];
    if (item.type !== "lifecycle" || item.title !== CONTEXT_WINDOW_TITLE) {
      continue;
    }
    const values = parseNumericLines(item.text);
    const usedTokens = firstOf(
      values,
      ["used", "usedTokens"],
      (value) => value >= 0,
    );
    if (usedTokens === null) continue;
    const contextWindow = firstOf(
      values,
      ["size", "contextWindow", "contextLimit", "maxTokens"],
      (value) => value > 0,
    );
    return {
      usedTokens,
      contextWindow,
      pct: percentOf(usedTokens, contextWindow),
    };
  }
  return null;
}

/**
 * The cell a person reads — the exact rendering `ContextLoad::render` prints.
 *
 * `null` renders as an em dash: nothing has reported, which is not zero.
 */
export function renderCodingSessionContextLoad(
  load: CodingSessionContextLoad | null,
): string {
  if (load === null) return "—";
  if (load.contextWindow === null || load.pct === null) {
    return `${load.usedTokens} tokens (window unknown)`;
  }
  return `${load.usedTokens}/${load.contextWindow} (${load.pct}%)`;
}

/**
 * Percent of the window in use, rounded half-up to a whole percent — 137 498
 * of 1 000 000 reads `14%`, matching `ContextLoad::pct`.
 */
function percentOf(
  usedTokens: number,
  contextWindow: number | null,
): number | null {
  if (contextWindow === null || contextWindow <= 0) return null;
  return Math.ceil(Math.floor((usedTokens * 200) / contextWindow) / 2);
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

function firstOf(
  values: ReadonlyMap<string, number>,
  keys: readonly string[],
  accept: (value: number) => boolean,
): number | null {
  for (const key of keys) {
    const value = values.get(key);
    if (value !== undefined && accept(value)) return value;
  }
  return null;
}
