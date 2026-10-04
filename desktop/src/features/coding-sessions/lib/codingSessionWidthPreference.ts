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
 * `expanded` is the cap when no side surface is sharing the workspace.
 * `narrow` — the default — is the reading measure, 48rem (`max-w-3xl`, about
 * 880px of text at 1x), *whether or not* a side surface is open (SV-13): a
 * measure is about line length, and closing a rail does not make a 72rem line
 * any easier to read. It used to widen to `max-w-6xl` with no rail open, so
 * the default transcript ran edge to edge on most windows. `wide` keeps two
 * caps — choosing it is asking for the room. `full` caps nothing — the gutter
 * alone holds the text off the edge.
 *
 * These are rem quantities, never px. The desktop app implements Cmd +/- by
 * scaling the root font size, so a rem cap widens with the glyphs and holds a
 * roughly constant character count; a px cap would freeze against the zoom.
 */
export const CODING_SESSION_MEASURE_CLASSES: Record<
  CodingSessionWidth,
  { default: string; expanded: string }
> = {
  narrow: { default: "max-w-3xl", expanded: "max-w-3xl" },
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
/**
 * The cap on *prose*, in `ch`, for the Mission lens.
 *
 * L4.6 gives the Mission stream the whole width the rails leave — no centred
 * box, no `max-w-*` on the column — which is what recovers the 258 px of dead
 * margin a 1920 window was spending on nothing. That alone would hand a
 * paragraph a 1,000 px line, which is the thing a reading measure exists to
 * prevent. So the measure moves *inside* the row: prose is capped here, and
 * everything that genuinely wants the column — transaction rows, the Work
 * Log, code blocks, the Audit table — keeps all of it.
 *
 * `ch` and not `rem`: the cap is a character count, which is what the
 * readability constraint actually is, and `ch` tracks the rendered font so it
 * survives Cmd +/- exactly as a rem token does. `full` caps nothing, the same
 * way it caps nothing in the container measure above.
 *
 * Conversation does not use this. Its container cap is unchanged, byte for
 * byte, and adding a second cap inside it would narrow its text twice.
 */
export const CODING_SESSION_PROSE_MEASURE_CLASSES: Record<
  CodingSessionWidth,
  string
> = {
  // Written out as complete variant strings, never composed at runtime:
  // Tailwind generates a class only if it appears literally in the source, so
  // a `[&_...]:${measure}` template would compile to nothing and the cap would
  // silently not exist.
  narrow: "[&_.message-markdown]:max-w-[65ch]",
  wide: "[&_.message-markdown]:max-w-[85ch]",
  full: "",
};

/**
 * The width Mission reads when the viewer has never chosen one.
 *
 * The stored default is `narrow`, a measure picked for a chat transcript in a
 * centred column. Mission is a dashboard: its column is as wide as the rails
 * leave, and a 65-character paragraph inside a 1,026 px column reads as a
 * ribbon. A viewer who *has* chosen keeps their choice on both lenses — this
 * only fills the blank.
 */
export const CODING_SESSION_MISSION_DEFAULT_WIDTH: CodingSessionWidth = "wide";

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
let codingSessionWidthIsExplicit = readCodingSessionWidthIsExplicit();

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
 * Did the viewer actually choose a width, or is this the fallback?
 *
 * Mission's prose measure needs the difference: an unset preference is a
 * blank to be filled with the measure that suits the lens, while a stored one
 * is a decision that holds on both lenses. Storage that throws — a private
 * window, blocked site data — reads as "not chosen", which is the safe half:
 * the viewer sees a default rather than a value invented from an exception.
 */
function readCodingSessionWidthIsExplicit(): boolean {
  try {
    const raw = globalThis.localStorage?.getItem(STORAGE_KEY);
    return raw === "narrow" || raw === "wide" || raw === "full";
  } catch {
    return false;
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
  const nextExplicit = readCodingSessionWidthIsExplicit();
  if (
    next === codingSessionWidth &&
    nextExplicit === codingSessionWidthIsExplicit
  ) {
    return;
  }
  codingSessionWidth = next;
  codingSessionWidthIsExplicit = nextExplicit;
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
  codingSessionWidthIsExplicit = true;

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

/**
 * The prose cap for one lens, given the viewer's choice.
 *
 * Mission only. Conversation passes `false` — or, more usually, never calls
 * this at all — and keeps its container cap as its single measure.
 */
export function codingSessionProseMeasure(
  width: CodingSessionWidth,
  explicit: boolean,
): string {
  return CODING_SESSION_PROSE_MEASURE_CLASSES[
    explicit ? width : CODING_SESSION_MISSION_DEFAULT_WIDTH
  ];
}

/**
 * The Mission stream's prose cap, as a class for the column to hand down.
 *
 * Returns `""` at the Full width, which is the honest answer: that choice is
 * "no cap", and a caller that wanted one anyway would be overriding a
 * decision the viewer made on the settings page.
 */
export function useCodingSessionProseMeasure(): string {
  const width = useCodingSessionWidth();
  const explicit = React.useSyncExternalStore(
    subscribe,
    getExplicitSnapshot,
    getServerExplicitSnapshot,
  );
  return codingSessionProseMeasure(width, explicit);
}

function getExplicitSnapshot(): boolean {
  return codingSessionWidthIsExplicit;
}

function getServerExplicitSnapshot(): boolean {
  return false;
}
