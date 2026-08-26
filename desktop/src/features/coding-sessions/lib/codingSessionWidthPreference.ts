import * as React from "react";

/**
 * How much of the window a coding session's text is allowed to use.
 *
 * - `narrow` — the reading measure the app has always used. Long lines are
 *   the thing that makes a transcript tiring to read, so the default caps
 *   them at a comfortable column and centres it.
 * - `wide` — a larger cap. Still a column, still centred, but it gives back
 *   most of the space a big display was wasting.
 * - `full` — no cap at all; text runs to the window edge less a thin margin.
 *
 * This governs the *cap*, not just the padding, which is the whole point: on
 * a wide display the cap is what sets the text edges and any padding beyond
 * it is slack. A setting that moved only the padding would appear to do
 * nothing on exactly the displays whose owners went looking for it.
 *
 * Persisted in localStorage. This is a device-level UI preference, not
 * community-scoped data, so it is intentionally not reset on community switch.
 */
export type CodingSessionWidth = "narrow" | "wide" | "full";

/**
 * A deliberately new key. The first cut of this setting stored
 * `full | light | none` under `buzz.codingSessions.gutter`, where `full` meant
 * *full padding* — the narrowest text. Here `full` means *full width*, the
 * widest. Reusing the key would silently invert the choice of anyone who had
 * already set one, so the old key is abandoned rather than migrated.
 */
const STORAGE_KEY = "buzz.appearance.codingSessionWidth";

/** Width used when nothing is stored, or the stored value is unrecognized. */
export const DEFAULT_CODING_SESSION_WIDTH: CodingSessionWidth = "narrow";

/**
 * The measure cap for each choice.
 *
 * `expanded` is the cap when no side surface is sharing the workspace, which
 * is why every choice has two: a rail takes real width, and a column sized for
 * an empty workspace would be cramped beside one. `full` caps nothing — the
 * gutter alone holds the text off the edge.
 *
 * These are rem quantities, never px. The desktop app implements Cmd +/- by
 * scaling the root font size, so a rem cap widens with the glyphs and holds a
 * roughly constant character count; a px cap would freeze against the zoom.
 */
export const CODING_SESSION_MEASURE_CLASSES: Record<
  CodingSessionWidth,
  { default: string; expanded: string }
> = {
  narrow: { default: "max-w-3xl", expanded: "max-w-6xl" },
  wide: { default: "max-w-5xl", expanded: "max-w-7xl" },
  full: { default: "max-w-none", expanded: "max-w-none" },
};

/**
 * The margin between the text and the window edge, per choice.
 *
 * `full` is the only one that differs, and it is deliberately the inset a DM
 * conversation already uses: `px-5`, matching the channel composer dock
 * (`ChannelPane.tsx`) and the 20px a message row ends up at once its
 * `px-2` scroller, `mx-1` and `px-2` are added up. A DM timeline is the app's
 * existing uncapped full-width reading surface, so a Full transcript beside
 * one should start at the same place rather than at a margin of its own
 * invention.
 *
 * Note there is no `sm:` step here. The capped widths widen their margin on a
 * larger window because they have room to spare; Full spends that room on
 * text, which is the entire point of choosing it.
 */
export const CODING_SESSION_GUTTER_CLASSES: Record<CodingSessionWidth, string> =
  {
    narrow: "px-5 sm:px-8",
    wide: "px-5 sm:px-8",
    full: "px-5",
  };

/**
 * The choices as the settings page states them, in the order shown.
 *
 * No measurements. Someone choosing how their transcripts should look is
 * picking a reading experience, not entering a layout value, and a number
 * here would be both meaningless to that decision and wrong on half the
 * window sizes it could be read on.
 */
export const CODING_SESSION_WIDTH_OPTIONS: {
  value: CodingSessionWidth;
  label: string;
  description: string;
}[] = [
  {
    value: "narrow",
    label: "Narrow",
    description:
      "A comfortable reading column, centred. Easiest on the eyes for long conversations.",
  },
  {
    value: "wide",
    label: "Wide",
    description:
      "A roomier column. Keeps lines readable while making better use of a large window.",
  },
  {
    value: "full",
    label: "Full",
    description:
      "Text spans the whole window. Best for wide code, diffs, and tables — long prose lines get harder to follow.",
  },
];

/** The stored string, narrowed to a choice this build still offers. */
export function parseCodingSessionWidth(
  value: string | null | undefined,
): CodingSessionWidth {
  return value === "narrow" || value === "wide" || value === "full"
    ? value
    : DEFAULT_CODING_SESSION_WIDTH;
}

const listeners = new Set<() => void>();

let codingSessionWidth = readStoredCodingSessionWidth();

function readStoredCodingSessionWidth(): CodingSessionWidth {
  try {
    return parseCodingSessionWidth(
      globalThis.localStorage?.getItem(STORAGE_KEY),
    );
  } catch {
    return DEFAULT_CODING_SESSION_WIDTH;
  }
}

/**
 * A coding session can be popped out into its own webview, and that window is
 * a separate JS context with its own copy of everything in this module. Only
 * the `storage` event crosses between them, so without this listener changing
 * the setting in the main window leaves an open pop-out on the old width until
 * it is reloaded — the control silently failing on the surface most likely to
 * be showing a transcript.
 */
globalThis.addEventListener?.("storage", (event) => {
  if (event.key !== null && event.key !== STORAGE_KEY) return;
  const next = readStoredCodingSessionWidth();
  if (next === codingSessionWidth) return;
  codingSessionWidth = next;
  for (const listener of listeners) listener();
});

function subscribe(listener: () => void): () => void {
  listeners.add(listener);
  return () => {
    listeners.delete(listener);
  };
}

function getSnapshot(): CodingSessionWidth {
  return codingSessionWidth;
}

function getServerSnapshot(): CodingSessionWidth {
  return DEFAULT_CODING_SESSION_WIDTH;
}

/** Read the persisted coding-session width outside of React. */
export function getCodingSessionWidth(): CodingSessionWidth {
  return codingSessionWidth;
}

/** Update the coding-session width and notify every subscribed surface. */
export function setCodingSessionWidth(width: CodingSessionWidth): void {
  codingSessionWidth = width;

  try {
    globalThis.localStorage?.setItem(STORAGE_KEY, width);
  } catch {
    // Persistence is best-effort; the in-memory value still applies.
  }

  for (const listener of listeners) {
    listener();
  }
}

/** Which width the person chose for coding sessions. */
export function useCodingSessionWidth(): CodingSessionWidth {
  return React.useSyncExternalStore(subscribe, getSnapshot, getServerSnapshot);
}

/**
 * The measure cap for the chosen width, given whether a side surface is open.
 *
 * Used by the measure box itself — `CodingSessionColumn` and the goal pill,
 * which wears the same classes without wrapping in one.
 */
export function useCodingSessionMeasure(expanded: boolean): string {
  const width = useCodingSessionWidth();
  const measure = CODING_SESSION_MEASURE_CLASSES[width];
  return expanded ? measure.expanded : measure.default;
}

/**
 * The gutter classes for the surface that owns the viewport edge.
 *
 * Every coding-session surface calls this and spreads the result onto the
 * ancestor of its measure box — transcript scroller, composer dock, goal row —
 * so all of them move together. Applying it to the measure box instead
 * silently narrows the measure by the padding, which is how the transcript
 * text and the composer edge drifted out of register.
 */
export function useCodingSessionColumnGutter(): string {
  return CODING_SESSION_GUTTER_CLASSES[useCodingSessionWidth()];
}
